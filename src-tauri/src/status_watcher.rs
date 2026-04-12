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
                    let roux_pane_id_for_task = update.roux_pane_id.clone();
                    tauri::async_runtime::spawn(async move {
                        push_attention_notification(
                            &app_for_task,
                            &cwd_for_task,
                            tool_name_for_task,
                            tool_input_for_task,
                            message_for_task,
                            roux_pane_id_for_task,
                        )
                        .await;
                    });
                }
            }
        }
    });

    Ok(())
}

/// Picks the most useful "which workspace is this from?" label for the
/// notification subtitle. Prefers the user-assigned Roux session name; falls
/// back to the basename of the cwd (so external-claude invocations that don't
/// match a Roux session still get a project hint instead of nothing).
fn session_label(session_name: Option<&str>, cwd: &str) -> Option<String> {
    if let Some(name) = session_name.filter(|s| !s.is_empty()) {
        return Some(name.to_string());
    }
    let trimmed = cwd.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let basename = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if basename.is_empty() { None } else { Some(basename.to_string()) }
}

/// Builds a human-readable (title, body) for a permission-request notification.
///
/// Claude Code's PreToolUse / Notification payload has `tool_name` plus a
/// `tool_input` JSON blob whose shape varies per tool. The previous behavior
/// dumped that blob verbatim into the body, producing notifications like
/// `Bash` / `{"command":"echo \"...\""}`. This formatter keeps the body in
/// plain English: the title becomes a short verb phrase ("Run command",
/// "Edit file"), and the body becomes the most informative single field
/// for that tool (the command, the file path, the URL, etc.).
fn humanize_attention(
    tool_name: Option<&str>,
    tool_input: Option<&Value>,
    message: Option<&str>,
) -> (String, Option<String>) {
    fn s<'a>(input: Option<&'a Value>, key: &str) -> Option<&'a str> {
        input.and_then(|v| v.get(key)).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
    }
    fn truncate(s: &str, max: usize) -> String {
        // Char-boundary safe truncation; avoids slicing inside multibyte chars.
        if s.chars().count() <= max {
            return s.to_string();
        }
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }

    let Some(tool) = tool_name else {
        let title = message.unwrap_or("Permission requested").to_string();
        return (title, None);
    };

    let input = tool_input;
    match tool {
        "Bash" => {
            let body = s(input, "command").map(|c| truncate(c, 200));
            ("Run command".to_string(), body)
        }
        "Read" => ("Read file".to_string(), s(input, "file_path").map(|p| p.to_string())),
        "Write" => ("Write file".to_string(), s(input, "file_path").map(|p| p.to_string())),
        "Edit" | "MultiEdit" => {
            ("Edit file".to_string(), s(input, "file_path").map(|p| p.to_string()))
        }
        "Glob" => {
            let pattern = s(input, "pattern").unwrap_or("").to_string();
            let body = match s(input, "path") {
                Some(p) => Some(format!("{} in {}", pattern, p)),
                None if !pattern.is_empty() => Some(pattern),
                None => None,
            };
            ("Find files".to_string(), body)
        }
        "Grep" => {
            let pattern = s(input, "pattern").unwrap_or("").to_string();
            let body = match s(input, "path") {
                Some(p) => Some(format!("{} in {}", pattern, p)),
                None if !pattern.is_empty() => Some(pattern),
                None => None,
            };
            ("Search files".to_string(), body)
        }
        "WebFetch" => ("Fetch URL".to_string(), s(input, "url").map(|u| u.to_string())),
        "WebSearch" => ("Web search".to_string(), s(input, "query").map(|q| q.to_string())),
        "Task" => {
            let body = s(input, "description")
                .or_else(|| s(input, "prompt"))
                .map(|t| truncate(t, 200));
            ("Run task".to_string(), body)
        }
        "TodoWrite" => ("Update todos".to_string(), None),
        "NotebookEdit" => {
            ("Edit notebook".to_string(), s(input, "notebook_path").map(|p| p.to_string()))
        }
        // Unknown tool: keep the tool name as the title, and try to pick a
        // sensible single string field rather than dumping JSON.
        other => {
            let body = input.and_then(|v| v.as_object()).and_then(|obj| {
                for key in ["command", "file_path", "path", "url", "query", "pattern", "description", "prompt"] {
                    if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
                        if !val.is_empty() {
                            return Some(truncate(val, 200));
                        }
                    }
                }
                None
            });
            (other.to_string(), body.or_else(|| message.map(|m| m.to_string())))
        }
    }
}

