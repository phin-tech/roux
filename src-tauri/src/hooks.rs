use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const HOOK_HANDLER_SCRIPT: &str = r#"#!/usr/bin/env python3
"""Roux hook handler — receives Claude Code hook events and writes status files."""
import json, os, sys, time

status = sys.argv[1] if len(sys.argv) > 1 else ""
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)

sid = data.get("session_id", "")
if not sid:
    sys.exit(0)

out = {
    "status": status,
    "claude_session_id": sid,
    "cwd": data.get("cwd", ""),
    "timestamp": int(time.time()),
}

if status == "attention":
    out["tool_name"] = data.get("tool_name", "")
    out["tool_input"] = data.get("tool_input", {})
    out["message"] = data.get("message", "")

status_dir = os.path.expanduser("~/.config/roux/status")
os.makedirs(status_dir, exist_ok=True)
with open(os.path.join(status_dir, f"{sid}.json"), "w") as f:
    json.dump(out, f)
"#;

const ROUX_HOOK_MARKER: &str = "roux/hook-handler.sh";

fn roux_config_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".config").join("roux"))
}

fn claude_settings_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

fn write_hook_handler() -> Result<(), String> {
    let config_dir = roux_config_dir()?;
    fs::create_dir_all(&config_dir).map_err(|e| format!("Failed to create config dir: {}", e))?;

    let script_path = config_dir.join("hook-handler.sh");
    fs::write(&script_path, HOOK_HANDLER_SCRIPT)
        .map_err(|e| format!("Failed to write hook handler: {}", e))?;

    let mut perms = fs::metadata(&script_path)
        .map_err(|e| format!("Failed to read metadata: {}", e))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms)
        .map_err(|e| format!("Failed to set permissions: {}", e))?;

    // Also ensure status directory exists
    fs::create_dir_all(config_dir.join("status"))
        .map_err(|e| format!("Failed to create status dir: {}", e))?;

    Ok(())
}

fn is_roux_hook(hook_obj: &Value) -> bool {
    hook_obj
        .get("command")
        .and_then(|c| c.as_str())
        .map(|c| c.contains(ROUX_HOOK_MARKER))
        .unwrap_or(false)
}

fn is_roux_hook_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| hooks.iter().any(is_roux_hook))
        .unwrap_or(false)
}

fn roux_hooks_config() -> Value {
    json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "~/.config/roux/hook-handler.sh working"
                        }
                    ]
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "~/.config/roux/hook-handler.sh idle"
                        }
                    ]
                }
            ],
            "PermissionRequest": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "~/.config/roux/hook-handler.sh attention"
                        }
                    ]
                }
            ],
            "Notification": [
                {
                    "matcher": "permission_prompt",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "~/.config/roux/hook-handler.sh attention"
                        }
                    ]
                },
                {
                    "matcher": "idle_prompt",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "~/.config/roux/hook-handler.sh idle"
                        }
                    ]
                }
            ]
        }
    })
}

fn merge_hooks(settings: &mut Value) -> Result<(), String> {
    let roux = roux_hooks_config();
    let roux_hooks = roux.get("hooks").unwrap().as_object().unwrap();

    // Ensure settings has a "hooks" object
    if settings.get("hooks").is_none() || !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }

    for (event_name, roux_entries) in roux_hooks {
        let roux_entries = roux_entries.as_array().unwrap();

        let existing = settings["hooks"]
            .get(event_name)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Filter out old Roux hooks
        let mut filtered: Vec<Value> = existing
            .into_iter()
            .filter(|entry| !is_roux_hook_entry(entry))
            .collect();

        // Append new Roux hooks
        filtered.extend(roux_entries.iter().cloned());

        settings["hooks"][event_name] = Value::Array(filtered);
    }

    Ok(())
}

pub fn install_hooks() -> Result<(), String> {
    write_hook_handler()?;

    let settings_path = claude_settings_path()?;

    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read settings: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings: {}", e))?
    } else {
        // Create the directory if needed
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .claude dir: {}", e))?;
        }
        json!({})
    };

    merge_hooks(&mut settings)?;

    let output = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, output)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    Ok(())
}
