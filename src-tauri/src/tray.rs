//! System tray (macOS menu bar / Windows task bar) integration.
//!
//! Surface today: a tray icon whose menu lists active (non-archived)
//! sessions with their status, surfaces unread notifications in a
//! submenu, and offers Show/Quit actions. Clicking a session entry
//! brings the main window to front and emits `tray-focus-session` so
//! the frontend can route focus.
//!
//! The menu is rebuilt from scratch on every refresh — both sessions
//! and notifications are cheap to list and Tauri's menu API doesn't
//! support targeted updates. Refresh is driven by `roux-status-update`,
//! `notification-event`, and a low-frequency polling timer (see
//! `main.rs`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use image::{ImageBuffer, Rgba};
use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, Wry,
};
use tokio::sync::Notify;

use crate::state::AppState;
use roux_core::{Notification, NotificationLevel, Session, SessionStatus};

/// Source bitmap for the tray icon. Embedded so the binary is self-
/// contained (no runtime file lookup) and so we can decode it once and
/// composite an "attention" variant from the same pixels.
const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/32x32.png");

const TRAY_ID: &str = "roux-main-tray";
const MENU_ID_SHOW: &str = "tray::show";
const MENU_ID_QUIT: &str = "tray::quit";
const MENU_ID_MARK_ALL_READ: &str = "tray::notif::mark-all-read";
const MENU_ID_CLEAR_ALL: &str = "tray::notif::clear-all";
const SESSION_PREFIX: &str = "tray::session::";
const NOTIF_PREFIX: &str = "tray::notif::item::";

/// How many unread notifications to surface in the tray submenu before
/// collapsing the rest behind a "+ N more" entry. Keeps the menu
/// scannable when a flood of hooks/watches has piled up.
const MAX_NOTIFS_IN_TRAY: usize = 10;

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, &[], &[], 0)?;
    let icons = tray_icons();

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icons.normal.clone())
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Roux")
        .on_menu_event(handle_menu_event)
        .build(app)?;

    start_refresh_worker(app.clone());
    refresh();
    Ok(())
}

/// Single `Notify` shared by every `refresh()` caller and consumed by
/// the worker started in `setup()`. `notify_one` is idempotent when no
/// waiter is parked, so a burst of triggers (status events +
/// notification events + ticker) coalesces into at most one extra run
/// after the in-progress one finishes — no overlapping tasks, no
/// chance of an older snapshot's `set_menu` clobbering a newer one's.
static REFRESH_SIGNAL: OnceLock<Arc<Notify>> = OnceLock::new();

fn start_refresh_worker(app: AppHandle) {
    let notify = Arc::new(Notify::new());
    if REFRESH_SIGNAL.set(notify.clone()).is_err() {
        // Worker already running (e.g. `setup` called twice in tests).
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            notify.notified().await;
            do_refresh(&app).await;
        }
    });
}

/// Request a tray refresh. Cheap and lock-free: signals the worker and
/// returns. Safe to call from any context, including event listener
/// callbacks that fire on the main thread.
pub fn refresh() {
    if let Some(signal) = REFRESH_SIGNAL.get() {
        signal.notify_one();
    }
}

/// Rebuild the tray menu from current session + notification state.
/// Always runs on the worker task, so completions are serialized and
/// the latest call wins.
async fn do_refresh(app: &AppHandle) {
    let sessions = {
        let state = app.state::<AppState>();
        match state.session_handle.list().await {
            Ok(list) => list,
            Err(e) => {
                rlog!("tray: list_sessions failed: {e}");
                return;
            }
        }
    };
    let active: Vec<Session> = sessions.into_iter().filter(|s| !s.archived).collect();

    let (unread, total_unread) = {
        let state = app.state::<AppState>();
        let all = state.notification_manager.list();
        let total = all.iter().filter(|n| !n.read).count();
        let top: Vec<Notification> = all
            .into_iter()
            .filter(|n| !n.read)
            .take(MAX_NOTIFS_IN_TRAY)
            .collect();
        (top, total)
    };

    let needs_attention = active
        .iter()
        .any(|s| matches!(s.status, SessionStatus::Attention));

    let menu = match build_menu(app, &active, &unread, total_unread) {
        Ok(m) => m,
        Err(e) => {
            rlog!("tray: build_menu failed: {e}");
            return;
        }
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_menu(Some(menu)) {
            rlog!("tray: set_menu failed: {e}");
        }
        // macOS: surface the unread count next to the icon. No-op
        // on platforms where set_title isn't supported.
        let _ = tray.set_title(if total_unread > 0 {
            Some(format!("{}", total_unread))
        } else {
            None
        });

        // Swap the icon only when the attention state actually flips.
        // Avoids per-tick set_icon churn (cheap but visible flicker on
        // some platforms) when nothing changed.
        //
        // The cache is updated only on success — if `set_icon` or
        // `set_icon_as_template` fails (rare, but possible on platform
        // edge cases), leaving the cache stale would mean future
        // refreshes skip the retry and the tray gets stuck in the
        // wrong state.
        if ATTENTION_ICON_ACTIVE.load(Ordering::Relaxed) != needs_attention {
            let icons = tray_icons();
            // Attention dot is colored; template mode would strip the
            // color on macOS, so flip template off for the dotted icon
            // and back on for the normal one.
            let result = if needs_attention {
                tray.set_icon(Some(icons.attention.clone()))
                    .and_then(|_| tray.set_icon_as_template(false))
            } else {
                tray.set_icon(Some(icons.normal.clone()))
                    .and_then(|_| tray.set_icon_as_template(true))
            };
            match result {
                Ok(()) => {
                    ATTENTION_ICON_ACTIVE.store(needs_attention, Ordering::Relaxed);
                }
                Err(e) => {
                    rlog!("tray: icon swap failed (will retry next refresh): {e}");
                }
            }
        }
    }
}

