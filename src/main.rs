//! Potter — LLM quick-access overlay daemon
//!
//! Entry point. Initialises logging, loads config, spawns the GTK4 application
//! and the background hotkey listener thread.

mod config;
mod history;
mod hotkey;
mod llm;
mod window;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise structured logging. Set RUST_LOG=debug for verbose output.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("potter=info".parse()?))
        .init();

    info!("Potter starting up...");

    // Load (or create) ~/.config/potter/config.toml
    let cfg = config::Config::load()?;
    info!("Config loaded. Default model: {}", cfg.defaults.model);

    // Start the GTK4 application (blocks until all windows are closed / daemon exits)
    window::run_app(cfg).await?;

    Ok(())
}
