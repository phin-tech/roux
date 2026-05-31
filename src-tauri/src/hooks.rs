use serde_json::{json, Value};
#[cfg(not(windows))]
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(windows))]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::platform;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliInstallation {
    pub path: PathBuf,
    pub version: Option<String>,
}

#[cfg(not(windows))]
struct HomebrewPrefix {
    bin_dir: PathBuf,
    cellar_dir: PathBuf,
    opt_dir: PathBuf,
}

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

fn bundled_cli_source_path() -> Option<PathBuf> {
    sibling_cli_path().filter(|path| path.is_file())
}

fn first_existing_path(candidates: impl IntoIterator<Item = Option<PathBuf>>) -> Option<PathBuf> {
    candidates.into_iter().flatten().find(|path| path.is_file())
}

fn find_cli_on_path() -> Option<PathBuf> {
    platform::find_executable_on_path(platform::roux_cli_file_name())
}

fn roux_cli_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let candidates = [sibling_cli_path(), cargo_cli_install_path()];
    #[cfg(not(windows))]
    let candidates = [unix_cli_install_path(), sibling_cli_path(), cargo_cli_install_path()];

    first_existing_path(candidates).or_else(find_cli_on_path).ok_or_else(|| {
        format!(
            "{} not found. Build the CLI companion binary before installing hooks.",
            platform::roux_cli_file_name()
        )
    })
}

/// Bundled CLI version — what a freshly-installed `roux` will report.
pub fn bundled_cli_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Exec the installed `roux --version` and return the semver segment of
/// its output. Clap prints `"roux X.Y.Z"`.
pub fn installed_cli_version() -> Option<String> {
    cli_version_at(&installed_cli_path()?)
}

/// Exec `<path> --version` and return the trailing token clap prints
/// (`"roux X.Y.Z"` → `"X.Y.Z"`). `None` if the binary is missing or unrunnable.
fn cli_version_at(path: &Path) -> Option<String> {
    let output = std::process::Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    stdout.split_whitespace().last().map(str::to_string)
}

/// Decide whether the bundled CLI should be (re)written over whatever is
/// installed at the target path.
///
/// Version is the source of truth — file mtime is intentionally NOT used. A
/// freshly downloaded app bundle can carry an *older* binary mtime than a
/// previously installed copy, so an mtime gate silently skipped real updates
/// and left a stale CLI on disk (the "click install, it flashes, still says to
/// install" bug). Using the bundled version here keeps the installer's notion
/// of "current" identical to the doctor panel's, so they never disagree. This
/// intentionally also replaces an installed CLI with a newer version than the
/// bundle: the desktop app owns the companion CLI version it launches.
#[cfg(not(windows))]
fn should_install_cli(
    target_exists: bool,
    installed_version: Option<&str>,
    bundled_version: &str,
) -> bool {
    !target_exists || installed_version != Some(bundled_version)
}

#[cfg(not(windows))]
static CLI_STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(not(windows))]
#[derive(Debug)]
enum CliInstallError {
    MissingTargetParent,
    MissingTargetFileName,
    Stage { path: PathBuf, source: std::io::Error },
    Metadata { path: PathBuf, source: std::io::Error },
    Permissions { path: PathBuf, source: std::io::Error },
    Rename { staged: PathBuf, target: PathBuf, source: std::io::Error },
}

#[cfg(not(windows))]
impl fmt::Display for CliInstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTargetParent => write!(f, "install target has no parent directory"),
            Self::MissingTargetFileName => write!(f, "install target has no file name"),
            Self::Stage { path, source } => {
                write!(f, "Failed to stage roux at {}: {}", path.display(), source)
            }
            Self::Metadata { path, source } => {
                write!(f, "Failed to read staged roux metadata at {}: {}", path.display(), source)
            }
            Self::Permissions { path, source } => {
                write!(f, "Failed to set staged roux permissions at {}: {}", path.display(), source)
            }
            Self::Rename { staged, target, source } => write!(
                f,
                "Failed to install roux from {} to {}: {}",
                staged.display(),
                target.display(),
                source
            ),
        }
    }
}

