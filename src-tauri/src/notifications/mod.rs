//! Notification service — in-memory store, event emitter, and Tauri commands.
//!
//! Phase 1 scope: service skeleton, specta-bound types, Tauri commands, and
//! wiring the existing watch desktop-notification path so each fired watch
//! notification also lands in the store. The frontend pane, OSC parser,
//! `roux notify` CLI, and focus-gated policy all arrive in later phases.

pub mod manager;
pub mod store;

pub use manager::NotificationManager;
#[allow(unused_imports)]
pub use manager::NOTIFICATION_EVENT;

use crate::state::AppState;
use roux_core::{Notification, NotificationRequest, NotificationSource};

#[tauri::command]
#[specta::specta]
pub async fn notifications_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Notification>, String> {
    Ok(state.notification_manager.list())
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_list_for_session(
    session_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Notification>, String> {
    Ok(state
        .notification_manager
        .list_for_session(session_id.as_deref()))
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_unread_count(
    session_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<u32, String> {
    // Semantics:
    //   session_id = Some(id) -> count for that session
    //   session_id = None & global = Some(true) -> count global-only
    //   session_id = None & global != Some(true) -> count everything
    let filter = match (session_id.as_deref(), global) {
        (Some(id), _) => Some(Some(id)),
        (None, Some(true)) => Some(None),
        (None, _) => None,
    };
    Ok(state.notification_manager.unread_count(filter) as u32)
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_mark_read(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    Ok(state.notification_manager.mark_read(&id, Some(&app)))
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_mark_all_read(
    session_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<u32, String> {
    let filter = match (session_id.as_deref(), global) {
        (Some(id), _) => Some(Some(id)),
        (None, Some(true)) => Some(None),
        (None, _) => None,
    };
    Ok(state
        .notification_manager
        .mark_all_read(filter, Some(&app)) as u32)
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_remove(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    Ok(state.notification_manager.remove(&id, Some(&app)))
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_clear(
    session_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<u32, String> {
    let filter = match (session_id.as_deref(), global) {
        (Some(id), _) => Some(Some(id)),
        (None, Some(true)) => Some(None),
        (None, _) => None,
    };
    Ok(state.notification_manager.clear(filter, Some(&app)) as u32)
}

/// Push a notification from the frontend. Primarily intended for testing,
/// demos, and the eventual `roux notify` CLI bridge; production notifications
/// originate in Rust (watches, hooks, tasks, OSC parser).
#[tauri::command]
#[specta::specta]
pub async fn notifications_push(
    request: NotificationRequest,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Notification, String> {
    Ok(state.notification_manager.push(request, Some(&app)))
}

/// Remove all notifications whose source matches the given source variant.
/// Only the variant is compared — inner fields (e.g. `watch_id`, `pane_id`)
/// are ignored — because the `DismissSource` action is "dismiss all of this
/// kind", not "dismiss all from exactly this id".
#[tauri::command]
#[specta::specta]
pub async fn notifications_dismiss_source(
    source: NotificationSource,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<u32, String> {
    Ok(state
        .notification_manager
        .remove_by_source_variant(&source, Some(&app)) as u32)
}
