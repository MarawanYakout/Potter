//! GTK4 overlay window -- Phase 1 complete implementation.
//!
//! Phase 1 delivers:
//!   - Frameless always-on-top window anchored to the top-right of the
//!     primary monitor (X11; Wayland compositors control placement).
//!   - Text entry that dispatches the prompt to the LLM router on Enter.
//!   - Async token streaming written into the output TextView in real time.
//!   - Loading indicator shown while a request is in-flight.
//!   - Up/Down arrow keys navigate prompt history (Up=older, Down=newer).
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

const APP_ID: &str = "net.marawan.potter";
const WINDOW_WIDTH: i32 = 460;
const WINDOW_HEIGHT: i32 = 540;
const EDGE_MARGIN: i32 = 24;

const CSS: &str = r#"
window {
    background-color: rgba(15, 15, 17, 0.93);
    border-radius: 14px;
    border: 1px solid rgba(255,255,255,0.07);
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
    padding: 6px 14px;
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
///
/// A dedicated multi-thread Tokio runtime is created here and passed into
/// the UI layer. All async dispatches use rt.spawn() directly -- there are
/// no Handle::current() calls on the glib main thread (which has no Tokio
/// runtime attached to it).
pub fn run_app(cfg: Config) -> Result<()> {
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    let cfg = Arc::new(cfg);

    app.connect_activate({
        let cfg = Arc::clone(&cfg);
        let rt = Arc::clone(&rt);
        move |app| {
            if let Err(e) = build_ui(app, Arc::clone(&cfg), Arc::clone(&rt)) {
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

fn build_ui(
    app: &Application,
    cfg: Arc<Config>,
    rt: Arc<tokio::runtime::Runtime>,
) -> Result<()> {
    let provider = CssProvider::new();
    provider.load_from_data(CSS);
    gtk4::style_context_add_provider_for_display(
        &gdk::Display::default().expect("No display found"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Potter")
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .decorated(false)
        .resizable(false)
        .build();

    // Position the window once the native surface is realised.
    // On X11 we attempt to move via the GDK surface.
    // On Wayland the compositor controls placement -- this is a no-op there.
    {
        let window_c = window.clone();
        window.connect_realize(move |_| {
            position_window_top_right(&window_c);
        });
    }

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(14);
    root.set_margin_bottom(14);
    root.set_margin_start(14);
    root.set_margin_end(14);

    let badge = Label::new(Some(&format!("@{}", cfg.defaults.model)));
    badge.add_css_class("potter-badge");
    badge.set_halign(gtk4::Align::Start);

    let input = Entry::new();
    input.add_css_class("potter-input");
    input.set_placeholder_text(Some("Ask anything... (@gemini  @claude  @local)"));

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

    let status = Label::new(None);
    status.add_css_class("potter-status");
    status.set_halign(gtk4::Align::Start);
    status.set_xalign(0.0);

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

    root.append(&badge);
    root.append(&input);
    root.append(&scroll);
    root.append(&status);
    root.append(&copy_btn);
    window.set_child(Some(&root));

    let history: Rc<RefCell<History>> =
        Rc::new(RefCell::new(History::new(cfg.defaults.max_history)));
    let busy: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // --- Enter: dispatch prompt to LLM ---
    {
        let cfg_c = Arc::clone(&cfg);
        let rt_c = Arc::clone(&rt);
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

            let parsed = llm::parse_prompt(&raw, &cfg_c.defaults.model);
            badge_c.set_text(&format!("@{}", parsed.provider));
            history_c.borrow_mut().push(raw.clone());
            entry.set_text("");
            output_c.buffer().set_text("");
            status_c.set_text("Thinking...");
            *busy_c.borrow_mut() = true;

            let cfg_async = Arc::clone(&cfg_c);
            let rt_async = Arc::clone(&rt_c);
            let output_async = output_c.clone();
            let status_async = status_c.clone();
            let busy_async = Rc::clone(&busy_c);
            let scroll_async = scroll_c.clone();

            // spawn_future_local runs on the glib main thread -- all GTK
            // widget mutations inside here are safe. Heavy async work is
            // offloaded to the Tokio runtime and results returned via
            // tokio::sync channels.
            glib::spawn_future_local(async move {
                // Step 1: obtain the token stream on the Tokio runtime.
                let (stream_tx, stream_rx) = tokio::sync::oneshot::channel();
                rt_async.spawn(async move {
                    let result = llm::route_prompt(&raw, &cfg_async).await;
                    let _ = stream_tx.send(result);
                });

                let stream_result = match stream_rx.await {
                    Ok(r) => r,
                    Err(_) => {
                        status_async.set_text("Error: LLM task dropped unexpectedly");
                        *busy_async.borrow_mut() = false;
                        return;
                    }
                };

                match stream_result {
                    Err(e) => {
                        status_async.set_text(&format!("Error: {e}"));
                        *busy_async.borrow_mut() = false;
                    }
                    Ok(mut stream) => {
                        use futures_util::StreamExt;
                        status_async.set_text("");

                        // Step 2: drive the stream on Tokio, send tokens back.
                        let (tok_tx, mut tok_rx) =
                            tokio::sync::mpsc::channel::<Result<String, String>>(32);
                        rt_async.spawn(async move {
                            while let Some(chunk) = stream.next().await {
                                let msg = chunk.map_err(|e| e.to_string());
                                if tok_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        });

                        // Step 3: drain on glib main thread -- safe to touch GTK.
                        while let Some(msg) = tok_rx.recv().await {
                            match msg {
                                Ok(token) => {
                                    let buf = output_async.buffer();
                                    let mut end = buf.end_iter();
                                    buf.insert(&mut end, &token);
                                    let adj = scroll_async.vadjustment();
                                    adj.set_value(adj.upper() - adj.page_size());
                                }
                                Err(e) => {
                                    status_async.set_text(&format!("Stream error: {e}"));
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

    // --- Escape: hide window ---
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

    // --- Global hotkey: toggle visibility ---
    {
        let hotkey_str = cfg.defaults.hotkey.clone();
        match hotkey::start_listener(&hotkey_str) {
            Err(e) => error!("Hotkey listener failed to start: {}", e),
            Ok(mut rx) => {
                let (gtk_tx, gtk_rx) =
                    glib::MainContext::channel::<()>(glib::Priority::DEFAULT);

                // Forward events using the explicit Tokio runtime handle.
                // No Handle::current() call is needed or used.
                rt.spawn(async move {
                    while rx.recv().await.is_some() {
                        let _ = gtk_tx.send(());
                    }
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

/// Attempts to position the window at the top-right corner of the primary
/// monitor. Must be called from connect_realize so the native surface exists.
///
/// On X11 the position is requested via the GDK native surface.
/// On Wayland the compositor controls window placement; this is a no-op.
fn position_window_top_right(window: &ApplicationWindow) {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let Some(monitor) = display
        .monitors()
        .item(0)
        .and_downcast::<gdk::Monitor>()
    else {
        return;
    };

    let geo = monitor.geometry();
    let target_x = geo.x() + geo.width() - WINDOW_WIDTH - EDGE_MARGIN;
    let target_y = geo.y() + EDGE_MARGIN;

    if let Some(surface) = window.surface() {
        // On X11 we can move the window via the native surface.
        // The gdk4-x11 crate is not a hard dependency; on Wayland or other
        // backends the downcast fails silently and we fall through.
        #[cfg(feature = "x11")]
        {
            use gdk4_x11::prelude::X11SurfaceExt;
            if let Ok(x11) = surface.downcast::<gdk4_x11::X11Surface>() {
                x11.move_(target_x, target_y);
                return;
            }
        }
        let _ = (surface, target_x, target_y);
    }
}
