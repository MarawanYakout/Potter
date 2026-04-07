//! Potter -- LLM quick-access overlay daemon
//!
//! Entry point. Initialises logging, loads config, and starts the GTK4
//! application. The hotkey listener runs on a dedicated OS thread and
//! communicates with the GTK main thread via a glib channel.

mod config;
mod history;
mod hotkey;
mod llm;
mod window;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // Initialise structured logging.
    // Set RUST_LOG=potter=debug for verbose output.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("potter=info".parse()?),
        )
        .init();

    info!("Potter starting up...");

    // Load (or create) ~/.config/potter/config.toml
    let cfg = config::Config::load()?;
    info!("Config loaded. Default model: {}", cfg.defaults.model);

    // Build and run the GTK4 application.
    // This call blocks until the user quits.
    window::run_app(cfg)?;

    Ok(())
}
