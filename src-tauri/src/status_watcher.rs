use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusUpdate {
    status: String,
    cwd: String,
    claude_session_id: String,
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
        "attention" => "error",
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

        for result in rx {
            let event = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {}
                _ => continue,
            }

            for path in &event.paths {
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }

                // Small delay to ensure file is fully written
                thread::sleep(std::time::Duration::from_millis(10));

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

                let mapped = map_status(&raw_status);

                // Emit a global event with cwd so frontend can match to the right session
                let update = StatusUpdate {
                    status: mapped.to_string(),
                    cwd,
                    claude_session_id: claude_sid,
                };

                let _ = app.emit("roux-status-update", &update);
            }
        }
    });

    Ok(())
}