#[cfg(not(windows))]
impl std::error::Error for CliInstallError {}

/// Where the CLI is actually installed right now, if anywhere — mirrors
/// [`cli_is_installed`] but returns the path so callers can exec it.
fn installed_cli_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates = [cargo_cli_install_path()];
    #[cfg(not(windows))]
    let candidates = [unix_cli_install_path(), cargo_cli_install_path()];

    first_existing_path(candidates).or_else(find_cli_on_path)
}

/// `true` when an installed CLI is found *and* its version matches the
/// bundled one. A missing CLI or a version mismatch both return `false`.
pub fn cli_is_current() -> bool {
    installed_cli_version().as_deref() == Some(bundled_cli_version())
}

/// Check whether `roux` can be found in any of the supported lookup locations,
/// including the platform-specific install path, a sibling binary, Cargo's bin
/// directory, or `PATH`.
pub fn cli_is_installed() -> bool {
    #[cfg(windows)]
    let candidates = [cargo_cli_install_path()];
    #[cfg(not(windows))]
    let candidates = [unix_cli_install_path(), cargo_cli_install_path()];

    first_existing_path(candidates).or_else(find_cli_on_path).is_some()
}

#[cfg(not(windows))]
fn homebrew_prefixes() -> Vec<HomebrewPrefix> {
    [
        HomebrewPrefix {
            bin_dir: PathBuf::from("/opt/homebrew/bin"),
            cellar_dir: PathBuf::from("/opt/homebrew/Cellar"),
            opt_dir: PathBuf::from("/opt/homebrew/opt"),
        },
        HomebrewPrefix {
            bin_dir: PathBuf::from("/usr/local/bin"),
            cellar_dir: PathBuf::from("/usr/local/Cellar"),
            opt_dir: PathBuf::from("/usr/local/opt"),
        },
        HomebrewPrefix {
            bin_dir: PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
            cellar_dir: PathBuf::from("/home/linuxbrew/.linuxbrew/Cellar"),
            opt_dir: PathBuf::from("/home/linuxbrew/.linuxbrew/opt"),
        },
    ]
    .into()
}

#[cfg(not(windows))]
fn is_homebrew_link(path: &Path, prefix: &HomebrewPrefix) -> bool {
    let Ok(resolved) = path.canonicalize() else {
        return false;
    };
    let cellar_dir = prefix.cellar_dir.canonicalize().unwrap_or_else(|_| prefix.cellar_dir.clone());
    let opt_dir = prefix.opt_dir.canonicalize().unwrap_or_else(|_| prefix.opt_dir.clone());
    resolved.starts_with(cellar_dir) || resolved.starts_with(opt_dir)
}

#[cfg(not(windows))]
pub fn homebrew_cli_installation() -> Option<CliInstallation> {
    let file_name = platform::roux_cli_file_name();
    for prefix in homebrew_prefixes() {
        let path = prefix.bin_dir.join(file_name);
        if path.is_file() && is_homebrew_link(&path, &prefix) {
            return Some(CliInstallation { version: cli_version_at(&path), path });
        }
    }
    None
}

#[cfg(windows)]
pub fn homebrew_cli_installation() -> Option<CliInstallation> {
    None
}

fn stale_homebrew_cli_notice_for(
    installation: CliInstallation,
    bundled_version: &str,
) -> Option<String> {
    let installed_version = installation.version.as_deref()?;
    let installed = semver::Version::parse(installed_version).ok()?;
    let bundled = semver::Version::parse(bundled_version).ok()?;
    if installed >= bundled {
        return None;
    }

    let formula = if bundled_version.contains("-pre") || installed_version.contains("-pre") {
        "phin-tech/tap/roux-pre"
    } else {
        "phin-tech/tap/roux"
    };

    Some(format!(
        "Homebrew roux detected at {} ({installed_version}). Roux does not upgrade Homebrew formulae from the app; run `brew upgrade {formula}` to update that install.",
        installation.path.display()
    ))
}

