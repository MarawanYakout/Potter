//! Local AI provider — OpenAI-compatible API.
//!
//! Supports any server that speaks the OpenAI chat-completions streaming API:
//! - Ollama        (default: http://localhost:11434)
//! - LM Studio     (default: http://localhost:1234/v1)
//! - llama.cpp     (default: http://localhost:8080)
//!
//! Endpoint: POST {base_url}/api/generate (Ollama native)
//!           POST {base_url}/chat/completions (OpenAI-compat)
//!
//! We detect which format to use based on whether the base_url ends with `/v1`.

use super::{LlmProvider, TokenStream};
use crate::config::LocalConfig;
use anyhow::{Context, Result};
use futures_util::stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

pub struct LocalProvider {
    base_url: String,
    model: String,
    client: Client,
}

impl LocalProvider {
    pub fn new(cfg: &LocalConfig) -> Self {
        Self {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            client: Client::new(),
        }
    }

    /// Returns true if the base_url looks like an OpenAI-compatible endpoint.
    fn is_openai_compat(&self) -> bool {
        self.base_url.ends_with("/v1")
    }
}

// ---------------------------------------------------------------------------
// Ollama native API types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize, Debug)]
struct OllamaChunk {
    response: Option<String>,
    done: Option<bool>,
}

// ---------------------------------------------------------------------------
// OpenAI-compatible API types (LM Studio / llama.cpp)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    stream: bool,
}

#[derive(Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize, Debug)]
struct OpenAIChunk {
    choices: Option<Vec<OpenAIChoice>>,
}

#[derive(Deserialize, Debug)]
struct OpenAIChoice {
    delta: Option<OpenAIDelta>,
}

#[derive(Deserialize, Debug)]
struct OpenAIDelta {
    content: Option<String>,
}

// ---------------------------------------------------------------------------
// Model discovery (Ollama)
// ---------------------------------------------------------------------------

/// Lists all models available in a running Ollama instance.
/// Returns an empty vec if Ollama is not reachable.
pub async fn list_ollama_models(base_url: &str) -> Vec<String> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let client = Client::new();
    let Ok(resp) = client.get(&url).send().await else {
        return vec![];
    };
    #[derive(Deserialize)]
    struct TagsResponse {
        models: Vec<ModelEntry>,
    }
    #[derive(Deserialize)]
    struct ModelEntry {
        name: String,
    }
    resp.json::<TagsResponse>()
        .await
        .map(|r| r.models.into_iter().map(|m| m.name).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// LlmProvider impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl LlmProvider for LocalProvider {
    fn name(&self) -> &str {
        "local"
    }

    async fn stream(&self, prompt: &str) -> Result<TokenStream> {
        if self.is_openai_compat() {
            self.stream_openai(prompt).await
        } else {
            self.stream_ollama(prompt).await
        }
    }
}

impl LocalProvider {
    /// Streams via the Ollama native `/api/generate` endpoint.
    async fn stream_ollama(&self, prompt: &str) -> Result<TokenStream> {
        let url = format!("{}/api/generate", self.base_url);
        debug!("Ollama request to {}", url);

        let body = OllamaRequest {
            model: self.model.clone(),
            prompt: prompt.to_string(),
            stream: true,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to reach Ollama. Is it running? (`ollama serve`)")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama error {}: {}", status, body);
        }

        use futures_util::StreamExt;
        let byte_stream = response.bytes_stream();

        let token_stream = byte_stream.filter_map(|chunk| async move {
            let bytes = chunk.ok()?;
            let text = std::str::from_utf8(&bytes).ok()?.to_string();
            let mut tokens = String::new();
            for line in text.lines() {
                if let Ok(c) = serde_json::from_str::<OllamaChunk>(line) {
                    if let Some(t) = c.response {
                        tokens.push_str(&t);
                    }
                }
            }
            if tokens.is_empty() { None } else { Some(Ok(tokens)) }
        });

        Ok(Box::pin(token_stream))
    }

    /// Streams via the OpenAI-compatible `/chat/completions` endpoint.
    async fn stream_openai(&self, prompt: &str) -> Result<TokenStream> {
        let url = format!("{}/chat/completions", self.base_url);
        debug!("OpenAI-compat request to {}", url);

        let body = OpenAIRequest {
            model: self.model.clone(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            stream: true,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to reach OpenAI-compatible server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Local API error {}: {}", status, body);
        }

        use futures_util::StreamExt;
        let byte_stream = response.bytes_stream();

        let token_stream = byte_stream.filter_map(|chunk| async move {
            let bytes = chunk.ok()?;
            let text = std::str::from_utf8(&bytes).ok()?.to_string();
            let mut tokens = String::new();
            for line in text.lines() {
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str.trim() == "[DONE]" { break; }
                    if let Ok(c) = serde_json::from_str::<OpenAIChunk>(json_str) {
                        if let Some(choices) = c.choices {
                            for choice in choices {
                                if let Some(delta) = choice.delta {
                                    if let Some(t) = delta.content {
                                        tokens.push_str(&t);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if tokens.is_empty() { None } else { Some(Ok(tokens)) }
        });

        Ok(Box::pin(token_stream))
    }
}
