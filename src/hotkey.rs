//! Global hotkey listener.
//!
//! Spawns a dedicated OS thread (required by `rdev`) that listens for key
//! events system-wide. When the configured hotkey is detected it sends a
//! signal through a `tokio::sync::mpsc` channel so the GTK4 main thread can
//! open the overlay window.

use anyhow::Result;
use rdev::{listen, Event, EventType, Key};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Signals that the overlay should be toggled.
#[derive(Debug)]
pub struct HotkeyFired;

/// Parses the configured hotkey string into a modifier + key pair.
///
/// Supported format: "alt+space", "option+space"
/// Returns `(modifier_key, trigger_key)`.
fn parse_hotkey(hotkey: &str) -> Result<(Key, Key)> {
    let parts: Vec<&str> = hotkey.to_lowercase().split('+').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid hotkey format '{}'. Expected 'modifier+key'", hotkey);
    }
    let modifier = match parts[0] {
        "alt" | "option" => Key::Alt,
        "ctrl" | "control" => Key::ControlLeft,
        "super" | "meta" | "cmd" => Key::MetaLeft,
        "shift" => Key::ShiftLeft,
        other => anyhow::bail!("Unknown modifier '{}'", other),
    };
    let trigger = match parts[1] {
        "space" => Key::Space,
        "return" | "enter" => Key::Return,
        other => anyhow::bail!("Unknown trigger key '{}'", other),
    };
    Ok((modifier, trigger))
}

/// Starts the global hotkey listener on a dedicated OS thread.
///
/// Returns a `mpsc::Receiver` that yields `HotkeyFired` each time the
/// configured shortcut is detected.
///
/// # Parameters
/// - `hotkey_str` — the string from config, e.g. `"alt+space"`
pub fn start_listener(hotkey_str: &str) -> Result<mpsc::Receiver<HotkeyFired>> {
    let (tx, rx) = mpsc::channel::<HotkeyFired>(8);
    let (modifier_key, trigger_key) = parse_hotkey(hotkey_str)?;

    // Tracks whether the modifier is currently held down.
    let modifier_held = Arc::new(AtomicBool::new(false));
    let modifier_held_clone = Arc::clone(&modifier_held);

    std::thread::spawn(move || {
        let callback = move |event: Event| {
            match event.event_type {
                EventType::KeyPress(key) if key == modifier_key => {
                    modifier_held_clone.store(true, Ordering::SeqCst);
                }
                EventType::KeyRelease(key) if key == modifier_key => {
                    modifier_held_clone.store(false, Ordering::SeqCst);
                }
                EventType::KeyPress(key)
                    if key == trigger_key
                        && modifier_held_clone.load(Ordering::SeqCst) =>
                {
                    debug!("Hotkey fired!");
                    if let Err(e) = tx.blocking_send(HotkeyFired) {
                        warn!("Failed to send hotkey event: {}", e);
                    }
                }
                _ => {}
            }
        };

        if let Err(e) = listen(callback) {
            warn!("rdev listener error: {:?}", e);
        }
    });

    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_alt_space() {
        let (modifier, trigger) = parse_hotkey("alt+space").unwrap();
        assert_eq!(modifier, Key::Alt);
        assert_eq!(trigger, Key::Space);
    }

    #[test]
    fn parse_option_space() {
        let (modifier, trigger) = parse_hotkey("option+space").unwrap();
        assert_eq!(modifier, Key::Alt);
        assert_eq!(trigger, Key::Space);
    }

    #[test]
    fn parse_invalid_format() {
        assert!(parse_hotkey("altspace").is_err());
        assert!(parse_hotkey("ctrl+alt+space").is_err());
    }
}
