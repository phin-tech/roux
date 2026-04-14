use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::platform;

const ROUX_HOOK_MARKER: &str = "roux-cli hook";

#[cfg(not(windows))]
fn unix_cli_install_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".local").join("bin").join(platform::roux_cli_file_name()))
}

fn cargo_cli_install_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cargo").join("bin").join(platform::roux_cli_file_name()))
}

fn sibling_cli_path() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| platform::sibling_roux_cli_path(&p))
}

fn first_existing_path(candidates: impl IntoIterator<Item = Option<PathBuf>>) -> Option<PathBuf> {
    candidates.into_iter().flatten().find(|path| path.is_file())
}

fn roux_cli_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let candidates = [sibling_cli_path(), cargo_cli_install_path()];
    #[cfg(not(windows))]
    let candidates = [unix_cli_install_path(), sibling_cli_path(), cargo_cli_install_path()];

    first_existing_path(candidates)
        .or_else(|| platform::find_executable_on_path(platform::roux_cli_file_name()))
        .ok_or_else(|| {
            format!(
                "{} not found. Build the CLI companion binary before installing hooks.",
                platform::roux_cli_file_name()
            )
        })
}

/// Bundled CLI version — what a freshly-installed `roux-cli` will report.
pub fn bundled_cli_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Exec the installed `roux-cli --version` and return the semver segment of
/// its output. Clap prints `"roux-cli X.Y.Z"`.
pub fn installed_cli_version() -> Option<String> {
    let path = installed_cli_path()?;
    let output = std::process::Command::new(&path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    stdout.split_whitespace().last().map(str::to_string)
}

/// Where the CLI is actually installed right now, if anywhere — mirrors
/// [`cli_is_installed`] but returns the path so callers can exec it.
fn installed_cli_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates = [cargo_cli_install_path()];
    #[cfg(not(windows))]
    let candidates = [unix_cli_install_path(), cargo_cli_install_path()];

    first_existing_path(candidates)
        .or_else(|| platform::find_executable_on_path(platform::roux_cli_file_name()))
}

/// `true` when an installed CLI is found *and* its version matches the
/// bundled one. A missing CLI or a version mismatch both return `false`.
pub fn cli_is_current() -> bool {
    installed_cli_version().as_deref() == Some(bundled_cli_version())
}

/// Check whether `roux-cli` can be found in any of the supported lookup locations,
/// including the platform-specific install path, a sibling binary, Cargo's bin
/// directory, or `PATH`.
pub fn cli_is_installed() -> bool {
    #[cfg(windows)]
    let candidates = [cargo_cli_install_path()];
    #[cfg(not(windows))]
    let candidates = [unix_cli_install_path(), cargo_cli_install_path()];

    first_existing_path(candidates)
        .or_else(|| platform::find_executable_on_path(platform::roux_cli_file_name()))
        .is_some()
}

#[cfg(windows)]
fn install_cli_binary_path() -> Result<PathBuf, String> {
    roux_cli_path()
}

#[cfg(not(windows))]
fn install_cli_binary_path() -> Result<PathBuf, String> {
    let bin_dir =
        dirs::home_dir().ok_or("Could not determine home directory")?.join(".local").join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("Failed to create ~/.local/bin: {}", e))?;

    let target = bin_dir.join(platform::roux_cli_file_name());

    // Find the source binary (next to the running roux binary)
    let source = sibling_cli_path().ok_or("Could not find roux-cli next to roux binary")?;

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
        return Ok(target);
    }

    // Fallback: try to find it anywhere
    roux_cli_path()
}

fn claude_settings_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

fn status_dir() -> Result<(), String> {
    let dir = crate::paths::roux_config_dir().join("status");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create status dir: {}", e))?;
    Ok(())
}

fn is_roux_hook_command(command: &str) -> bool {
    command.contains(ROUX_HOOK_MARKER)
        || (command.contains("roux-cli")
            && [
                " hook working",
                " hook idle",
                " hook attention",
                " hook error",
                " hook disconnected",
            ]
            .iter()
            .any(|hook| command.contains(hook)))
}

fn is_roux_hook(hook_obj: &Value) -> bool {
    hook_obj.get("command").and_then(|c| c.as_str()).map(is_roux_hook_command).unwrap_or(false)
}

fn is_roux_hook_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| hooks.iter().any(is_roux_hook))
        .unwrap_or(false)
}

fn hook_command(cli_path: &Path, status: &str) -> String {
    platform::command_string(cli_path, &["hook", status])
}

fn roux_hooks_config(cli_path: &Path) -> Value {
    json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command(cli_path, "working")
                        }
                    ]
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command(cli_path, "idle")
                        }
                    ]
                }
            ],
            "PermissionRequest": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command(cli_path, "attention")
                        }
                    ]
                }
            ],
            "StopFailure": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command(cli_path, "error")
                        }
                    ]
                }
            ],
            // SessionEnd fires on /clear, /logout, and CLI shutdown. /clear
            // does NOT exit the claude process, so mapping to "disconnected"
            // was wrong: it flipped session.status and rendered the Session
            // Disconnected screen over a live Claude. "idle" is accurate for
            // /clear (Claude is waiting for input) and harmless for /logout
            // (the PTY-shell stays alive; any follow-up input goes to the
            // shell prompt).
            "SessionEnd": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command(cli_path, "idle")
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
                            "command": hook_command(cli_path, "attention")
                        }
                    ]
                },
                {
                    "matcher": "idle_prompt",
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command(cli_path, "idle")
                        }
                    ]
                }
            ]
        }
    })
}

