//! Transcription locale : `whisper.cpp` (via `whisper-rs`) derrière un trait simple.

use anyhow::Context;
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