/// Tracks whether the tray currently shows the attention-state icon, so
/// `refresh` can detect transitions and only call `set_icon` when the
/// state actually flips.
static ATTENTION_ICON_ACTIVE: AtomicBool = AtomicBool::new(false);

struct TrayIcons {
    normal: Image<'static>,
    attention: Image<'static>,
}

fn tray_icons() -> &'static TrayIcons {
    static ICONS: OnceLock<TrayIcons> = OnceLock::new();
    ICONS.get_or_init(|| {
        // include_bytes! guarantees the PNG was present at build time;
        // a decode failure here is a packaging bug, not a runtime
        // condition we can recover from.
        let base = image::load_from_memory(TRAY_ICON_PNG)
            .expect("tray: embedded 32x32.png failed to decode")
            .to_rgba8();
        let (w, h) = base.dimensions();

        let normal = Image::new_owned(base.clone().into_raw(), w, h);

        let mut with_dot = base;
        draw_attention_dot(&mut with_dot);
        let attention = Image::new_owned(with_dot.into_raw(), w, h);

        TrayIcons { normal, attention }
    })
}

/// Paint a filled red disc in the bottom-right corner of the icon as a
/// "needs attention" badge. Anti-aliased so it doesn't look pixelated
/// when macOS upscales the 32×32 source for retina displays.
fn draw_attention_dot(img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>) {
    let (w, h) = img.dimensions();
    // Dot covers ~⅜ of the icon, anchored 1px in from the corner.
    let radius = (w.min(h) as f32) * 0.32;
    let cx = w as f32 - radius - 1.0;
    let cy = h as f32 - radius - 1.0;
    let red = Rgba([0xE0, 0x2A, 0x2A, 0xFF]);

    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= radius - 0.5 {
                img.put_pixel(x, y, red);
            } else if dist < radius + 0.5 {
                // Single-pixel anti-aliased edge. `t` is how much of the
                // pixel sits inside the disc (0..1).
                let t = (radius + 0.5 - dist).clamp(0.0, 1.0);
                let bg = *img.get_pixel(x, y);
                img.put_pixel(x, y, blend_rgba(bg, red, t));
            }
        }
    }
}

fn blend_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    let lerp = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Rgba([lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2]), lerp(a[3], b[3])])
}

fn build_menu(
    app: &AppHandle,
    sessions: &[Session],
    unread: &[Notification],
    total_unread: usize,
) -> tauri::Result<Menu<Wry>> {
    let header_label = format_header(sessions.len(), total_unread);
    let header = MenuItemBuilder::with_id("tray::header", header_label)
        .enabled(false)
        .build(app)?;

    let mut builder = MenuBuilder::new(app).item(&header).separator();

    if total_unread > 0 {
        let submenu = build_notifications_submenu(app, unread, total_unread)?;
        builder = builder.item(&submenu).separator();
    }

    for s in sessions {
        let label = format!("{}  {} — {}", status_glyph(&s.status), s.name, s.status);
        let id = format!("{SESSION_PREFIX}{}", s.id);
        builder = builder.text(id, label);
    }

    if !sessions.is_empty() {
        builder = builder.separator();
    }

    builder
        .text(MENU_ID_SHOW, "Show Roux")
        .text(MENU_ID_QUIT, "Quit Roux")
        .build()
}

fn format_header(active_sessions: usize, unread: usize) -> String {
    let sessions_part = if active_sessions == 0 {
        "no active sessions".to_string()
    } else {
        format!(
            "{} active session{}",
            active_sessions,
            if active_sessions == 1 { "" } else { "s" }
        )
    };
    if unread > 0 {
        format!("Roux — {sessions_part} · {unread} unread")
    } else {
        format!("Roux — {sessions_part}")
    }
}