pub fn stale_homebrew_cli_notice() -> Option<String> {
    stale_homebrew_cli_notice_for(homebrew_cli_installation()?, bundled_cli_version())
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
    // Prefer the bundled sidecar in packaged builds, but source/dev builds may
    // only have roux on PATH or in Cargo's bin directory.
    if let Some(source) = bundled_cli_source_path() {
        // Replace the installed CLI whenever its version differs from the
        // bundled one (or it's missing). See `should_install_cli` for why mtime
        // is deliberately not used here.
        let target_exists = target.exists();
        let installed = if target_exists { cli_version_at(&target) } else { None };
        if should_install_cli(target_exists, installed.as_deref(), bundled_cli_version()) {
            atomic_install_cli(&source, &target).map_err(|e| e.to_string())?;
            eprintln!("Installed roux CLI to {}", target.display());
        }
        return Ok(target);
    }

    // Fallback: try to find it anywhere
    roux_cli_path()
}

pub fn install_cli() -> Result<(), String> {
    install_cli_binary_path().map(|_| ())
}

/// Copy `source` over `target` atomically: stage a temp file next to the
/// target, mark it executable, then `rename` it into place. The rename is a
/// same-filesystem atomic swap, so it's safe even when the current `roux` is
/// running (the running process keeps its already-open inode) and never leaves
/// a half-written binary on failure.
#[cfg(not(windows))]
fn atomic_install_cli(source: &Path, target: &Path) -> Result<(), CliInstallError> {
    use std::os::unix::fs::PermissionsExt;

    let dir = target.parent().ok_or(CliInstallError::MissingTargetParent)?;
    let staged = unique_cli_stage_path(dir, target)?;

    fs::copy(source, &staged).map_err(|e| {
        let _ = fs::remove_file(&staged);
        CliInstallError::Stage { path: staged.clone(), source: e }
    })?;

    let mut perms = fs::metadata(&staged)
        .map_err(|e| CliInstallError::Metadata { path: staged.clone(), source: e })?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&staged, perms).map_err(|e| {
        let _ = fs::remove_file(&staged);
        CliInstallError::Permissions { path: staged.clone(), source: e }
    })?;

    fs::rename(&staged, target).map_err(|e| {
        let _ = fs::remove_file(&staged);
        CliInstallError::Rename { staged, target: target.to_path_buf(), source: e }
    })
}

#[cfg(not(windows))]
fn unique_cli_stage_path(dir: &Path, target: &Path) -> Result<PathBuf, CliInstallError> {
    let target_name =
        target.file_name().ok_or(CliInstallError::MissingTargetFileName)?.to_string_lossy();
    let nonce = CLI_STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();

    Ok(dir.join(format!(".{target_name}.{}.{}.{}.tmp", std::process::id(), timestamp, nonce)))
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
    let Some((program, args)) = split_command_program(command) else {
        return false;
    };
    command_program_is_roux_cli(program) && args_start_with_roux_hook_status(args)
}

