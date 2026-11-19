//! Capture du microphone, rééchantillonnage vers 16 kHz et export WAV.
//!
//! Taux de référence : Whisper attend des échantillons `f32`, mono, 16 kHz.

use std::sync::mpsc;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

pub type Result<T> = anyhow::Result<T>;

/// Nom du périphérique d'entrée par défaut, pour l'afficher dans l'interface.
pub fn default_input_name() -> Option<String> {
    #[cfg(feature = "real-audio")]
    {
        cpal::default_host()
            .default_input_device()
            .and_then(|d| d.name().ok())
    }
    #[cfg(not(feature = "real-audio"))]
    {
        None
    }
}

/// Capture du microphone par défaut. Les échantillons `f32` au taux natif de
/// l'appareil arrivent en lots dans un canal, que le consommateur rééchantillonne
/// vers 16 kHz avec [`Resampler`].
pub struct Recorder {
    #[cfg(feature = "real-audio")]
    _stream: cpal::Stream,
    #[cfg(feature = "real-audio")]
    receiver: Option<mpsc::Receiver<Vec<f32>>>,
    #[cfg(feature = "real-audio")]
    pub sample_rate: u32,
    #[cfg(not(feature = "real-audio"))]
    _dummy: (),
}

impl Recorder {
    pub fn start() -> Result<Self> {
        #[cfg(feature = "real-audio")]
        {
            let host = cpal::default_host();
            let device = host
                .default_input_device()
                .ok_or_else(|| anyhow::anyhow!("aucun microphone disponible"))?;
            let supported = device.default_input_config()?;
            let sample_rate = supported.sample_rate().0;
            let config: cpal::StreamConfig = supported.into();

            let (tx, rx) = mpsc::channel::<Vec<f32>>();
            let stream = device.build_input_stream(
                &config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.to_vec());
                },
                move |err| eprintln!("erreur de capture audio : {err}"),
                None,
            )?;
            stream.play()?;

            Ok(Self {
                _stream: stream,
                receiver: Some(rx),
                sample_rate,
            })
        }
        #[cfg(not(feature = "real-audio"))]
        {
            // Mock pour la CI - retourne un recorder vide
            Ok(Self { _dummy: () })
        }
    }

    /// Transfère le récepteur d'échantillons au fil des sessions ; appelé une fois.
    pub fn get_sample_rate(&self) -> u32 {
        #[cfg(feature = "real-audio")]
        {
            self.sample_rate
        }
        #[cfg(not(feature = "real-audio"))]
        {
            TARGET_SAMPLE_RATE
        }
    }

    pub fn take_receiver(&mut self) -> mpsc::Receiver<Vec<f32>> {
        #[cfg(feature = "real-audio")]
        {
            self.receiver
                .take()
                .expect("le récepteur audio n'est fourni qu'une fois")
        }
        #[cfg(not(feature = "real-audio"))]
        {
            // Mock - retourne un canal vide
            let (_, rx) = mpsc::channel::<Vec<f32>>();
            rx
        }
    }
}

/// Rééchantillonneur linéaire minimal (taux natif vers 16 kHz). Suffisant pour
/// de la transcription ; à remplacer par un vrai filtre plus tard (feuille de route).
pub struct Resampler {
    ratio: f64,
    next_output: f64,
    base: f64,
    prev: Option<f32>,
}

impl Resampler {
    pub fn new(from_sample_rate: u32) -> Self {
        Self {
            ratio: f64::from(from_sample_rate) / f64::from(TARGET_SAMPLE_RATE),
            next_output: 0.0,
            base: 0.0,
            prev: None,
        }
    }

    /// Traite un lot d'échantillons d'entrée et renvoie les échantillons 16 kHz correspondants.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity((input.len() as f64 / self.ratio) as usize + 2);
        for (k, &x) in input.iter().enumerate() {
            let i = self.base + k as f64;
            if i == 0.0 {
                out.push(x);
                self.next_output += self.ratio;
            } else {
                while self.next_output <= i {
                    let frac = (self.next_output - (i - 1.0)) as f32;
                    let p = self.prev.unwrap_or(x);
                    out.push(p + frac * (x - p));
                    self.next_output += self.ratio;
                }
            }
            self.prev = Some(x);
        }
        self.base += input.len() as f64;
        out
    }
}

/// Écrit des échantillons 16 kHz en WAV 16 bits monophonique.
pub fn write_wav(path: &std::path::Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        let value = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        writer.write_sample(value)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(feature = "real-audio")]
mod real_audio {
    pub use cpal::*;
}
