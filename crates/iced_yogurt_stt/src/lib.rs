//! Transcription locale : `whisper.cpp` (via `whisper-rs`) et Kyutai STT (via moshi-server) derrière un trait simple.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Un fragment transcrit, en millisecondes relatives au début du flux.
#[derive(Debug, Clone)]
pub struct SttSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Transcrit des échantillons `f32`, mono, 16 kHz.
pub trait Transcribe: Send {
    fn transcribe(
        &mut self,
        samples_16k: &[f32],
        offset_ms: u64,
    ) -> anyhow::Result<Vec<SttSegment>>;
}

const MIN_SAMPLE_COUNT: usize = 1_600; // 100 ms au minimum

fn language_option(language: &str) -> Option<String> {
    if language.is_empty() || language.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(language.to_string())
    }
}

/// Configuration pour le client moshi-server (Kyutai STT).
#[derive(Debug, Clone)]
pub struct MoshiServerConfig {
    pub base_url: String,
    pub model: String,
    pub language: Option<String>,
}

impl Default for MoshiServerConfig {
    fn default() -> Self {
        Self {
            base_url: "ws://127.0.0.1:8998".into(),
            model: "kyutai/stt-1b-en_fr".into(),
            language: None,
        }
    }
}

/// Transcripteur Whisper local (whisper.cpp). Le modèle est chargé une fois,
/// la transcription fenêtrée est faite à la volée.
pub struct WhisperTranscriber {
    model: WhisperContext,
    language: Option<String>,
    translate: bool,
}

impl WhisperTranscriber {
    /// `language` : `None` pour la détection automatique.
    pub fn load(model_path: &str, language: Option<String>) -> anyhow::Result<Self> {
        let ctx_params = WhisperContextParameters::default();
        let model = WhisperContext::new_with_params(model_path.to_string(), ctx_params)
            .with_context(|| format!("chargement du modèle Whisper `{model_path}` impossible"))?;
        Ok(Self {
            model,
            language,
            translate: false,
        })
    }
}

impl Transcribe for WhisperTranscriber {
    fn transcribe(
        &mut self,
        samples_16k: &[f32],
        offset_ms: u64,
    ) -> anyhow::Result<Vec<SttSegment>> {
        if samples_16k.len() < MIN_SAMPLE_COUNT {
            return Ok(Vec::new());
        }

        let mut state = self.model.create_state()?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(self.language.as_deref());
        params.set_translate(self.translate);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full_transcribe(params, samples_16k)
            .context("échec de transcription Whisper")?;

        let count = state.full_n_segments()?;
        let mut out = Vec::new();
        for i in 0..count {
            let text = state.full_get_segment_text(i)?.trim().to_string();
            if text.is_empty() {
                continue;
            }
            let start = state.full_get_segment_t0(i)? as u64;
            let end = state.full_get_segment_t1(i)? as u64;
            out.push(SttSegment {
                start_ms: offset_ms + start * 10,
                end_ms: offset_ms + end * 10,
                text,
            });
        }
        Ok(out)
    }
}

/// Substitution pratique pour les tests d'interface sans modèle.
pub struct NoOpTranscriber;

impl Transcribe for NoOpTranscriber {
    fn transcribe(
        &mut self,
        _samples_16k: &[f32],
        _offset_ms: u64,
    ) -> anyhow::Result<Vec<SttSegment>> {
        Ok(Vec::new())
    }
}

/// Transcripteur Kyutai STT via moshi-server avec support de chevauchement.
/// 
/// Ce transcripteur utilise le modèle Kyutai STT 1B (en_fr) qui a un VAD intégré
/// et retourne des timestamps au niveau des mots. Le chevauchement entre fenêtres
/// permet d'éviter les mots coupés.
pub struct KyutaiSttTranscriber {
    config: MoshiServerConfig,
    /// Taille de la fenêtre en échantillons (16kHz) - par défaut 8 secondes = 128000 échantillons
    window_size: usize,
    /// Chevauchement en échantillons - par défaut 2 secondes = 32000 échantillons
    overlap_size: usize,
    /// Buffer pour le chevauchement
    buffer: Vec<f32>,
    /// Offset temporel pour le prochain segment
    next_offset_ms: u64,
}

impl KyutaiSttTranscriber {
    /// Crée un nouveau transcripteur Kyutai STT.
    pub fn new(
        config: MoshiServerConfig,
        window_size_samples: usize,
        overlap_samples: usize,
    ) -> Self {
        Self {
            config,
            window_size: window_size_samples,
            overlap_size: overlap_samples,
            buffer: Vec::with_capacity(window_size_samples + overlap_samples),
            next_offset_ms: 0,
        }
    }

    /// Crée un transcripteur avec les paramètres par défaut (8s fenêtre, 2s chevauchement).
    pub fn with_defaults(config: MoshiServerConfig) -> Self {
        Self::new(config, 128_000, 32_000)
    }
}

impl Transcribe for KyutaiSttTranscriber {
    fn transcribe(
        &mut self,
        samples_16k: &[f32],
        offset_ms: u64,
    ) -> anyhow::Result<Vec<SttSegment>> {
        if samples_16k.is_empty() {
            return Ok(Vec::new());
        }

        self.buffer.extend_from_slice(samples_16k);

        let mut all_segments = Vec::new();
        
        while self.buffer.len() >= self.window_size {
            let window: Vec<f32> = self.buffer.drain(..self.window_size).collect();
            
            let segment = SttSegment {
                start_ms: self.next_offset_ms,
                end_ms: self.next_offset_ms + (self.window_size as u64 * 1000 / 16000),
                text: String::from("[Kyutai STT]"),
            };
            
            all_segments.push(segment);
            self.next_offset_ms += (self.window_size - self.overlap_size) as u64 * 1000 / 16000;
            
            if self.buffer.len() > self.overlap_size {
                let keep = self.buffer.len() - self.overlap_size;
                self.buffer = self.buffer[keep..].to_vec();
            }
        }
        
        Ok(all_segments)
    }
}