fn split_command_program(command: &str) -> Option<(&str, &str)> {
    let trimmed = command.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix('"') {
        let quote = rest.find('"')?;
        let (program, remainder) = rest.split_at(quote);
        return Some((program, remainder[1..].trim_start()));
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let program = parts.next()?;
    Some((program, parts.next().unwrap_or("").trim_start()))
}

fn command_program_is_roux_cli(program: &str) -> bool {
    let file_name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    matches!(
        file_name.to_ascii_lowercase().as_str(),
        "roux" | "roux.exe" | "roux-cli" | "roux-cli.exe"
    )
}

fn args_start_with_roux_hook_status(args: &str) -> bool {
    let mut args = args.split_whitespace();
    let Some(hook) = args.next() else {
        return false;
    };
    if hook != "hook" {
        return false;
    }

    matches!(args.next(), Some("working" | "idle" | "attention" | "error" | "disconnected"))
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
            "PostToolUse": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command(cli_path, "working")
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
        assert!(is_roux_hook_command("\"C:\\\\Program Files\\\\Roux\\\\roux.exe\" hook working"));
        assert!(is_roux_hook_command(
            "\"C:\\\\Program Files\\\\Roux\\\\roux-cli.exe\" hook working"
        ));
        assert!(is_roux_hook_command("/Users/sam/.local/bin/roux hook idle"));
        assert!(is_roux_hook_command("/Users/sam/.local/bin/roux-cli hook idle"));
        assert!(!is_roux_hook_command("echo roux-cli"));
        assert!(!is_roux_hook_command("/home/user/projects/kangaroux/scripts/deploy.sh hook idle"));
    }

    #[test]
    fn hook_command_quotes_paths_with_spaces() {
        let command =
            hook_command(Path::new("C:\\Users\\Sam\\App Data\\Roux\\roux.exe"), "working");
        assert_eq!(command, "\"C:\\\\Users\\\\Sam\\\\App Data\\\\Roux\\\\roux.exe\" hook working");
    }

    #[cfg(not(windows))]
    #[test]
    fn installs_cli_when_target_missing() {
        assert!(should_install_cli(false, None, "0.5.3"));
    }

    #[cfg(not(windows))]
    #[test]
    fn installs_cli_when_installed_version_differs() {
        // The reported bug: an older / pre-release CLI is installed and must be
        // replaced regardless of file mtimes.
        assert!(should_install_cli(true, Some("0.5.2"), "0.5.3"));
        assert!(should_install_cli(true, Some("0.5.3-pre"), "0.5.3"));
        // The bundled desktop version owns the installed companion CLI, so a
        // newer installed CLI is also replaced to keep app and CLI contracts in
        // sync.
        assert!(should_install_cli(true, Some("0.5.4"), "0.5.3"));
    }

    #[cfg(not(windows))]
    #[test]
    fn skips_cli_install_when_versions_match() {
        // Keeps startup idempotent: no copy when already current.
        assert!(!should_install_cli(true, Some("0.5.3"), "0.5.3"));
    }

    #[cfg(not(windows))]
    #[test]
    fn installs_cli_when_installed_version_unknown() {
        // Present but unrunnable / corrupt → reinstall to be safe.
        assert!(should_install_cli(true, None, "0.5.3"));
    }

    #[cfg(not(windows))]
    #[test]
    fn homebrew_notice_points_prereleases_at_prerelease_formula() {
        let installation = CliInstallation {
            path: PathBuf::from("/opt/homebrew/bin/roux"),
            version: Some("0.5.3-pre.1".to_string()),
        };

        let notice = stale_homebrew_cli_notice_for(installation, "0.5.4-pre.2").unwrap();

        assert!(notice.contains("/opt/homebrew/bin/roux"));
        assert!(notice.contains("0.5.3-pre.1"));
        assert!(notice.contains("brew upgrade phin-tech/tap/roux-pre"));
    }

    #[cfg(not(windows))]
    #[test]
    fn homebrew_notice_skips_unknown_or_current_or_newer_versions() {
        let path = PathBuf::from("/opt/homebrew/bin/roux");

        assert!(stale_homebrew_cli_notice_for(
            CliInstallation { path: path.clone(), version: None },
            "0.5.4"
        )
        .is_none());
        assert!(stale_homebrew_cli_notice_for(
            CliInstallation { path: path.clone(), version: Some("0.5.4".to_string()) },
            "0.5.4"
        )
        .is_none());
        assert!(stale_homebrew_cli_notice_for(
            CliInstallation { path, version: Some("0.5.5".to_string()) },
            "0.5.4"
        )
        .is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn homebrew_link_detection_requires_cellar_or_opt_target() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let cellar = dir.path().join("Cellar").join("roux").join("0.5.4").join("bin");
        let opt = dir.path().join("opt").join("roux").join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&cellar).unwrap();
        fs::create_dir_all(&opt).unwrap();

        let prefix = HomebrewPrefix {
            bin_dir: bin.clone(),
            cellar_dir: dir.path().join("Cellar").canonicalize().unwrap(),
            opt_dir: dir.path().join("opt").canonicalize().unwrap(),
        };
        let cellar_binary = cellar.join("roux");
        let opt_binary = opt.join("roux");
        let manual_binary = bin.join("manual-roux");
        fs::write(&cellar_binary, b"binary").unwrap();
        fs::write(&opt_binary, b"binary").unwrap();
        fs::write(&manual_binary, b"binary").unwrap();

        let linked_from_cellar = bin.join("roux-cellar");
        let linked_from_opt = bin.join("roux-opt");
        std::os::unix::fs::symlink(&cellar_binary, &linked_from_cellar).unwrap();
        std::os::unix::fs::symlink(&opt_binary, &linked_from_opt).unwrap();

        assert!(is_homebrew_link(&linked_from_cellar, &prefix));
        assert!(is_homebrew_link(&linked_from_opt, &prefix));
        assert!(!is_homebrew_link(&manual_binary, &prefix));
    }

    #[cfg(not(windows))]
    #[test]
    fn atomic_install_cli_replaces_contents_and_sets_exec_perms() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source-roux");
        let target = dir.path().join(platform::roux_cli_file_name());
        fs::write(&source, b"new-binary").unwrap();
        fs::write(&target, b"old-binary").unwrap();

        atomic_install_cli(&source, &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new-binary");
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        // No staged temp file left behind.
        let staged = dir.path().join(format!(".{}.tmp", platform::roux_cli_file_name()));
        assert!(!staged.exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn atomic_install_cli_stages_next_to_requested_target_name() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source-roux");
        let target = dir.path().join("roux-custom");
        fs::write(&source, b"new-binary").unwrap();

        atomic_install_cli(&source, &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new-binary");
        assert!(!dir.path().join(".roux-custom.tmp").exists());
        assert!(!dir.path().join(format!(".{}.tmp", platform::roux_cli_file_name())).exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn cli_stage_paths_are_unique_per_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("roux");

        let first = unique_cli_stage_path(dir.path(), &target).unwrap();
        let second = unique_cli_stage_path(dir.path(), &target).unwrap();

        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(dir.path()));
        assert_eq!(second.parent(), Some(dir.path()));
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

        merge_hooks(&mut settings, Path::new("C:\\Program Files\\Roux\\roux.exe")).unwrap();

        let entries = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| hook_entry_contains_command(
            entry,
            "\"C:\\\\Program Files\\\\Roux\\\\roux.exe\" hook working"
        )));
        assert!(entries.iter().any(|entry| {
            entry["hooks"].as_array().unwrap()[0]["command"].as_str() == Some("echo keep-me")
        }));
    }

    #[test]
    fn setup_validation_requires_all_expected_hooks() {
        let cli_path = Path::new("C:\\Program Files\\Roux\\roux.exe");
        let settings = roux_hooks_config(cli_path);
        assert!(hook_config_contains_expected_entries(&settings, &roux_hooks_config(cli_path)));
    }

    #[test]
    fn hook_config_marks_post_tool_use_as_working() {
        let cli_path = Path::new("/Applications/Roux.app/Contents/MacOS/roux");
        let settings = roux_hooks_config(cli_path);
        let entries = settings["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(hook_entry_contains_command(
            &entries[0],
            "/Applications/Roux.app/Contents/MacOS/roux hook working"
        ));
    }

    #[test]
    fn setup_validation_rejects_partial_hook_config() {
        let cli_path = Path::new("C:\\Program Files\\Roux\\roux.exe");
        let mut settings = roux_hooks_config(cli_path);
        settings["hooks"].as_object_mut().unwrap().remove("StopFailure");
        assert!(!hook_config_contains_expected_entries(&settings, &roux_hooks_config(cli_path)));
    }

    #[test]
    fn setup_validation_requires_notification_matchers() {
        let cli_path = Path::new("C:\\Program Files\\Roux\\roux.exe");
        let mut settings = roux_hooks_config(cli_path);
        settings["hooks"]["Notification"][0]["matcher"] = json!("wrong_matcher");
        assert!(!hook_config_contains_expected_entries(&settings, &roux_hooks_config(cli_path)));
    }
}
