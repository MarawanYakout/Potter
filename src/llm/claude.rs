//! Anthropic Claude provider -- CLI subprocess wrapper.
//!
//! Spawns the `claude` binary (from the Claude Code CLI) as a child process
//! and streams its stdout line-by-line back as token chunks.
//!
//! Prerequisites:
//!   - `claude` CLI installed: https://docs.anthropic.com/en/docs/claude-code
//!   - Authenticated: run `claude login` once

use super::{LlmProvider, TokenStream};
use crate::config::ClaudeConfig;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::stream;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::debug;

pub struct ClaudeProvider {
    binary: String,
}

impl ClaudeProvider {
    pub fn new(cfg: &ClaudeConfig) -> Self {
        Self {
            binary: cfg.binary.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    async fn stream(&self, prompt: &str) -> Result<TokenStream> {
        debug!("Spawning claude subprocess: {} -p <prompt>", self.binary);

        let mut child = Command::new(&self.binary)
            .arg("-p")
            .arg(prompt)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            // kill_on_drop ensures the subprocess is sent SIGKILL when the
            // Child handle is dropped -- i.e. when the user presses Escape
            // mid-response and the stream is abandoned. Without this the
            // process would continue running in the background as a zombie.
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn '{}'. Is the Claude CLI installed and in PATH?",
                    self.binary
                )
            })?;

        let stdout = child
            .stdout
            .take()
            .context("Failed to acquire claude stdout handle")?;

        // Move the Child into the stream state tuple so it stays alive
        // (and kill_on_drop fires) until the stream is exhausted or dropped.
        let reader = BufReader::new(stdout);
        let lines = reader.lines();

        let token_stream = stream::unfold(
            (lines, child),
            |(mut lines, child)| async move {
                match lines.next_line().await {
                    Ok(Some(line)) => Some((Ok(format!("{line}\n")), (lines, child))),
                    Ok(None) => None,
                    Err(e) => Some((
                        Err(anyhow::anyhow!("claude read error: {}", e)),
                        (lines, child),
                    )),
                }
            },
        );

        Ok(Box::pin(token_stream))
    }
}
