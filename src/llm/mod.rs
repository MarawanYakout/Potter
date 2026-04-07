//! LLM provider trait and @prefix router.
//!
//! Any backend (Gemini, Claude, Local) implements the `LlmProvider` trait.
//! `route_prompt` parses the optional `@prefix` from the user's input and
//! dispatches to the correct provider, returning an async stream of tokens.

pub mod claude;
pub mod gemini;
pub mod local;

use anyhow::Result;
use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;

/// A streaming response: an async stream of `String` token chunks.
pub type TokenStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

/// Common interface for all LLM backends.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Returns the short name of this provider (e.g. "gemini", "claude", "local").
    fn name(&self) -> &str;

    /// Sends `prompt` and returns a streaming sequence of token chunks.
    async fn stream(&self, prompt: &str) -> Result<TokenStream>;

    /// Non-streaming convenience -- collects the full stream into one String.
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
    /// Which provider was requested.
    pub provider: String,
    /// Optional model override for the local provider (e.g. `@local:mistral`).
    pub model_override: Option<String>,
    /// The actual prompt text after stripping the @prefix.
    pub text: String,
}

/// Parses an optional `@prefix` from the start of a raw input string.
///
/// Examples:
/// - `"@gemini What is Rust?"`        -> provider="gemini",  text="What is Rust?"
/// - `"@local:mistral Explain RLHF"` -> provider="local",   model_override=Some("mistral")
/// - `"Hello world"`                 -> provider=default,   text="Hello world"
pub fn parse_prompt(raw: &str, default_provider: &str) -> ParsedPrompt {
    let trimmed = raw.trim();

    if let Some(rest) = trimmed.strip_prefix('@') {
        let (prefix_part, text) = rest
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((rest, ""));

        let (provider_str, model_override) =
            if let Some((p, m)) = prefix_part.split_once(':') {
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

/// Routes a raw user prompt string to the correct `LlmProvider` and
/// returns a live token stream.
///
/// Provider instances are constructed fresh per call -- they are
/// stateless wrappers around an HTTP client.
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
        other => anyhow::bail!(
            "Unknown provider '@{}'. Valid prefixes: @gemini, @claude, @local.",
            other
        ),
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
    fn parse_local_with_model_override() {
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
    fn parse_prefix_only_no_body() {
        let p = parse_prompt("@gemini", "local");
        assert_eq!(p.provider, "gemini");
        assert_eq!(p.text, "");
    }

    #[test]
    fn parse_strips_leading_whitespace() {
        let p = parse_prompt("  hello  ", "gemini");
        assert_eq!(p.text, "hello");
    }
}
