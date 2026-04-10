use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, Manager};
use thiserror::Error;

use roux_core::{
    ActionKind, NotificationAction, NotificationLevel, NotificationRequest, NotificationSource,
};

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct StatusUpdate {
    status: String,
    cwd: String,
    claude_session_id: String,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    message: Option<String>,
}

#[derive(Debug, Error)]
pub enum StatusWatcherError {
    #[error("Could not determine home directory")]
    HomeDirUnavailable,
    #[error("Failed to create status dir: {source}")]
    CreateStatusDir {
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to create watcher: {source}")]
    CreateWatcher {
        #[source]
        source: notify::Error,
    },
    #[error("Failed to watch status dir: {source}")]
    WatchStatusDir {
        #[source]
        source: notify::Error,
    },
}

fn status_dir() -> Result<PathBuf, StatusWatcherError> {
    let home = dirs::home_dir().ok_or(StatusWatcherError::HomeDirUnavailable)?;
    let dir = home.join(".config").join("roux").join("status");
    fs::create_dir_all(&dir).map_err(|source| StatusWatcherError::CreateStatusDir { source })?;
    Ok(dir)
}

fn map_status(raw: &str) -> &str {
    match raw {
        "working" => "generating",
        "idle" => "idle",
        "attention" => "attention",
        "error" => "error",
        "disconnected" => "disconnected",
        _ => raw,
    }
}

pub fn start_watching(app: tauri::AppHandle) -> Result<(), StatusWatcherError> {
    let watch_dir = status_dir()?;

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())
        .map_err(|source| StatusWatcherError::CreateWatcher { source })?;

    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .map_err(|source| StatusWatcherError::WatchStatusDir { source })?;

    thread::spawn(move || {
        // Keep watcher alive for the lifetime of this thread
        let _watcher = watcher;

        // Debounce: collect changed paths, then process after a short quiet period
        let debounce_duration = Duration::from_millis(50);

        loop {
            // Block until the first event arrives
            let first = match rx.recv() {
                Ok(Ok(event)) => event,
                Ok(Err(_)) => continue,
                Err(_) => break, // channel closed
            };

            // Collect this event's paths and drain any more that arrive within the debounce window
            let mut changed_paths = HashSet::new();
            for path in first.paths {
                if matches!(first.kind, EventKind::Create(_) | EventKind::Modify(_))
                    && path.extension().and_then(|e| e.to_str()) == Some("json")
                {
                    changed_paths.insert(path);
                }
            }

            // Drain additional events within the debounce window
            while let Ok(result) = rx.recv_timeout(debounce_duration) {
                if let Ok(event) = result {
                    if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                        for path in event.paths {
                            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                                changed_paths.insert(path);
                            }
                        }
                    }
                }
            }

            // Process each unique path once
            for path in &changed_paths {
                let content = match fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let parsed: Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let raw_status = match parsed.get("status").and_then(|s| s.as_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };

                let cwd = parsed.get("cwd").and_then(|s| s.as_str()).unwrap_or("").to_string();

                let claude_sid = parsed
                    .get("claude_session_id")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();

                let tool_name = parsed
                    .get("tool_name")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                let tool_input = parsed.get("tool_input").cloned().filter(|v| !v.is_null());

                let message = parsed
                    .get("message")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                let mapped = map_status(&raw_status);

                let update = StatusUpdate {
                    status: mapped.to_string(),
                    cwd: cwd.clone(),
                    claude_session_id: claude_sid,
                    tool_name: tool_name.clone(),
                    tool_input: tool_input.clone(),
                    message: message.clone(),
                };

                let _ = app.emit("roux-status-update", &update);

                // For attention states, also push a notification so the
                // notifications pane and any OS fan-out get a first-class
                // entry. We resolve the session from the cwd on a background
                // task (session_handle is async) so the watcher thread stays
                // non-blocking.
                if mapped == "attention" {
                    let app_for_task = app.clone();
                    let cwd_for_task = cwd.clone();
                    let tool_name_for_task = tool_name.clone();
                    let tool_input_for_task = tool_input.clone();
                    let message_for_task = message.clone();
                    tauri::async_runtime::spawn(async move {
                        push_attention_notification(
                            &app_for_task,
                            &cwd_for_task,
                            tool_name_for_task,
                            tool_input_for_task,
                            message_for_task,
                        )
                        .await;
                    });
                }
            }
        }
    });

    Ok(())
}

async fn push_attention_notification(
    app: &tauri::AppHandle,
    cwd: &str,
    tool_name: Option<String>,
    tool_input: Option<Value>,
    message: Option<String>,
) {
    let state = app.state::<AppState>();

    // Match the cwd to a known session by worktree_path or repo_root — the
    // same match the frontend has been doing for status updates.
    let session_id = match state.session_handle.list().await {
        Ok(sessions) => sessions
            .into_iter()
            .find(|s| s.worktree_path == cwd || s.repo_root == cwd)
            .map(|s| s.id),
        Err(_) => None,
    };

    let title = tool_name
        .clone()
        .or_else(|| message.clone())
        .unwrap_or_else(|| "Permission requested".to_string());

    let body = match (&tool_name, &tool_input) {
        (Some(_), Some(input)) => {
            let serialized = serde_json::to_string(input).unwrap_or_default();
            if serialized.len() > 200 {
                Some(format!("{}…", &serialized[..200]))
            } else {
                Some(serialized)
            }
        }
        _ => message.clone(),
    };

    let mut actions: Vec<NotificationAction> = Vec::new();
    if let Some(ref sid) = session_id {
        actions.push(NotificationAction {
            id: "focus".into(),
            label: "Focus session".into(),
            kind: ActionKind::FocusSession { session_id: sid.clone() },
            primary: true,
        });
    }
    actions.push(NotificationAction {
        id: "dismiss".into(),
        label: "Dismiss".into(),
        kind: ActionKind::Dismiss,
        primary: actions.is_empty(),
    });

    state.notification_manager.push(
        NotificationRequest {
            level: NotificationLevel::Attention,
            source: NotificationSource::Hook {
                provider: "claude".to_string(),
            },
            title,
            subtitle: None,
            body,
            session_id,
            actions,
        },
        Some(app),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn maps_known_status_values() {
        assert_eq!(map_status("working"), "generating");
        assert_eq!(map_status("idle"), "idle");
    }

    #[test]
    fn status_watcher_error_display_keeps_existing_messages() {
        let error =
            StatusWatcherError::CreateStatusDir { source: io::Error::other("permission denied") };

        assert_eq!(error.to_string(), "Failed to create status dir: permission denied");
    }
}