async fn push_attention_notification(
    app: &tauri::AppHandle,
    cwd: &str,
    tool_name: Option<String>,
    tool_input: Option<Value>,
    message: Option<String>,
    roux_pane_id: Option<String>,
) {
    let state = app.state::<AppState>();

    // Match the cwd to a known session by worktree_path or repo_root — the
    // same match the frontend has been doing for status updates.
    let matched_session = match state.session_handle.list().await {
        Ok(sessions) => sessions
            .into_iter()
            .find(|s| s.worktree_path == cwd || s.repo_root == cwd),
        Err(_) => None,
    };
    let session_id = matched_session.as_ref().map(|s| s.id.clone());
    let session_name = matched_session.as_ref().map(|s| s.name.clone());

    let (title, body) = humanize_attention(tool_name.as_deref(), tool_input.as_ref(), message.as_deref());
    let subtitle = session_label(session_name.as_deref(), cwd);

    // Prefer pane-level focus when the hook carried a roux_pane_id (Claude
    // launched inside a Roux-managed PTY). Fall back to session focus for
    // legacy or external installs.
    let mut actions: Vec<NotificationAction> = Vec::new();
    if let Some(pane_id) = roux_pane_id {
        actions.push(NotificationAction {
            id: "focus".into(),
            label: "Focus pane".into(),
            kind: ActionKind::FocusPane { pane_id },
            primary: true,
        });
    } else if let Some(ref sid) = session_id {
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
            subtitle,
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
    fn session_label_prefers_session_name() {
        assert_eq!(session_label(Some("auth-rewrite"), "/repo/x"), Some("auth-rewrite".into()));
    }

    #[test]
    fn session_label_falls_back_to_cwd_basename_when_no_session() {
        assert_eq!(session_label(None, "/Users/me/src/roux"), Some("roux".into()));
    }

    #[test]
    fn session_label_handles_trailing_slash() {
        assert_eq!(session_label(None, "/Users/me/src/roux/"), Some("roux".into()));
    }

    #[test]
    fn session_label_treats_empty_session_name_as_missing() {
        assert_eq!(session_label(Some(""), "/repo/proj"), Some("proj".into()));
    }

    #[test]
    fn session_label_returns_none_for_empty_cwd_and_no_name() {
        assert_eq!(session_label(None, ""), None);
    }

    #[test]
    fn humanize_bash_uses_command_string() {
        let input = json!({ "command": "echo hello", "description": "say hi" });
        let (title, body) = humanize_attention(Some("Bash"), Some(&input), None);
        assert_eq!(title, "Run command");
        assert_eq!(body.as_deref(), Some("echo hello"));
    }

    #[test]
    fn humanize_bash_truncates_long_commands_at_char_boundary() {
        let cmd = "x".repeat(500);
        let input = json!({ "command": cmd });
        let (_, body) = humanize_attention(Some("Bash"), Some(&input), None);
        let body = body.unwrap();
        assert!(body.ends_with('…'));
        assert_eq!(body.chars().count(), 201);
    }

    #[test]
    fn humanize_edit_uses_file_path() {
        let input = json!({ "file_path": "/repo/src/main.rs", "old_string": "a", "new_string": "b" });
        let (title, body) = humanize_attention(Some("Edit"), Some(&input), None);
        assert_eq!(title, "Edit file");
        assert_eq!(body.as_deref(), Some("/repo/src/main.rs"));
    }

    #[test]
    fn humanize_grep_combines_pattern_and_path() {
        let input = json!({ "pattern": "TODO", "path": "src/" });
        let (title, body) = humanize_attention(Some("Grep"), Some(&input), None);
        assert_eq!(title, "Search files");
        assert_eq!(body.as_deref(), Some("TODO in src/"));
    }

    #[test]
    fn humanize_unknown_tool_picks_known_field_not_json() {
        let input = json!({ "url": "https://example.com", "extra": 1 });
        let (title, body) = humanize_attention(Some("MyCustomTool"), Some(&input), None);
        assert_eq!(title, "MyCustomTool");
        assert_eq!(body.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn humanize_unknown_tool_with_no_recognizable_field_falls_back_to_message() {
        let input = json!({ "weird_field": "x" });
        let (title, body) = humanize_attention(Some("MyCustomTool"), Some(&input), Some("explain"));
        assert_eq!(title, "MyCustomTool");
        assert_eq!(body.as_deref(), Some("explain"));
    }

    #[test]
    fn humanize_no_tool_falls_back_to_message() {
        let (title, body) = humanize_attention(None, None, Some("Permission needed for X"));
        assert_eq!(title, "Permission needed for X");
        assert_eq!(body, None);
    }

    #[test]
    fn humanize_body_never_contains_raw_json_braces() {
        // Regression: the old implementation produced bodies like
        // `{"command":"…"}`. None of the tool-specific paths should ever
        // emit a body that starts with `{`.
        let cases = vec![
            ("Bash", json!({ "command": "ls" })),
            ("Read", json!({ "file_path": "/x" })),
            ("Edit", json!({ "file_path": "/x" })),
            ("Glob", json!({ "pattern": "*.rs" })),
            ("Grep", json!({ "pattern": "fn", "path": "src/" })),
            ("WebFetch", json!({ "url": "https://x" })),
            ("Task", json!({ "description": "do stuff" })),
        ];
        for (tool, input) in cases {
            let (_, body) = humanize_attention(Some(tool), Some(&input), None);
            let body = body.unwrap_or_default();
            assert!(!body.starts_with('{'), "tool {} produced JSON-looking body: {}", tool, body);
        }
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
