//! GTK4 overlay window.
//!
//! Creates a frameless, always-on-top, semi-transparent window anchored to
//! the top-right corner of the primary monitor. Hosts:
//!   - A text entry field for the user's prompt
//!   - A scrollable output area that streams the LLM response
//!   - A small model-selector toggle / @prefix hint
//!
//! The window hides on Escape (while focused) and can be summoned again via
//! the global hotkey without restarting.

use crate::config::Config;
use anyhow::Result;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Entry, Label,
    Orientation, PolicyType, ScrolledWindow, TextView,
};
use tracing::info;

/// CSS for the overlay window — dark, minimal, frosted-glass aesthetic.
const OVERLAY_CSS: &str = r#"
    window {
        background-color: rgba(18, 18, 20, 0.92);
        border-radius: 12px;
        border: 1px solid rgba(255, 255, 255, 0.08);
    }
    .potter-input {
        background-color: rgba(255, 255, 255, 0.06);
        color: #e8e8e8;
        border: 1px solid rgba(255, 255, 255, 0.12);
        border-radius: 8px;
        padding: 10px 14px;
        font-size: 15px;
        font-family: 'Inter', 'Segoe UI', sans-serif;
        caret-color: #4f98a3;
    }
    .potter-input:focus {
        border-color: #4f98a3;
        box-shadow: 0 0 0 2px rgba(79, 152, 163, 0.25);
    }
    .potter-output {
        background-color: transparent;
        color: #cdccca;
        font-size: 14px;
        font-family: 'Inter', 'Segoe UI', sans-serif;
        padding: 8px 14px;
    }
    .potter-model-badge {
        color: #4f98a3;
        font-size: 11px;
        font-family: monospace;
        padding: 2px 8px;
        background-color: rgba(79, 152, 163, 0.12);
        border-radius: 4px;
    }
    .potter-copy-btn {
        background-color: rgba(255, 255, 255, 0.06);
        color: #797876;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 6px;
        padding: 4px 10px;
        font-size: 12px;
    }
    .potter-copy-btn:hover {
        background-color: rgba(255, 255, 255, 0.10);
        color: #cdccca;
    }
"#;

/// Application ID for the GTK4 app.
const APP_ID: &str = "net.marawan.potter";

/// Runs the GTK4 application. This function blocks until the app exits.
///
/// It starts the hotkey listener in the background and wires it to the
/// GTK main thread via `glib::MainContext::channel`.
pub async fn run_app(cfg: Config) -> Result<()> {
    let app = Application::builder().application_id(APP_ID).build();

    let cfg_clone = cfg.clone();
    app.connect_activate(move |app| {
        build_ui(app, &cfg_clone);
    });

    // Start hotkey listener and bridge events to GTK main context
    let hotkey_str = cfg.defaults.hotkey.clone();
    let mut hotkey_rx = crate::hotkey::start_listener(&hotkey_str)
        .expect("Failed to start hotkey listener");

    // Bridge the tokio channel to glib's main context
    let (gtk_tx, gtk_rx) = glib::MainContext::channel::<()>(glib::Priority::DEFAULT);

    tokio::spawn(async move {
        while hotkey_rx.recv().await.is_some() {
            let _ = gtk_tx.send(());
        }
    });

    // The gtk_rx receiver and window toggle logic is set up inside build_ui
    // via a shared window reference. For the scaffold this is documented as
    // the integration point — full wiring is in Phase 1 implementation.
    info!("GTK4 app starting — hotkey listener active");

    let exit_code = app.run_with_args::<String>(&[]);
    if exit_code != glib::ExitCode::SUCCESS {
        anyhow::bail!("GTK application exited with error");
    }
    Ok(())
}

/// Builds the overlay window and all its child widgets.
fn build_ui(app: &Application, cfg: &Config) {
    // Apply CSS
    let provider = CssProvider::new();
    provider.load_from_data(OVERLAY_CSS);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Main window — frameless, always on top
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Potter")
        .default_width(440)
        .default_height(520)
        .decorated(false)
        .resizable(false)
        .build();

    // TODO(phase1): Set window position to top-right using display geometry
    // window.set_gravity(gdk::Gravity::NorthEast);

    // Root vertical box
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    // Model badge (shows active provider)
    let model_badge = Label::new(Some(&format!("@{}", cfg.defaults.model)));
    model_badge.add_css_class("potter-model-badge");
    model_badge.set_halign(gtk4::Align::Start);

    // Prompt input
    let input = Entry::new();
    input.add_css_class("potter-input");
    input.set_placeholder_text(Some("Ask anything... (@gemini, @claude, @local)"));

    // Output area
    let output_view = TextView::new();
    output_view.add_css_class("potter-output");
    output_view.set_editable(false);
    output_view.set_wrap_mode(gtk4::WrapMode::Word);

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(300)
        .child(&output_view)
        .build();

    // Copy button
    let copy_btn = Button::with_label("Copy");
    copy_btn.add_css_class("potter-copy-btn");
    copy_btn.set_halign(gtk4::Align::End);

    let output_view_clone = output_view.clone();
    copy_btn.connect_clicked(move |_| {
        let buf = output_view_clone.buffer();
        let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
        if let Some(display) = gtk4::gdk::Display::default() {
            display.clipboard().set_text(&text);
        }
    });

    // Escape key closes the window
    let key_controller = gtk4::EventControllerKey::new();
    let window_clone = window.clone();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            window_clone.hide();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    // Wire up prompt submission
    // TODO(phase1): On Enter, parse @prefix, route to correct LLM provider,
    // stream tokens into output_view buffer.
    let _cfg_clone = cfg.clone();
    input.connect_activate(move |entry| {
        let prompt = entry.text().to_string();
        if prompt.trim().is_empty() {
            return;
        }
        // Placeholder until async LLM routing is wired:
        entry.set_text("");
        let _ = prompt; // Phase 1: dispatch to llm::route(prompt, cfg)
    });

    // Assemble layout
    vbox.append(&model_badge);
    vbox.append(&input);
    vbox.append(&scrolled);
    vbox.append(&copy_btn);
    window.set_child(Some(&vbox));

    window.present();
    input.grab_focus();
}
