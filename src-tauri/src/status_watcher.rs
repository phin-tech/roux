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
    /// The provider-internal session id (Claude's `session_id`, Codex's
    /// equivalent, etc.). `None` when the hook didn't carry one. Previously
    /// named `claudeSessionId` and hard-populated; renamed so non-Claude
    /// providers aren't forced to masquerade as Claude.
    provider_session_id: Option<String>,
    /// Provider that emitted the hook (`"claude"`, `"codex"`, …). Present when
    /// the hook bridge recognized the source; empty string for legacy payloads.
    provider: String,
    /// Roux session id captured from `ROUX_SESSION_ID` at hook time.
    /// Absent for agents launched outside a Roux-managed PTY.
    roux_session_id: Option<String>,
    /// Roux pane id captured from `ROUX_PANE_ID` at hook time. Tier-1 routing
    /// uses this to update the exact pane's runtime agent state without cwd
    /// heuristics. Absent for legacy / external installs (tier 2 fallback).
    roux_pane_id: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    message: Option<String>,
}

#[derive(Debug, Error)]
pub enum StatusWatcherError {
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
    let dir = crate::paths::roux_config_dir().join("status");
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

/// Pure parse of a hook payload JSON blob into a StatusUpdate. Returns `None`
/// for payloads with no `status` field — the watcher then drops the event.
/// Extracted so tier-1 routing fields (`roux_session_id`, `roux_pane_id`,
/// `provider`) can be unit-tested without the file watcher / Tauri emitter.
fn parse_status_payload(parsed: &Value) -> Option<StatusUpdate> {
    let raw_status = parsed.get("status").and_then(|s| s.as_str())?.to_string();

    let cwd = parsed.get("cwd").and_then(|s| s.as_str()).unwrap_or("").to_string();

    // Prefer the new provider-agnostic key; fall back to `claude_session_id`
    // for older roux-cli hook shims that haven't been reinstalled yet.
    let provider_session_id = parsed
        .get("provider_session_id")
        .or_else(|| parsed.get("claude_session_id"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let provider = parsed
        .get("provider")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    let roux_session_id = parsed
        .get("roux_session_id")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let roux_pane_id = parsed
        .get("roux_pane_id")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

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

    Some(StatusUpdate {
        status: map_status(&raw_status).to_string(),
        cwd,
        provider_session_id,
        provider,
        roux_session_id,
        roux_pane_id,
        tool_name,
        tool_input,
        message,
    })
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

                let update = match parse_status_payload(&parsed) {
                    Some(u) => u,
                    None => continue,
                };

                let cwd = update.cwd.clone();
                let mapped = update.status.clone();
                let tool_name = update.tool_name.clone();
                let tool_input = update.tool_input.clone();
                let message = update.message.clone();

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
    use serde_json::json;
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

    #[test]
    fn parse_payload_extracts_tier1_fields() {
        let payload = json!({
            "status": "working",
            "cwd": "/repo",
            "provider_session_id": "claude-abc",
            "provider": "claude",
            "roux_session_id": "sess-1",
            "roux_pane_id": "pane-1",
        });

        let update = parse_status_payload(&payload).expect("parse ok");
        assert_eq!(update.status, "generating");
        assert_eq!(update.cwd, "/repo");
        assert_eq!(update.provider_session_id.as_deref(), Some("claude-abc"));
        assert_eq!(update.provider, "claude");
        assert_eq!(update.roux_session_id.as_deref(), Some("sess-1"));
        assert_eq!(update.roux_pane_id.as_deref(), Some("pane-1"));
    }

    #[test]
    fn parse_payload_accepts_legacy_claude_session_id_key() {
        // Backward compat: a roux-cli shim from before the rename still
        // writes `claude_session_id`. The watcher reads either key so a
        // half-upgraded install keeps routing correctly.
        let payload = json!({
            "status": "idle",
            "cwd": "/repo",
            "claude_session_id": "claude-legacy",
            "provider": "claude",
        });

        let update = parse_status_payload(&payload).expect("parse ok");
        assert_eq!(update.provider_session_id.as_deref(), Some("claude-legacy"));
    }

    #[test]
    fn parse_payload_prefers_provider_session_id_over_legacy_key() {
        // If both keys are present (e.g. during a rolling upgrade where the
        // shim writes both), the canonical `provider_session_id` wins.
        let payload = json!({
            "status": "idle",
            "cwd": "/repo",
            "claude_session_id": "stale-legacy",
            "provider_session_id": "new-canonical",
        });

        let update = parse_status_payload(&payload).expect("parse ok");
        assert_eq!(update.provider_session_id.as_deref(), Some("new-canonical"));
    }

    #[test]
    fn parse_payload_legacy_missing_tier1_fields_returns_none_for_routing() {
        // Legacy hook install: no roux_* fields. Still produces a valid
        // StatusUpdate so notification fan-out keeps working, but tier-1
        // pane routing is disabled because roux_pane_id is None.
        let payload = json!({
            "status": "idle",
            "cwd": "/repo",
            "claude_session_id": "claude-legacy",
        });

        let update = parse_status_payload(&payload).expect("parse ok");
        assert_eq!(update.status, "idle");
        assert_eq!(update.provider, "");
        assert_eq!(update.roux_session_id, None);
        assert_eq!(update.roux_pane_id, None);
    }

    #[test]
    fn parse_payload_empty_tier1_strings_treated_as_none() {
        // The hook writes empty strings when env vars are unset on some
        // shells; we want those treated as "not present" so routing code
        // doesn't try to match on "".
        let payload = json!({
            "status": "idle",
            "cwd": "/repo",
            "provider_session_id": "",
            "roux_session_id": "",
            "roux_pane_id": "",
        });

        let update = parse_status_payload(&payload).expect("parse ok");
        assert_eq!(update.provider_session_id, None);
        assert_eq!(update.roux_session_id, None);
        assert_eq!(update.roux_pane_id, None);
    }

    #[test]
    fn parse_payload_drops_payload_without_status() {
        let payload = json!({
            "cwd": "/repo",
            "provider_session_id": "x",
        });
        assert!(parse_status_payload(&payload).is_none());
    }
}
