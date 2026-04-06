//! LLM provider trait and @prefix router.
//!
//! Any backend (Gemini, Claude, Local) implements the `LlmProvider` trait.
//! `route_prompt` parses the optional `@prefix` from the user's input and
//! dispatches to the correct provider, returning a stream of text chunks.

pub mod claude;
pub mod gemini;
pub mod local;

use anyhow::Result;
use futures_util::Stream;
use std::pin::Pin;

/// A streaming response: an async stream of `String` chunks.
pub type TokenStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

/// Common interface for all LLM backends.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Returns the short name of this provider (e.g. "gemini", "claude", "local").
    fn name(&self) -> &str;

    /// Sends `prompt` and returns a streaming sequence of token chunks.
    async fn stream(&self, prompt: &str) -> Result<TokenStream>;

    /// Non-streaming fallback — collects the full stream into a single String.
    async fn complete(&self, prompt: &str) -> Result<String> {
        use futures_util::StreamExt;
        let mut stream = self.stream(prompt).await?;
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            buf.push_str(&chunk?);
        }
        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// @prefix parser
// ---------------------------------------------------------------------------

/// The result of parsing a user's raw input string.
#[derive(Debug, PartialEq)]
pub struct ParsedPrompt {
    /// Which provider was requested ("gemini", "claude", "local", or the
    /// default from config when no prefix is present).
    pub provider: String,
    /// Optional model override for local provider, e.g. `@local:mistral`.
    pub model_override: Option<String>,
    /// The actual prompt text after stripping the @prefix.
    pub text: String,
}

/// Parses `@prefix` from the start of a raw input string.
///
/// Examples:
/// - `"@gemini What is Rust?"` → provider = "gemini", text = "What is Rust?"
/// - `"@local:mistral Explain RLHF"` → provider = "local", model_override = Some("mistral")
/// - `"Hello world"` → provider = default_provider (from config)
pub fn parse_prompt(raw: &str, default_provider: &str) -> ParsedPrompt {
    let trimmed = raw.trim();

    if let Some(rest) = trimmed.strip_prefix('@') {
        // Split on first space to separate prefix from prompt body
        let (prefix_part, text) = rest
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((rest, ""));

        // Handle @local:modelname
        let (provider_str, model_override) = if let Some((p, m)) = prefix_part.split_once(':') {
            (p.to_string(), Some(m.to_string()))
        } else {
            (prefix_part.to_string(), None)
        };

        ParsedPrompt {
            provider: provider_str,
            model_override,
            text: text.to_string(),
        }
    } else {
        ParsedPrompt {
            provider: default_provider.to_string(),
            model_override: None,
            text: trimmed.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Routes a raw user prompt to the correct `LlmProvider` and returns a
/// token stream. Providers are constructed from the config on each call
/// (they are lightweight — no persistent connections).
pub async fn route_prompt(
    raw: &str,
    cfg: &crate::config::Config,
) -> Result<TokenStream> {
    let parsed = parse_prompt(raw, &cfg.defaults.model);

    match parsed.provider.as_str() {
        "gemini" => {
            let provider = gemini::GeminiProvider::new(&cfg.gemini);
            provider.stream(&parsed.text).await
        }
        "claude" => {
            let provider = claude::ClaudeProvider::new(&cfg.claude);
            provider.stream(&parsed.text).await
        }
        "local" => {
            let mut local_cfg = cfg.local.clone();
            if let Some(model) = parsed.model_override {
                local_cfg.model = model;
            }
            let provider = local::LocalProvider::new(&local_cfg);
            provider.stream(&parsed.text).await
        }
        other => anyhow::bail!("Unknown provider '{}'. Use @gemini, @claude, or @local.", other),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gemini_prefix() {
        let p = parse_prompt("@gemini What is Rust?", "local");
        assert_eq!(p.provider, "gemini");
        assert_eq!(p.text, "What is Rust?");
        assert!(p.model_override.is_none());
    }

    #[test]
    fn parse_local_with_model() {
        let p = parse_prompt("@local:mistral Explain transformers", "gemini");
        assert_eq!(p.provider, "local");
        assert_eq!(p.model_override, Some("mistral".to_string()));
        assert_eq!(p.text, "Explain transformers");
    }

    #[test]
    fn parse_no_prefix_uses_default() {
        let p = parse_prompt("Hello world", "claude");
        assert_eq!(p.provider, "claude");
        assert_eq!(p.text, "Hello world");
    }

    #[test]
    fn parse_prefix_only_no_text() {
        let p = parse_prompt("@gemini", "local");
        assert_eq!(p.provider, "gemini");
        assert_eq!(p.text, "");
    }
}
