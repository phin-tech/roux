//! Daemon-side watcher that translates hook status files into authoritative
//! `SessionStatus` updates on the session service.
//!
//! This module is spawned by the daemon (and in desktop local-fallback mode)
//! **only by the process that owns the authoritative session store**. A
//! daemon-connected desktop must NOT spawn this — doing so would double-update
//! an idle store against the daemon's authoritative state.
//!
//! The watcher reads `~/.config/roux/status/*.json` files written by
//! `roux hook <status>`. Only files that include a `roux_session_id` field are
//! routed; files from unmanaged agents (no `roux_session_id`) are ignored.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;

use roux_core::SessionStatus;

use crate::session_service::SessionHandle;

#[derive(Debug, thiserror::Error)]
pub enum StatusSourceError {
    #[error("failed to create status dir: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to create watcher: {0}")]
    CreateWatcher(#[source] notify::Error),
    #[error("failed to watch status dir: {0}")]
    WatchDir(#[source] notify::Error),
}

/// Start the background watcher. Returns immediately; the watcher runs on a
/// dedicated thread. `status_dir` should be the path to the hook status
/// directory (e.g. `~/.config/roux/status`).
pub fn start_watching(
    status_dir: PathBuf,
    session_handle: SessionHandle,
) -> Result<(), StatusSourceError> {
    fs::create_dir_all(&status_dir)
        .map_err(StatusSourceError::CreateDir)?;

    let rt = tokio::runtime::Handle::current();

    let (notify_tx, notify_rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(notify_tx, notify::Config::default())
        .map_err(StatusSourceError::CreateWatcher)?;
    watcher
        .watch(&status_dir, RecursiveMode::NonRecursive)
        .map_err(StatusSourceError::WatchDir)?;

    // Scan after watch() is active so concurrent writes are either seen here
    // or delivered through notify_rx — no gap between the scan and the watcher.
    if let Ok(entries) = fs::read_dir(&status_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                        if let Some((session_id, status)) = extract_status(&parsed) {
                            let handle = session_handle.clone();
                            rt.spawn(async move {
                                let _ = handle.update_status(&session_id, status).await;
                            });
                        }
                    }
                }
            }
        }
    }

    thread::spawn(move || {
        let _watcher = watcher; // keep alive
        let debounce = Duration::from_millis(50);

        loop {
            let first = match notify_rx.recv() {
                Ok(Ok(event)) => event,
                Ok(Err(_)) => continue,
                Err(_) => break,
            };

            let mut changed: HashSet<PathBuf> = HashSet::new();
            collect_changed_json_paths(&first, &mut changed);
            while let Ok(result) = notify_rx.recv_timeout(debounce) {
                if let Ok(event) = result {
                    collect_changed_json_paths(&event, &mut changed);
                }
            }

            for path in changed {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                        if let Some((session_id, status)) = extract_status(&parsed) {
                            let handle = session_handle.clone();
                            rt.spawn(async move {
                                let _ = handle.update_status(&session_id, status).await;
                            });
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

fn collect_changed_json_paths(event: &Event, changed: &mut HashSet<PathBuf>) {
    let is_json = |p: &Path| p.extension().and_then(|e| e.to_str()) == Some("json");
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            for path in &event.paths {
                if is_json(path) {
                    changed.insert(path.clone());
                }
            }
        }
        _ => {}
    }
}

/// Extract `(roux_session_id, SessionStatus)` from a parsed hook payload.
/// Returns `None` if `roux_session_id` is absent or empty (unmanaged agent).
fn extract_status(payload: &Value) -> Option<(String, SessionStatus)> {
    let session_id = payload
        .get("roux_session_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())?;

    let raw_status = payload.get("status").and_then(|v| v.as_str())?;
    Some((session_id, SessionStatus::from_hook_status(raw_status)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_status_returns_none_without_roux_session_id() {
        let payload = json!({ "status": "working", "cwd": "/repo" });
        assert!(extract_status(&payload).is_none());
    }

    #[test]
    fn extract_status_returns_none_with_empty_roux_session_id() {
        let payload = json!({ "status": "working", "roux_session_id": "" });
        assert!(extract_status(&payload).is_none());
    }

    #[test]
    fn extract_status_returns_none_without_status() {
        let payload = json!({ "roux_session_id": "s-1" });
        assert!(extract_status(&payload).is_none());
    }

    #[test]
    fn extract_status_maps_working_to_generating() {
        let payload = json!({ "status": "working", "roux_session_id": "s-1" });
        let (id, status) = extract_status(&payload).unwrap();
        assert_eq!(id, "s-1");
        assert_eq!(status, SessionStatus::Generating);
    }

    #[test]
    fn extract_status_maps_all_known_statuses() {
        for (raw, expected) in [
            ("idle", SessionStatus::Idle),
            ("attention", SessionStatus::Attention),
            ("error", SessionStatus::Error),
            ("disconnected", SessionStatus::Disconnected),
        ] {
            let payload = json!({ "status": raw, "roux_session_id": "s-1" });
            let (_, status) = extract_status(&payload).unwrap();
            assert_eq!(status, expected, "raw={raw}");
        }
    }
}
