//! Client léger pour un serveur local compatible OpenAI : Ollama,
//! `llama-server`, LM Studio, vLLM, etc. Aucun appel externe au-delà de
//! l'URL configurée par l'utilisateur.

use std::time::Duration;

use anyhow::{anyhow, Context};
use serde_json::json;

#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
}

impl OpenAiClient {
    /// `base_url` inclut la racine de l'API, ex. `http://127.0.0.1:11434/v1`.
    pub fn new(base_url: &str, api_key: &str, model: &str) -> anyhow::Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let mut headers = reqwest::header::HeaderMap::new();
        let value = format!("Bearer {api_key}");
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&value)
                .map_err(|e| anyhow!("clé d'API invalide : {e}"))?,
        );
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self {
            http,
            base_url,
            model: model.to_string(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Un appel `chat/completions` minimaliste, sans streaming.
    pub async fn chat(&self, system: &str, user: &str) -> anyhow::Result<String> {
        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
            "temperature": 0.4,
            "stream": false,
        });

        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
            .context("serveur LLM injoignable (lancez Ollama ou llama-server ?)")?;
        let status = response.status();
        let payload: serde_json::Value = response
            .json()
            .await
            .context("réponse du serveur LLM illisible")?;
        if !status.is_success() {
            return Err(anyhow!("le serveur LLM a répondu {status} : {payload}"));
        }
        payload["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("réponse inattendue du serveur LLM : {payload}"))
    }
}

/// Les prompts, isolés pour rester faciles à tester et à ajuster.
pub mod prompts {
    pub fn summary_system() -> &'static str {
        "Tu es un assistant qui rédige des comptes rendus de réunion. \
Réponds dans la langue principale du texte fourni. \
Structure attendue : un titre court, une liste de points clés, puis une liste \
d'actions avec leur responsable quand il est identifiable. Sois factuel, \
n'invente rien et appuie-toi uniquement sur la transcription."
    }

    pub fn summary_user(transcript: &str, notes: Option<&str>) -> String {
        let mut prompt = String::from("Transcription de la réunion :\n\n");
        prompt.push_str(transcript);
        if let Some(notes) = notes {
            if !notes.trim().is_empty() {
                prompt.push_str("\n\nNotes prises par l'utilisateur pendant la réunion :\n");
                prompt.push_str(notes);
            }
        }
        prompt.push_str("\n\nRédige le compte rendu.");
        prompt
    }

    pub fn answer_system() -> &'static str {
        "Tu réponds aux questions de l'utilisateur à partir d'extraits de \
réunions. Cite brièvement les passages qui appuient ta réponse. Réponds dans \
la langue de la question."
    }

    pub fn answer_user(question: &str, transcript: &str) -> String {
        format!("Transcription :\n\n{transcript}\n\nQuestion : {question}")
    }
}

#[cfg(test)]
mod tests {
    use super::prompts;

    #[test]
    fn summary_user_inclut_la_transcription_et_les_notes() {
        let p = prompts::summary_user("Bonjour", Some("note"));
        assert!(p.contains("Bonjour") && p.contains("note"));
    }
}
