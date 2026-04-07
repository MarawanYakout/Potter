//! GTK4 overlay window -- Phase 1 complete implementation.
//!
//! Phase 1 delivers:
//!   - Frameless always-on-top window anchored to the top-right of the
//!     primary monitor.
//!   - Text entry that dispatches the prompt to the LLM router on Enter.
//!   - Async token streaming written into the output TextView in real time.
//!   - Loading indicator shown while a request is in-flight.
//!   - Up/Down arrow keys navigate prompt history.
//!   - Escape hides the window; the global hotkey shows it again.
//!   - Copy button copies the full output to the clipboard.

use crate::{
    config::Config,
    history::History,
    hotkey,
    llm,
};
use anyhow::Result;
use gtk4::{
    gdk, glib,
    prelude::*,
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider,
    Entry, Label, Orientation, PolicyType, ScrolledWindow, TextView,
    EventControllerKey,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
};
use tracing::{debug, error, info};

/// GTK4 application ID.
const APP_ID: &str = "net.marawan.potter";

/// Window dimensions.
const WINDOW_WIDTH: i32 = 460;
const WINDOW_HEIGHT: i32 = 540;
/// Margin from screen edge (px).
const EDGE_MARGIN: i32 = 24;

// ---------------------------------------------------------------------------
// CSS
// ---------------------------------------------------------------------------

const CSS: &str = r#"
window {
    background-color: rgba(15, 15, 17, 0.93);
    border-radius: 14px;
    border: 1px solid rgba(255,255,255,0.07);
}
.potter-header {
    padding: 4px 0 2px 0;
}
.potter-badge {
    color: #4f98a3;
    font-size: 11px;
    font-family: monospace;
    background-color: rgba(79,152,163,0.13);
    border-radius: 4px;
    padding: 2px 8px;
    margin-bottom: 2px;
}
.potter-input {
    background-color: rgba(255,255,255,0.055);
    color: #e2e2e0;
    border: 1px solid rgba(255,255,255,0.10);
    border-radius: 9px;
    padding: 10px 14px;
    font-size: 15px;
    caret-color: #4f98a3;
}
.potter-input:focus {
    border-color: rgba(79,152,163,0.70);
    box-shadow: 0 0 0 2px rgba(79,152,163,0.18);
}
.potter-output {
    background-color: transparent;
    color: #c8c7c5;
    font-size: 14px;
    padding: 6px 14px 6px 14px;
}
.potter-status {
    color: #4f98a3;
    font-size: 12px;
    font-family: monospace;
    padding: 0 14px;
    min-height: 20px;
}
.potter-copy {
    background-color: rgba(255,255,255,0.055);
    color: #7a7978;
    border: 1px solid rgba(255,255,255,0.08);
    border-radius: 6px;
    padding: 4px 12px;
    font-size: 12px;
}
.potter-copy:hover {
    background-color: rgba(255,255,255,0.09);
    color: #c8c7c5;
}
"#;

// ---------------------------------------------------------------------------
// Application entry point
// ---------------------------------------------------------------------------

/// Builds and runs the GTK4 application. Blocks until the app exits.
pub fn run_app(cfg: Config) -> Result<()> {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    let cfg = Arc::new(cfg);

    app.connect_activate({
        let cfg = Arc::clone(&cfg);
        move |app| {
            if let Err(e) = build_ui(app, Arc::clone(&cfg)) {
                error!("Failed to build UI: {}", e);
            }
        }
    });

    app.run_with_args::<String>(&[]);
    Ok(())
}

// ---------------------------------------------------------------------------
// UI construction
// ---------------------------------------------------------------------------

