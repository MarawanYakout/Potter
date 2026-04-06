//! Configuration loader.
//!
//! Reads `~/.config/potter/config.toml`. If the file doesn't exist it is
//! created with sensible defaults so the user has a starting template.

use anyhow::{Context, Result};
use dirs::config_dir;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::info;

// ---------------------------------------------------------------------------
// Top-level config struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub defaults: Defaults,
    pub gemini: GeminiConfig,
    pub claude: ClaudeConfig,
    pub local: LocalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    /// Which provider to use when no @prefix is given.
    /// Accepted values: "gemini" | "claude" | "local"
    pub model: String,
    /// Global hotkey string, e.g. "alt+space" or "option+space"
    pub hotkey: String,
    /// Window anchor: "top-right" | "bottom-right" | "center"
    pub window_position: String,
    /// Maximum number of prompts to keep in history
    pub max_history: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    /// Path to the claude binary. Defaults to "claude" (assumes it's in PATH).
    pub binary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalConfig {
    /// Base URL of the OpenAI-compatible server.
    /// Ollama default: http://localhost:11434
    /// LM Studio default: http://localhost:1234/v1
    pub base_url: String,
    /// Model name to request (e.g. "llama3.2", "mistral", "phi3")
    pub model: String,
}

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

impl Default for Config {
    fn default() -> Self {
        Self {
            defaults: Defaults {
                model: "gemini".into(),
                hotkey: "alt+space".into(),
                window_position: "top-right".into(),
                max_history: 100,
            },
            gemini: GeminiConfig {
                api_key: "YOUR_GEMINI_API_KEY".into(),
                model: "gemini-2.0-flash".into(),
            },
            claude: ClaudeConfig {
                binary: "claude".into(),
            },
            local: LocalConfig {
                base_url: "http://localhost:11434".into(),
                model: "llama3.2".into(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Load / save helpers
// ---------------------------------------------------------------------------

impl Config {
    /// Returns the path to `~/.config/potter/config.toml`.
    pub fn path() -> Result<PathBuf> {
        let mut p = config_dir().context("Cannot determine config directory")?;
        p.push("potter");
        p.push("config.toml");
        Ok(p)
    }

    /// Loads the config from disk. Creates the file with defaults if absent.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            Self::create_default(&path)?;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config at {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&content).context("Config file contains invalid TOML")?;
        Ok(cfg)
    }

    /// Writes default config to `path`, creating parent directories as needed.
    fn create_default(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let default_toml = toml::to_string_pretty(&Config::default())?;
        fs::write(path, &default_toml)?;
        info!("Created default config at {}", path.display());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_serializes() {
        let cfg = Config::default();
        let s = toml::to_string_pretty(&cfg).unwrap();
        assert!(s.contains("gemini"));
    }

    #[test]
    fn default_config_round_trips() {
        let cfg = Config::default();
        let s = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&s).unwrap();
        assert_eq!(parsed.defaults.model, "gemini");
        assert_eq!(parsed.local.base_url, "http://localhost:11434");
    }
}
