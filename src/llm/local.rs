//! Local AI provider -- OpenAI-compatible REST API.
//!
//! Supports any OpenAI-compatible server:
//!   - Ollama        (http://localhost:11434)       -- native /api/generate
//!   - LM Studio     (http://localhost:1234/v1)     -- /chat/completions
//!   - llama.cpp     (http://localhost:8080/v1)     -- /chat/completions
//!
//! Format detection: if base_url ends with `/v1` we use the OpenAI
//! chat-completions format; otherwise we use the Ollama native format.

use super::{LlmProvider, TokenStream};
use crate::config::LocalConfig;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
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

    fn is_openai_compat(&self) -> bool {
        self.base_url.ends_with("/v1")
    }
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize, Debug)]
struct OllamaChunk {
    response: Option<String>,
    done: Option<bool>,
}

#[derive(Serialize)]
struct OAIRequest<'a> {
    model: &'a str,
    messages: Vec<OAIMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct OAIMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize, Debug)]
struct OAIChunk {
    choices: Option<Vec<OAIChoice>>,
}

#[derive(Deserialize, Debug)]
struct OAIChoice {
    delta: Option<OAIDelta>,
}

#[derive(Deserialize, Debug)]
struct OAIDelta {
    content: Option<String>,
}

/// Returns all model names from a running Ollama instance.
/// Returns an empty Vec if Ollama is unreachable -- not an error.
///
/// Previously used block_in_place + block_on which deadlocks on a
/// current-thread runtime. Fixed to use a direct .await chain.
pub async fn list_ollama_models(base_url: &str) -> Vec<String> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));

    #[derive(Deserialize)]
    struct Tags {
        models: Vec<ModelEntry>,
    }
    #[derive(Deserialize)]
    struct ModelEntry {
        name: String,
    }

    let Ok(response) = Client::new().get(&url).send().await else {
        return Vec::new();
    };
    let Ok(tags) = response.json::<Tags>().await else {
        return Vec::new();
    };
    tags.models.into_iter().map(|m| m.name).collect()
}

#[async_trait]
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
    async fn stream_ollama(&self, prompt: &str) -> Result<TokenStream> {
        let url = format!("{}/api/generate", self.base_url);
        debug!("Ollama generate: POST {}", url);

        let response = self
            .client
            .post(&url)
            .json(&OllamaRequest {
                model: &self.model,
                prompt,
                stream: true,
            })
            .send()
            .await
            .context("Cannot reach Ollama. Is it running? Try: ollama serve")?;

        if !response.status().is_success() {
            let s = response.status();
            let b = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama error {}: {}", s, b);
        }

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
                    if c.done.unwrap_or(false) {
                        break;
                    }
                }
            }
            if tokens.is_empty() { None } else { Some(Ok(tokens)) }
        });

        Ok(Box::pin(token_stream))
    }

    async fn stream_openai(&self, prompt: &str) -> Result<TokenStream> {
        let url = format!("{}/chat/completions", self.base_url);
        debug!("OpenAI-compat: POST {}", url);

        let response = self
            .client
            .post(&url)
            .json(&OAIRequest {
                model: &self.model,
                messages: vec![OAIMessage {
                    role: "user",
                    content: prompt,
                }],
                stream: true,
            })
            .send()
            .await
            .context("Cannot reach OpenAI-compatible server")?;

        if !response.status().is_success() {
            let s = response.status();
            let b = response.text().await.unwrap_or_default();
            anyhow::bail!("Local API error {}: {}", s, b);
        }

        let byte_stream = response.bytes_stream();
        let token_stream = byte_stream.filter_map(|chunk| async move {
            let bytes = chunk.ok()?;
            let text = std::str::from_utf8(&bytes).ok()?.to_string();
            let mut tokens = String::new();
            for line in text.lines() {
                let Some(json_str) = line.strip_prefix("data: ") else {
                    continue;
                };
                if json_str.trim() == "[DONE]" {
                    break;
                }
                if let Ok(c) = serde_json::from_str::<OAIChunk>(json_str) {
                    for choice in c.choices.into_iter().flatten() {
                        if let Some(delta) = choice.delta {
                            if let Some(t) = delta.content {
                                tokens.push_str(&t);
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
