//! Claude CLI provider.
//!
//! Spawns the `claude` binary as a subprocess and captures its stdout stream.
//! The Claude CLI must be installed and authenticated (`claude login`).
//!
//! Usage of the CLI: `claude -p "<prompt>"`

use super::{LlmProvider, TokenStream};
use crate::config::ClaudeConfig;
use anyhow::{Context, Result};
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

#[async_trait::async_trait]
impl LlmProvider for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    async fn stream(&self, prompt: &str) -> Result<TokenStream> {
        debug!("Spawning claude subprocess");

        let mut child = Command::new(&self.binary)
            .arg("-p")
            .arg(prompt)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
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
            .context("Failed to capture claude stdout")?;

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Stream each line of stdout as a token chunk.
        // Claude CLI outputs the response incrementally line by line.
        let token_stream = stream::unfold(lines, |mut lines| async move {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let chunk = format!("{line}\n");
                    Some((Ok(chunk), lines))
                }
                Ok(None) => None, // EOF
                Err(e) => Some((Err(anyhow::anyhow!(e)), lines)),
            }
        });

        // Ensure the child process is awaited after the stream ends
        // (fire-and-forget; process will be cleaned up by the OS otherwise)
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        Ok(Box::pin(token_stream))
    }
}