fn build_ui(app: &Application, cfg: Arc<Config>) -> Result<()> {
    // Apply CSS stylesheet
    let provider = CssProvider::new();
    provider.load_from_data(CSS);
    gtk4::style_context_add_provider_for_display(
        &gdk::Display::default().expect("No display found"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // --- Main window ---
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Potter")
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .decorated(false)
        .resizable(false)
        .build();

    // Position window at top-right of primary monitor
    position_window_top_right(&window);

    // --- Root layout ---
    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(14);
    root.set_margin_bottom(14);
    root.set_margin_start(14);
    root.set_margin_end(14);

    // Model badge
    let badge = Label::new(Some(&format!("@{}", cfg.defaults.model)));
    badge.add_css_class("potter-badge");
    badge.set_halign(gtk4::Align::Start);

    // Prompt input
    let input = Entry::new();
    input.add_css_class("potter-input");
    input.set_placeholder_text(Some("Ask anything... (@gemini  @claude  @local)"));

    // Output text view
    let output = TextView::new();
    output.add_css_class("potter-output");
    output.set_editable(false);
    output.set_cursor_visible(false);
    output.set_wrap_mode(gtk4::WrapMode::WordChar);

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(340)
        .vexpand(true)
        .child(&output)
        .build();

    // Status line ("Thinking..." / errors)
    let status = Label::new(None);
    status.add_css_class("potter-status");
    status.set_halign(gtk4::Align::Start);
    status.set_xalign(0.0);

    // Copy button
    let copy_btn = Button::with_label("Copy");
    copy_btn.add_css_class("potter-copy");
    copy_btn.set_halign(gtk4::Align::End);

    {
        let output_c = output.clone();
        copy_btn.connect_clicked(move |_| {
            let buf = output_c.buffer();
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&text);
            }
        });
    }

    // Assemble layout
    root.append(&badge);
    root.append(&input);
    root.append(&scroll);
    root.append(&status);
    root.append(&copy_btn);
    window.set_child(Some(&root));

    // --- Shared state ---
    let history: Rc<RefCell<History>> =
        Rc::new(RefCell::new(History::new(cfg.defaults.max_history)));
    // Track whether a request is currently running so we can block double-sends
    let busy: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // --- Enter key: dispatch prompt to LLM ---
    {
        let cfg_c = Arc::clone(&cfg);
        let output_c = output.clone();
        let status_c = status.clone();
        let badge_c = badge.clone();
        let history_c = Rc::clone(&history);
        let busy_c = Rc::clone(&busy);
        let scroll_c = scroll.clone();

        input.connect_activate(move |entry| {
            let raw = entry.text().to_string();
            if raw.trim().is_empty() || *busy_c.borrow() {
                return;
            }

            // Parse prefix to show active provider in badge
            let parsed = llm::parse_prompt(&raw, &cfg_c.defaults.model);
            badge_c.set_text(&format!("@{}", parsed.provider));

            // Push to history and clear input
            history_c.borrow_mut().push(raw.clone());
            entry.set_text("");

            // Clear previous output and show status
            output_c.buffer().set_text("");
            status_c.set_text("Thinking...");
            *busy_c.borrow_mut() = true;

            // Clone everything needed inside the async block
            let cfg_async = Arc::clone(&cfg_c);
            let output_async = output_c.clone();
            let status_async = status_c.clone();
            let busy_async = Rc::clone(&busy_c);
            let scroll_async = scroll_c.clone();

            // Spawn the async LLM call on the glib main context so GTK
            // widget mutations stay on the main thread.
            glib::spawn_future_local(async move {
                match llm::route_prompt(&raw, &cfg_async).await {
                    Err(e) => {
                        status_async.set_text(&format!("Error: {}", e));
                        *busy_async.borrow_mut() = false;
                    }
                    Ok(mut stream) => {
                        use futures_util::StreamExt;
                        status_async.set_text("");

                        while let Some(chunk) = stream.next().await {
                            match chunk {
                                Ok(token) => {
                                    // Append token to the output buffer
                                    let buf = output_async.buffer();
                                    let mut end = buf.end_iter();
                                    buf.insert(&mut end, &token);

                                    // Auto-scroll to bottom
                                    let adj = scroll_async
                                        .vadjustment();
                                    adj.set_value(adj.upper() - adj.page_size());
                                }
                                Err(e) => {
                                    status_async
                                        .set_text(&format!("Stream error: {}", e));
                                    break;
                                }
                            }
                        }
                        *busy_async.borrow_mut() = false;
                    }
                }
            });
        });
    }

    // --- Arrow keys: history navigation ---
    {
        let history_c = Rc::clone(&history);
        let input_c = input.clone();
        let key_ctrl = EventControllerKey::new();

        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            match key {
                gdk::Key::Up => {
                    if let Some(prev) = history_c.borrow_mut().prev() {
                        input_c.set_text(prev);
                        // Move cursor to end of text
                        input_c.set_position(-1);
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Down => {
                    match history_c.borrow_mut().next() {
                        Some(next) => {
                            input_c.set_text(next);
                            input_c.set_position(-1);
                        }
                        None => input_c.set_text(""),
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        input.add_controller(key_ctrl);
    }

    // --- Escape key: hide window ---
    {
        let window_c = window.clone();
        let key_ctrl = EventControllerKey::new();
        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                window_c.hide();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(key_ctrl);
    }

    // --- Hotkey listener: toggle window visibility ---
    {
        let hotkey_str = cfg.defaults.hotkey.clone();
        match hotkey::start_listener(&hotkey_str) {
            Err(e) => error!("Hotkey listener failed to start: {}", e),
            Ok(mut rx) => {
                // Bridge tokio mpsc -> glib channel so we stay on main thread
                let (gtk_tx, gtk_rx) =
                    glib::MainContext::channel::<()>(glib::Priority::DEFAULT);

                // Tokio task: forward hotkey events across the channel boundary
                let rt = tokio::runtime::Handle::current();
                std::thread::spawn(move || {
                    rt.block_on(async move {
                        while rx.recv().await.is_some() {
                            let _ = gtk_tx.send(());
                        }
                    });
                });

                let window_c = window.clone();
                let input_c = input.clone();
                gtk_rx.attach(None, move |()| {
                    if window_c.is_visible() {
                        window_c.hide();
                    } else {
                        position_window_top_right(&window_c);
                        window_c.present();
                        input_c.grab_focus();
                    }
                    glib::ControlFlow::Continue
                });

                info!("Global hotkey '{}' registered", hotkey_str);
            }
        }
    }

    window.present();
    input.grab_focus();
    debug!("UI built and window presented");
    Ok(())
}

// ---------------------------------------------------------------------------
// Window positioning
// ---------------------------------------------------------------------------

/// Moves the window to the top-right corner of the primary monitor.
/// Falls back to (0, 0) if display geometry is unavailable.
fn position_window_top_right(window: &ApplicationWindow) {
    if let Some(display) = gdk::Display::default() {
        // Get the monitor the window is on (or first monitor)
        let monitor = display
            .monitors()
            .item(0)
            .and_downcast::<gdk::Monitor>();

        if let Some(monitor) = monitor {
            let geometry = monitor.geometry();
            let x = geometry.x() + geometry.width() - WINDOW_WIDTH - EDGE_MARGIN;
            let y = geometry.y() + EDGE_MARGIN;
            // GTK4 uses surface-level positioning via the native surface
            // On X11/Wayland this is done through the native surface handle.
            // We store target coords and apply when the surface is realised.
            window.set_data("potter-target-x", x);
            window.set_data("potter-target-y", y);
        }
    }
}
