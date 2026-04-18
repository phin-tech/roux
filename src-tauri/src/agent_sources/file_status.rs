//! File-backed hook-status source.
//!
//! Watches `~/.config/roux/status/` for JSON files written by the
//! `roux-cli hook <status>` command that Claude's hook framework runs.
//! Each change is parsed into an `AgentInput` and pushed onto the
//! registry channel. The Tauri frontend continues to receive
//! `roux-status-update` events for every change so its existing
//! per-pane routing (see `src/lib/panes/statusRouting.ts`) is
//! unaffected — only the attention-notification lifecycle moves into
//! the FSM registry.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;
use tauri::Emitter;
use thiserror::Error;

use roux_core::agent_fsm::{AgentEvent, AgentIdentity, MappedStatus};

use crate::agent_registry::{AgentInput, EventContext, RegistryMessage};

/// Event payload emitted to the frontend for every hook status change.
/// Shape is `camelCase` so TypeScript code can destructure directly.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StatusUpdate {
    pub status: String,
    pub cwd: String,
    pub provider_session_id: Option<String>,
    pub provider: String,
    pub roux_session_id: Option<String>,
    pub roux_pane_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub message: Option<String>,
}

#[derive(Debug, Error)]
pub enum FileStatusSourceError {
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

pub fn status_dir() -> Result<PathBuf, FileStatusSourceError> {
    let dir = crate::paths::roux_config_dir().join("status");
    fs::create_dir_all(&dir).map_err(|source| FileStatusSourceError::CreateStatusDir { source })?;
    Ok(dir)
}

/// Pure parser: hook-payload JSON → `StatusUpdate`. Returns `None` for
/// payloads with no `status` field (the watcher drops the event).
pub fn parse_status_payload(parsed: &Value) -> Option<StatusUpdate> {
    let raw_status = parsed.get("status").and_then(|s| s.as_str())?.to_string();
    let cwd = parsed.get("cwd").and_then(|s| s.as_str()).unwrap_or("").to_string();

    // Prefer the provider-agnostic key, but fall back to the legacy
    // `claude_session_id` so an older roux-cli shim paired with a newer
    // desktop binary keeps routing correctly. `cli.rs`'s hook writer
    // comment documents the contract.
    let provider_session_id = parsed
        .get("provider_session_id")
        .or_else(|| parsed.get("claude_session_id"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let provider = parsed.get("provider").and_then(|s| s.as_str()).unwrap_or("").to_string();

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

/// Map a `StatusUpdate`'s mapped status string to the FSM's typed
/// `MappedStatus`, or `None` for statuses the FSM doesn't model
/// (`"error"`, `"disconnected"`, unknown). Non-FSM statuses still flow
/// through the `roux-status-update` Tauri event for the frontend; they
/// just don't drive attention-notification lifecycle.
pub fn status_update_to_mapped(update: &StatusUpdate) -> Option<MappedStatus> {
    match update.status.as_str() {
        "generating" => Some(MappedStatus::Generating),
        "idle" => Some(MappedStatus::Idle),
        "attention" => Some(MappedStatus::Attention),
        _ => None,
    }
}

/// Build the `AgentIdentity` used to route this update to an FSM. Uses
/// the same pane > session > cwd precedence as the existing
/// notification dedup-key construction.
pub fn payload_to_identity(update: &StatusUpdate) -> AgentIdentity {
    AgentIdentity {
        pane_id: update.roux_pane_id.clone(),
        session_id: update.roux_session_id.clone(),
        cwd: if update.cwd.is_empty() {
            None
        } else {
            Some(PathBuf::from(&update.cwd))
        },
    }
}

pub fn payload_to_event_context(update: &StatusUpdate) -> EventContext {
    // `StatusUpdate::provider_session_id` is intentionally *not* copied:
    // it rides the `roux-status-update` Tauri event for the frontend,
    // but nothing downstream of the FSM registry reads it. See the
    // notes on `EventContext` in `agent_registry.rs`.
    EventContext {
        cwd: update.cwd.clone(),
        provider: update.provider.clone(),
        roux_session_id: update.roux_session_id.clone(),
        roux_pane_id: update.roux_pane_id.clone(),
        tool_name: update.tool_name.clone(),
        tool_input: update.tool_input.clone(),
        message: update.message.clone(),
    }
}

/// Start the file watcher. Spawns a dedicated thread that debounces
/// notify events and translates each change into an `AgentInput` + a
/// `roux-status-update` emission. `tx` is the registry's input sender.
pub fn start_watching(
    app: tauri::AppHandle,
    tx: mpsc::Sender<RegistryMessage>,
) -> Result<(), FileStatusSourceError> {
    let watch_dir = status_dir()?;
    let (notify_tx, notify_rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(notify_tx, notify::Config::default())
        .map_err(|source| FileStatusSourceError::CreateWatcher { source })?;
    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .map_err(|source| FileStatusSourceError::WatchStatusDir { source })?;

    thread::spawn(move || {
        let _watcher = watcher; // keep alive
        let debounce = Duration::from_millis(50);

        loop {
            let first = match notify_rx.recv() {
                Ok(Ok(event)) => event,
                Ok(Err(_)) => continue,
                Err(_) => break,
            };

            let mut changed_paths: HashSet<PathBuf> = HashSet::new();
            let mut removed_paths: HashSet<PathBuf> = HashSet::new();
            collect_paths(&first, &mut changed_paths, &mut removed_paths);

            while let Ok(result) = notify_rx.recv_timeout(debounce) {
                if let Ok(event) = result {
                    collect_paths(&event, &mut changed_paths, &mut removed_paths);
                }
            }

            for path in &changed_paths {
                if let Err(e) = process_path_change(&app, &tx, path) {
                    crate::rlog!("file_status: process_path_change error {}: {}", path.display(), e);
                }
            }

            for path in &removed_paths {
                if changed_paths.contains(path) {
                    // A rapid write-then-delete in the same debounce window:
                    // the change already routed, and we can't know the
                    // identity from the removal alone. Drop.
                    continue;
                }
                crate::rlog!("file_status: status file removed {}", path.display());
                // We don't currently attempt to recover the pane identity
                // from the filename alone — the hook file is named after
                // the provider session id, which isn't our routing key.
                // Agents that crash emit no further hook updates, so the
                // attention notification will linger until the session's
                // own lifecycle source fires `SessionEnded`.
            }
        }
    });

    Ok(())
}

fn collect_paths(
    event: &Event,
    changed: &mut HashSet<PathBuf>,
    removed: &mut HashSet<PathBuf>,
) {
    let is_json = |path: &Path| path.extension().and_then(|e| e.to_str()) == Some("json");
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            for path in &event.paths {
                if is_json(path) {
                    changed.insert(path.clone());
                }
            }
        }
        EventKind::Remove(_) => {
            for path in &event.paths {
                if is_json(path) {
                    removed.insert(path.clone());
                }
            }
        }
        _ => {}
    }
}

fn process_path_change(
    app: &tauri::AppHandle,
    tx: &mpsc::Sender<RegistryMessage>,
    path: &Path,
) -> Result<(), String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let parsed: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let update = parse_status_payload(&parsed).ok_or_else(|| "payload missing status".to_string())?;

    let _ = app.emit("roux-status-update", &update);

    let Some(mapped) = status_update_to_mapped(&update) else {
        // Non-FSM status (error, disconnected, unknown). Frontend still
        // saw the event via `roux-status-update`.
        return Ok(());
    };

    let identity = payload_to_identity(&update);
    let context = payload_to_event_context(&update);
    let input = AgentInput {
        identity,
        event: AgentEvent::HookStatus(mapped),
        context,
    };
    tx.send(RegistryMessage::Input(input)).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_payload_extracts_all_tier1_fields() {
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
        // Backcompat: a roux-cli shim from before the rename still
        // writes `claude_session_id`. The parser reads either key so a
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
        // Rolling upgrade: if both keys are present (a shim that writes
        // both during transition), the canonical key wins so stale data
        // from the shim's legacy field can't overwrite a fresh id.
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
    fn parse_payload_drops_payload_without_status() {
        let payload = json!({ "cwd": "/repo" });
        assert!(parse_status_payload(&payload).is_none());
    }

    #[test]
    fn parse_payload_empty_tier1_strings_treated_as_none() {
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
    fn map_status_normalizes_working_to_generating() {
        assert_eq!(map_status("working"), "generating");
        assert_eq!(map_status("idle"), "idle");
        assert_eq!(map_status("attention"), "attention");
        assert_eq!(map_status("unknown"), "unknown");
    }

    #[test]
    fn status_update_to_mapped_covers_fsm_states() {
        let mut u = bare_update("generating");
        assert_eq!(status_update_to_mapped(&u), Some(MappedStatus::Generating));
        u.status = "idle".into();
        assert_eq!(status_update_to_mapped(&u), Some(MappedStatus::Idle));
        u.status = "attention".into();
        assert_eq!(status_update_to_mapped(&u), Some(MappedStatus::Attention));
    }

    #[test]
    fn status_update_to_mapped_none_for_non_fsm_states() {
        let mut u = bare_update("error");
        assert!(status_update_to_mapped(&u).is_none());
        u.status = "disconnected".into();
        assert!(status_update_to_mapped(&u).is_none());
        u.status = "weird".into();
        assert!(status_update_to_mapped(&u).is_none());
    }

    #[test]
    fn payload_to_identity_uses_tier1_fields_directly() {
        let mut u = bare_update("attention");
        u.cwd = "/repo".into();
        u.roux_pane_id = Some("p-1".into());
        u.roux_session_id = Some("s-1".into());

        let id = payload_to_identity(&u);
        assert_eq!(id.pane_id.as_deref(), Some("p-1"));
        assert_eq!(id.session_id.as_deref(), Some("s-1"));
        assert_eq!(id.cwd, Some(PathBuf::from("/repo")));
    }

    #[test]
    fn payload_to_identity_empty_cwd_is_none() {
        let u = bare_update("idle");
        let id = payload_to_identity(&u);
        assert_eq!(id.cwd, None);
    }

    #[test]
    fn payload_to_event_context_copies_all_fields() {
        let mut u = bare_update("attention");
        u.cwd = "/repo".into();
        u.tool_name = Some("Bash".into());
        u.tool_input = Some(json!({"command": "ls"}));
        u.message = Some("hi".into());
        u.provider = "claude".into();
        u.roux_pane_id = Some("p-1".into());

        let ctx = payload_to_event_context(&u);
        assert_eq!(ctx.cwd, "/repo");
        assert_eq!(ctx.tool_name.as_deref(), Some("Bash"));
        assert!(ctx.tool_input.is_some());
        assert_eq!(ctx.message.as_deref(), Some("hi"));
        assert_eq!(ctx.provider, "claude");
        assert_eq!(ctx.roux_pane_id.as_deref(), Some("p-1"));
    }

    fn bare_update(status: &str) -> StatusUpdate {
        StatusUpdate {
            status: status.into(),
            cwd: String::new(),
            provider_session_id: None,
            provider: String::new(),
            roux_session_id: None,
            roux_pane_id: None,
            tool_name: None,
            tool_input: None,
            message: None,
        }
    }
}
