//! Google Gemini provider.
//!
//! Uses the Gemini REST API with server-sent events (SSE) for streaming.
//! Endpoint: POST https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent

use super::{LlmProvider, TokenStream};
use crate::config::GeminiConfig;
use anyhow::{Context, Result};
use futures_util::stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

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
// Request / Response types
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
// LlmProvider impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
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
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error {}: {}", status, body);
        }

        debug!("Gemini stream opened");

        // Parse the SSE stream line by line
        use futures_util::StreamExt;
        let byte_stream = response.bytes_stream();

        let token_stream = byte_stream.filter_map(|chunk| async move {
            let bytes = chunk.ok()?;
            let text = std::str::from_utf8(&bytes).ok()?.to_string();

            // SSE lines look like: `data: {json}` or `data: [DONE]`
            let mut tokens = String::new();
            for line in text.lines() {
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str.trim() == "[DONE]" {
                        break;
                    }
                    if let Ok(chunk) = serde_json::from_str::<GeminiStreamChunk>(json_str) {
                        if let Some(candidates) = chunk.candidates {
                            for c in candidates {
                                if let Some(content) = c.content {
                                    if let Some(parts) = content.parts {
                                        for p in parts {
                                            if let Some(t) = p.text {
                                                tokens.push_str(&t);
                                            }
                                        }
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
