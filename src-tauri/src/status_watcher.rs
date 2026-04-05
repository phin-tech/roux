use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusUpdate {
    status: String,
    cwd: String,
    claude_session_id: String,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    message: Option<String>,
}

fn status_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    let dir = home.join(".config").join("roux").join("status");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create status dir: {}", e))?;
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

pub fn start_watching(app: tauri::AppHandle) -> Result<(), String> {
    let watch_dir = status_dir()?;

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())
        .map_err(|e| format!("Failed to create watcher: {}", e))?;

    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to watch status dir: {}", e))?;

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
                if matches!(first.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    if path.extension().and_then(|e| e.to_str()) == Some("json") {
                        changed_paths.insert(path);
                    }
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

                let cwd = parsed
                    .get("cwd")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();

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
                    cwd,
                    claude_session_id: claude_sid,
                    tool_name,
                    tool_input,
                    message,
                };

                let _ = app.emit("roux-status-update", &update);
            }
        }
    });

    Ok(())
}
