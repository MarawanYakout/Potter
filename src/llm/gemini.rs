//! Google Gemini provider.
//!
//! Streams responses from the Gemini REST API using server-sent events (SSE).
//!
//! Endpoint:
//!   POST https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent

use super::{LlmProvider, TokenStream};
use crate::config::GeminiConfig;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl GeminiProvider {
    pub fn new(cfg: &GeminiConfig) -> Self {
        Self {
            api_key: cfg.api_key.clone(),
            model: cfg.model.clone(),
            client: Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.model, self.api_key
        )
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
    role: String,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Deserialize, Debug)]
struct GeminiStreamChunk {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize, Debug)]
struct CandidateContent {
    parts: Option<Vec<CandidatePart>>,
}

#[derive(Deserialize, Debug)]
struct CandidatePart {
    text: Option<String>,
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl LlmProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    async fn stream(&self, prompt: &str) -> Result<TokenStream> {
        let body = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
                role: "user".to_string(),
            }],
        };

        let response = self
            .client
            .post(self.endpoint())
            .json(&body)
            .send()
            .await
            .context("Failed to reach Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error {}: {}", status, body_text);
        }

        debug!("Gemini SSE stream opened for model '{}'", self.model);

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
                if let Ok(chunk) = serde_json::from_str::<GeminiStreamChunk>(json_str) {
                    for candidate in chunk.candidates.into_iter().flatten() {
                        for part in candidate
                            .content
                            .into_iter()
                            .flat_map(|c| c.parts.into_iter().flatten())
                        {
                            if let Some(t) = part.text {
                                tokens.push_str(&t);
                            }
                        }
                    }
                }
            }

            if tokens.is_empty() {
                None
            } else {
                Some(Ok(tokens))
            }
        });

        Ok(Box::pin(token_stream))
    }
}