fn build_notifications_submenu(
    app: &AppHandle,
    unread: &[Notification],
    total_unread: usize,
) -> tauri::Result<tauri::menu::Submenu<Wry>> {
    let title = format!("Notifications ({total_unread})");
    let mut sub = SubmenuBuilder::new(app, title);

    for n in unread {
        let label = format_notification_label(n);
        let id = format!("{NOTIF_PREFIX}{}", n.id);
        sub = sub.text(id, label);
    }

    if total_unread > unread.len() {
        let extra = total_unread - unread.len();
        let more = MenuItemBuilder::with_id(
            "tray::notif::more",
            format!("+ {extra} more in app"),
        )
        .enabled(false)
        .build(app)?;
        sub = sub.item(&more);
    }

    sub = sub
        .separator()
        .text(MENU_ID_MARK_ALL_READ, "Mark all as read")
        .text(MENU_ID_CLEAR_ALL, "Clear all");

    sub.build()
}

fn format_notification_label(n: &Notification) -> String {
    let mut label = format!("{}  {}", level_glyph(n.level), n.title);
    // macOS truncates long menu items mid-glyph, so trim ourselves.
    // 80 chars covers most titles plus a short subtitle.
    if let Some(sub) = n.subtitle.as_deref().filter(|s| !s.is_empty()) {
        label.push_str(" — ");
        label.push_str(sub);
    }
    if label.chars().count() > 80 {
        let truncated: String = label.chars().take(77).collect();
        format!("{truncated}…")
    } else {
        label
    }
}

fn status_glyph(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Idle => "○",
        SessionStatus::Thinking => "◐",
        SessionStatus::Generating => "●",
        SessionStatus::Error => "✕",
        SessionStatus::Disconnected => "⊘",
        SessionStatus::Attention => "!",
    }
}

fn level_glyph(level: NotificationLevel) -> &'static str {
    match level {
        NotificationLevel::Info => "ℹ",
        NotificationLevel::Success => "✓",
        NotificationLevel::Attention => "!",
        NotificationLevel::Warning => "⚠",
        NotificationLevel::Error => "✕",
    }
}

/// Tray menu callbacks fire on the macOS main thread, inside tao's
/// `extern "C" fn send_event`. A panic anywhere downstream of this fn
/// can't unwind across the Cocoa FFI boundary — Rust force-aborts. So
/// we keep this fn as a thin classifier: own the id as a `String`,
/// hand the actual work to a tokio task, and wrap the dispatcher body
/// in `catch_unwind` so even classification errors can't crash us.
fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let app = app.clone();
    let id = event.id().as_ref().to_string();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch_menu_event(app, id);
    }));
    if let Err(e) = result {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        rlog!("tray: handle_menu_event panicked: {msg}");
    }
}

fn dispatch_menu_event(app: AppHandle, id: String) {
    if id == MENU_ID_SHOW {
        tauri::async_runtime::spawn(async move {
            show_main_window(&app);
        });
    } else if id == MENU_ID_QUIT {
        // Match the existing quit path: emit `quit-requested` and let the
        // frontend confirm + call back into the app to actually exit.
        tauri::async_runtime::spawn(async move {
            let _ = app.emit("quit-requested", ());
        });
    } else if id == MENU_ID_MARK_ALL_READ {
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            state.notification_manager.mark_all_read(None, Some(&app));
        });
    } else if id == MENU_ID_CLEAR_ALL {
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            state.notification_manager.clear(None, Some(&app));
        });
    } else if let Some(notif_id) = id.strip_prefix(NOTIF_PREFIX) {
        let notif_id = notif_id.to_string();
        tauri::async_runtime::spawn(async move {
            handle_notification_click(&app, &notif_id);
        });
    } else if let Some(session_id) = id.strip_prefix(SESSION_PREFIX) {
        let session_id = session_id.to_string();
        tauri::async_runtime::spawn(async move {
            show_main_window(&app);
            let _ = app.emit("tray-focus-session", session_id);
        });
    }
}

fn handle_notification_click(app: &AppHandle, notif_id: &str) {
    // Resolve the target session before mutating state — the
    // `mark_read` event will trigger a tray refresh that drops the
    // entry, which is fine, but we still need the session id to focus.
    let target_session = {
        let state = app.state::<AppState>();
        state
            .notification_manager
            .list()
            .into_iter()
            .find(|n| n.id == notif_id)
            .and_then(|n| n.session_id)
    };

    {
        let state = app.state::<AppState>();
        state.notification_manager.mark_read(notif_id, Some(app));
    }

    show_main_window(app);
    if let Some(sid) = target_session {
        let _ = app.emit("tray-focus-session", sid);
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
