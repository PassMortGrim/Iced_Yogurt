//! Session d'enregistrement : micro -> rééchantillonnage -> fenêtres -> Whisper/Kyutai STT.
//!
//! Un fil de travail consomme les échantillons du microphone, rééchantillonne
//! vers 16 kHz, transcrit par fenêtres (avec chevauchement pour Kyutai STT) et publie
//! les événements sur un canal que l'interface consomme.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use iced_yogurt_audio::{write_wav, Resampler, TARGET_SAMPLE_RATE};
use iced_yogurt_stt::{KyutaiSttTranscriber, MoshiServerConfig, Transcribe};
#[cfg(feature = "whisper")]
use iced_yogurt_stt::WhisperTranscriber;

/// Événements produits par la session vers l'interface.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Fenêtre transmise : fragments transcrits, relatifs au début du flux.
    Segments(Vec<iced_yogurt_stt::SttSegment>),
    /// Le fichier audio a été écrit (ou `None` si l'option est désactivée).
    AudioSaved(Option<String>),
    /// La session est terminée proprement.
    Finished,
    /// Avertissement ou erreur non bloquante.
    Warned(String),
}

/// Poignée renvoyée à l'interface pendant que le fil de travail tourne.
#[derive(Clone)]
pub struct Session {
    pub meeting_id: i64,
    stop: Arc<AtomicBool>,
    pub events: Arc<Mutex<mpsc::Receiver<SessionEvent>>>,
}