fn merge_hooks(settings: &mut Value, cli_path: &Path) -> Result<(), String> {
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

    let cli_path = install_cli_binary_path().or_else(|_| roux_cli_path())?;
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

    eprintln!("Roux hooks installed (using {})", cli_path.display());
    Ok(())
}

pub fn setup_is_complete() -> bool {
    let Ok(cli_path) = roux_cli_path() else {
        return false;
    };
    let Ok(settings_path) = claude_settings_path() else {
        return false;
    };
    if !settings_path.exists() {
        return false;
    }
    let Ok(content) = fs::read_to_string(settings_path) else {
        return false;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&content) else {
        return false;
    };

    hook_config_contains_expected_entries(&settings, &roux_hooks_config(&cli_path))
}

fn hook_entry_contains_command(entry: &Value, expected: &str) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command").and_then(|c| c.as_str()).map(|c| c == expected).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn hook_entry_matches_expected(entry: &Value, expected: &Value) -> bool {
    let matcher_matches = entry.get("matcher").and_then(|m| m.as_str())
        == expected.get("matcher").and_then(|m| m.as_str());
    matcher_matches
        && expected
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|expected_hooks| {
                expected_hooks.iter().all(|expected_hook| {
                    expected_hook
                        .get("command")
                        .and_then(|c| c.as_str())
                        .map(|command| hook_entry_contains_command(entry, command))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
}

fn hook_config_contains_expected_entries(settings: &Value, expected: &Value) -> bool {
    let Some(expected_hooks) = expected.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };

    expected_hooks.iter().all(|(event_name, expected_entries)| {
        let Some(expected_entries) = expected_entries.as_array() else {
            return false;
        };
        let Some(existing_entries) = settings
            .get("hooks")
            .and_then(|h| h.get(event_name))
            .and_then(|entries| entries.as_array())
        else {
            return false;
        };

        expected_entries.iter().all(|expected_entry| {
            existing_entries
                .iter()
                .any(|existing_entry| hook_entry_matches_expected(existing_entry, expected_entry))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roux_hook_detection_matches_quoted_exe_commands() {
        assert!(is_roux_hook_command(
            "\"C:\\\\Program Files\\\\Roux\\\\roux-cli.exe\" hook working"
        ));
        assert!(is_roux_hook_command("/Users/sam/.local/bin/roux-cli hook idle"));
        assert!(!is_roux_hook_command("echo roux-cli"));
    }

    #[test]
    fn hook_command_quotes_paths_with_spaces() {
        let command =
            hook_command(Path::new("C:\\Users\\Sam\\App Data\\Roux\\roux-cli.exe"), "working");
        assert_eq!(
            command,
            "\"C:\\\\Users\\\\Sam\\\\App Data\\\\Roux\\\\roux-cli.exe\" hook working"
        );
    }

    #[test]
    fn merge_hooks_replaces_existing_roux_entries() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "/old/roux-cli hook working"
                            }
                        ]
                    },
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "echo keep-me"
                            }
                        ]
                    }
                ]
            }
        });

        merge_hooks(&mut settings, Path::new("C:\\Program Files\\Roux\\roux-cli.exe")).unwrap();

        let entries = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| hook_entry_contains_command(
            entry,
            "\"C:\\\\Program Files\\\\Roux\\\\roux-cli.exe\" hook working"
        )));
        assert!(entries.iter().any(|entry| {
            entry["hooks"].as_array().unwrap()[0]["command"].as_str() == Some("echo keep-me")
        }));
    }

    #[test]
    fn setup_validation_requires_all_expected_hooks() {
        let cli_path = Path::new("C:\\Program Files\\Roux\\roux-cli.exe");
        let settings = roux_hooks_config(cli_path);
        assert!(hook_config_contains_expected_entries(&settings, &roux_hooks_config(cli_path)));
    }

    #[test]
    fn setup_validation_rejects_partial_hook_config() {
        let cli_path = Path::new("C:\\Program Files\\Roux\\roux-cli.exe");
        let mut settings = roux_hooks_config(cli_path);
        settings["hooks"].as_object_mut().unwrap().remove("StopFailure");
        assert!(!hook_config_contains_expected_entries(&settings, &roux_hooks_config(cli_path)));
    }

    #[test]
    fn setup_validation_requires_notification_matchers() {
        let cli_path = Path::new("C:\\Program Files\\Roux\\roux-cli.exe");
        let mut settings = roux_hooks_config(cli_path);
        settings["hooks"]["Notification"][0]["matcher"] = json!("wrong_matcher");
        assert!(!hook_config_contains_expected_entries(&settings, &roux_hooks_config(cli_path)));
    }
}
