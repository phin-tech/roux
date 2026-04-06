use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const ROUX_HOOK_MARKER: &str = "roux-cli hook";

fn roux_cli_path() -> Result<String, String> {
    // Look for roux-cli in common locations
    let candidates = [
        // Primary install location
        dirs::home_dir().map(|h| h.join(".local").join("bin").join("roux-cli")),
        // Next to the main roux binary (dev builds)
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("roux-cli"))),
        // Installed via cargo install
        dirs::home_dir().map(|h| h.join(".cargo").join("bin").join("roux-cli")),
    ];

    for candidate in candidates.iter().flatten() {
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    // Try PATH
    if let Ok(output) = std::process::Command::new("which").arg("roux-cli").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    Err("roux-cli binary not found. Run 'cargo install --path src-tauri' or copy roux-cli to ~/.local/bin/".to_string())
}

/// Check if roux-cli is already installed at ~/.local/bin/
pub fn cli_is_installed() -> bool {
    dirs::home_dir()
        .map(|h| h.join(".local").join("bin").join("roux-cli").exists())
        .unwrap_or(false)
}

/// Install roux-cli to ~/.local/bin/ on first app load
pub fn install_cli_binary() -> Result<String, String> {
    let bin_dir =
        dirs::home_dir().ok_or("Could not determine home directory")?.join(".local").join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("Failed to create ~/.local/bin: {}", e))?;

    let target = bin_dir.join("roux-cli");

    // Find the source binary (next to the running roux binary)
    let source = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("roux-cli")))
        .ok_or("Could not find roux-cli next to roux binary")?;

    if source.exists() {
        // Only copy if source is newer or target doesn't exist
        let should_copy = if target.exists() {
            let src_modified = fs::metadata(&source).and_then(|m| m.modified()).ok();
            let tgt_modified = fs::metadata(&target).and_then(|m| m.modified()).ok();
            match (src_modified, tgt_modified) {
                (Some(s), Some(t)) => s > t,
                _ => true,
            }
        } else {
            true
        };

        if should_copy {
            fs::copy(&source, &target).map_err(|e| format!("Failed to copy roux-cli: {}", e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&target).map_err(|e| e.to_string())?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&target, perms).map_err(|e| e.to_string())?;
            }
            eprintln!("Installed roux-cli to {}", target.display());
        }
        return Ok(target.to_string_lossy().to_string());
    }

    // Fallback: try to find it anywhere
    roux_cli_path()
}

fn claude_settings_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

fn status_dir() -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    let dir = home.join(".config").join("roux").join("status");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create status dir: {}", e))?;
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

fn roux_hooks_config(cli_path: &str) -> Value {
    json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{} hook working", cli_path)
                        }
                    ]
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{} hook idle", cli_path)
                        }
                    ]
                }
            ],
            "PermissionRequest": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{} hook attention", cli_path)
                        }
                    ]
                }
            ],
            "StopFailure": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{} hook error", cli_path)
                        }
                    ]
                }
            ],
            "SessionEnd": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{} hook disconnected", cli_path)
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
                            "command": format!("{} hook attention", cli_path)
                        }
                    ]
                },
                {
                    "matcher": "idle_prompt",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{} hook idle", cli_path)
                        }
                    ]
                }
            ]
        }
    })
}

fn merge_hooks(settings: &mut Value, cli_path: &str) -> Result<(), String> {
    let roux = roux_hooks_config(cli_path);
    let roux_hooks = roux.get("hooks").unwrap().as_object().unwrap();

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

        // Filter out old Roux hooks (matches both old python script and new cli)
        let mut filtered: Vec<Value> = existing
            .into_iter()
            .filter(|entry| !is_roux_hook_entry(entry))
            // Also filter old python-based hooks
            .filter(|entry| {
                !entry
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hooks| {
                        hooks.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(|c| c.contains("roux/hook-handler.sh"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
            .collect();

        filtered.extend(roux_entries.iter().cloned());
        settings["hooks"][event_name] = Value::Array(filtered);
    }

    Ok(())
}

pub fn install_hooks() -> Result<(), String> {
    status_dir()?;

    let cli_path = install_cli_binary().or_else(|_| roux_cli_path())?;
    let settings_path = claude_settings_path()?;

    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read settings: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings: {}", e))?
    } else {
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .claude dir: {}", e))?;
        }
        json!({})
    };

    merge_hooks(&mut settings, &cli_path)?;

    let output = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, output).map_err(|e| format!("Failed to write settings: {}", e))?;

    eprintln!("Roux hooks installed (using {})", cli_path);
    Ok(())
}