impl Session {
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Démarre la session : le `Recorder` (donc le stream cpal) est déplacé dans
/// le fil de travail et vit jusqu'à la fin de la session.
pub fn start(
    mut recorder: iced_yogurt_audio::Recorder,
    meeting_id: i64,
    _model_path: Option<String>,
    _language: &str,
    audio_dir: PathBuf,
    save_audio: bool,
    use_kyutai_stt: bool,
    kyutai_config: Option<MoshiServerConfig>,
    window_seconds: u64,
    overlap_seconds: u64,
) -> anyhow::Result<Session> {
    let samples_rx = recorder.take_receiver();
    let sample_rate = recorder.get_sample_rate();
    let (tx, rx) = mpsc::channel::<SessionEvent>();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_loop = Arc::clone(&stop);
    let language = _language.to_string();

    std::thread::Builder::new()
        .name("iced-yogurt-session".to_string())
        .spawn(move || {
            run(
                samples_rx,
                recorder,
                sample_rate,
                meeting_id,
                _model_path,
                language,
                audio_dir,
                save_audio,
                use_kyutai_stt,
                kyutai_config,
                window_seconds,
                overlap_seconds,
                tx,
                &stop_loop,
            );
        })?;

    Ok(Session {
        meeting_id,
        stop,
        events: Arc::new(Mutex::new(rx)),
    })
}

#[allow(clippy::too_many_arguments)]
fn run(
    samples_rx: mpsc::Receiver<Vec<f32>>,
    // Garde le stream cpal vivant jusqu'à la fin de la session.
    recorder: iced_yogurt_audio::Recorder,
    sample_rate: u32,
    meeting_id: i64,
    _model_path: Option<String>,
    _language: String,
    audio_dir: PathBuf,
    save_audio: bool,
    use_kyutai_stt: bool,
    kyutai_config: Option<MoshiServerConfig>,
    window_seconds: u64,
    overlap_seconds: u64,
    tx: mpsc::Sender<SessionEvent>,
    stop: &AtomicBool,
) {
    // Chargement du transcripteur (Whisper ou Kyutai STT).
    let mut transcriber: Option<Box<dyn Transcribe>> = None;
    
    if use_kyutai_stt {
        // Utiliser Kyutai STT avec chevauchement
        if let Some(config) = kyutai_config {
            let window_size = (window_seconds * TARGET_SAMPLE_RATE as u64) as usize;
            let overlap_size = (overlap_seconds * TARGET_SAMPLE_RATE as u64) as usize;
            transcriber = Some(Box::new(KyutaiSttTranscriber::new(
                config,
                window_size,
                overlap_size,
            )));
            let _ = tx.send(SessionEvent::Warned(
                format!("Kyutai STT activé : fenêtre={}s, chevauchement={}s", window_seconds, overlap_seconds)
            ));
        } else {
            let _ = tx.send(SessionEvent::Warned(
                "Kyutai STT sélectionné mais configuration manquante (transcription désactivée)".to_string(),
            ));
        }
    } else {
        // Utiliser Whisper (ancienne implémentation)
        #[cfg(feature = "whisper")]
        {
            match &_model_path {
                Some(path) if std::path::Path::new(path).exists() => {
                    let lang = if _language.eq_ignore_ascii_case("auto") {
                        None
                    } else {
                        Some(_language.clone())
                    };
                    match WhisperTranscriber::load(path, lang) {
                        Ok(t) => transcriber = Some(Box::new(t)),
                        Err(err) => {
                            let _ = tx.send(SessionEvent::Warned(format!("Whisper indisponible : {err}")));
                        }
                    }
                }
                Some(path) => {
                    let _ = tx.send(SessionEvent::Warned(format!(
                        "modèle Whisper introuvable : {path} (transcription désactivée)"
                    )));
                }
                None => {
                    let _ = tx.send(SessionEvent::Warned(
                        "aucun modèle Whisper configuré (transcription désactivée)".to_string(),
                    ));
                }
            }
        }
        #[cfg(not(feature = "whisper"))]
        {
            let _ = tx.send(SessionEvent::Warned(
                "Whisper non disponible (compilé sans la feature 'whisper'). Utilisez Kyutai STT.".to_string(),
            ));
        }
    }

    let mut resampler = Resampler::new(sample_rate);
    let mut pending: Vec<f32> = Vec::new();
    let mut recorded: Vec<f32> = Vec::new();
    let mut processed_ms = 0u64;
    // 8 secondes par fenêtre de transcription.
    const WINDOW: usize = TARGET_SAMPLE_RATE as usize * 8;

    let loop_result = (|| -> anyhow::Result<()> {
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            match samples_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(chunk) => {
                    let out = resampler.process(&chunk);
                    if !out.is_empty() {
                        recorded.extend_from_slice(&out);
                        pending.extend_from_slice(&out);
                    }
                    if pending.len() >= WINDOW {
                        flush(&mut pending, &mut processed_ms, transcriber.as_mut(), &tx)?;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // Dernière fenêtre, même partielle.
        flush(&mut pending, &mut processed_ms, transcriber.as_mut(), &tx)?;
        Ok(())
    })();

    if let Err(err) = loop_result {
        let _ = tx.send(SessionEvent::Warned(format!(
            "erreur pendant l'enregistrement : {err}"
        )));
    }

    // Le stream n'est plus nécessaire : le relâcher avant l'écriture du fichier.
    drop(recorder);

    // Sauvegarde éventuelle du flux 16 kHz complet.
    let audio_path = if save_audio && !recorded.is_empty() {
        let path = audio_dir.join(format!("reunion-{meeting_id}.wav"));
        match write_wav(&path, &recorded) {
            Ok(()) => Some(path.to_string_lossy().into_owned()),
            Err(err) => {
                let _ = tx.send(SessionEvent::Warned(format!(
                    "écriture WAV impossible : {err}"
                )));
                None
            }
        }
    } else {
        None
    };

    let _ = tx.send(SessionEvent::AudioSaved(audio_path));
    let _ = tx.send(SessionEvent::Finished);
}

fn flush(
    pending: &mut Vec<f32>,
    processed_ms: &mut u64,
    transcriber: Option<&mut Box<dyn Transcribe>>,
    tx: &mpsc::Sender<SessionEvent>,
) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let chunk: Vec<f32> = pending.drain(..).collect();
    let advance =
        |processed_ms: &mut u64| *processed_ms += chunk.len() as u64 * 1000 / u64::from(TARGET_SAMPLE_RATE);

    let Some(transcriber) = transcriber else {
        advance(processed_ms);
        return Ok(());
    };
    // Fenêtre trop courte : on avance quand même l'horloge.
    if chunk.len() < TARGET_SAMPLE_RATE as usize / 10 {
        advance(processed_ms);
        return Ok(());
    }

    let segments = transcriber.transcribe(&chunk, *processed_ms)?;
    if !segments.is_empty() {
        let _ = tx.send(SessionEvent::Segments(segments));
    }
    advance(processed_ms);
    Ok(())
}
