use std::path::PathBuf;

use roux_core::WorktreeProvider;
use serde::Serialize;

use crate::automation_hooks::{worktree_provider_hooks, HookContext, HookEvent};
use crate::services::worktrees as svc;
use crate::state::AppState;

// Each command below that shells out to `git` / `wt` is declared `async fn`
// and wraps the subprocess work in `tauri::async_runtime::spawn_blocking`.
// Without this, `wt remove` (which can run user-defined post-remove hooks
// for seconds) blocks Tauri's webview thread and the user sees a macOS
// beachball until the subprocess returns.

fn build_post_worktree_create_context(
    provider: WorktreeProvider,
    wt_available: bool,
    repo_path: &str,
    branch: &str,
    worktree_path: &str,
) -> HookContext {
    let mut context =
        HookContext::new(HookEvent::PostWorktreeCreate).with_provider(provider, wt_available);
    context.repo_path = Some(repo_path.to_string());
    context.worktree_path = Some(worktree_path.to_string());
    context.branch = Some(branch.to_string());
    context.cwd = Some(worktree_path.to_string());
    context.provider_hooks_ran =
        worktree_provider_hooks(HookEvent::PostWorktreeCreate, context.worktrunk);
    context
}

fn build_post_worktree_remove_context(
    provider: WorktreeProvider,
    wt_available: bool,
    repo_path: &str,
    worktree_path: &str,
) -> HookContext {
    let mut context =
        HookContext::new(HookEvent::PostWorktreeRemove).with_provider(provider, wt_available);
    context.repo_path = Some(repo_path.to_string());
    context.worktree_path = Some(worktree_path.to_string());
    context.cwd = Some(worktree_path.to_string());
    context.provider_hooks_ran =
        worktree_provider_hooks(HookEvent::PostWorktreeRemove, context.worktrunk);
    context
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_create_worktree(
    repo_path: String,
    branch: String,
    start_point: Option<String>,
    fetch_first: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    // Snapshot settings under the lock, then release before we move into
    // the blocking task.
    let (base_path, provider) = {
        let settings = state.settings.lock().unwrap();
        (settings.worktree_base_path.clone(), settings.worktree_provider)
    };
    let fetch_first = fetch_first.unwrap_or(false);
    let wt = crate::services::setup::resolve_wt_binary();
    let wt_available = wt.is_some();
    let pre_context =
        HookContext::new(HookEvent::PreWorktreeCreate).with_provider(provider, wt_available);
    let pre_context = HookContext {
        repo_path: Some(repo_path.clone()),
        branch: Some(branch.clone()),
        cwd: Some(repo_path.clone()),
        ..pre_context
    };
    state
        .automation_hooks
        .run_blocking(HookEvent::PreWorktreeCreate, pre_context)
        .await
        .map_err(|e| e.to_string())?;
    let post_hooks = state.automation_hooks.clone();
    let post_provider = provider;
    let post_repo_path = repo_path.clone();
    let post_branch = branch.clone();
    let worktree_path = tauri::async_runtime::spawn_blocking(move || {
        if fetch_first {
            roux_core::fetch_origin(&repo_path).map_err(|e| e.to_string())?;
        }
        roux_core::create_worktree_with_provider(
            &repo_path,
            &branch,
            base_path.as_deref(),
            start_point.as_deref(),
            provider,
            wt.as_ref(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("create_worktree task panicked: {e}"))?
    .map_err(|e| e.to_string())?;
    let context = build_post_worktree_create_context(
        post_provider,
        wt_available,
        &post_repo_path,
        &post_branch,
        &worktree_path,
    );
    post_hooks.spawn_background(HookEvent::PostWorktreeCreate, context);
    Ok(worktree_path)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_remove_worktree(
    repo_path: String,
    worktree_path: String,
    also_branch: Option<bool>,
    force: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let provider = state.settings.lock().unwrap().worktree_provider;
    let also_branch = also_branch.unwrap_or(false);
    let force = force.unwrap_or(false);
    let wt = crate::services::setup::resolve_wt_binary();
    let wt_available = wt.is_some();
    let pre_context = HookContext {
        repo_path: Some(repo_path.clone()),
        worktree_path: Some(worktree_path.clone()),
        cwd: Some(worktree_path.clone()),
        ..HookContext::new(HookEvent::PreWorktreeRemove).with_provider(provider, wt_available)
    };
    state
        .automation_hooks
        .run_blocking(HookEvent::PreWorktreeRemove, pre_context)
        .await
        .map_err(|e| e.to_string())?;
    let post_hooks = state.automation_hooks.clone();
    let post_repo_path = repo_path.clone();
    let post_worktree_path = worktree_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        roux_core::remove_worktree_with_provider(
            &repo_path,
            &worktree_path,
            also_branch,
            force,
            provider,
            wt.as_ref(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("remove_worktree task panicked: {e}"))?
    .map_err(|e| e.to_string())?;
    let context = build_post_worktree_remove_context(
        provider,
        wt_available,
        &post_repo_path,
        &post_worktree_path,
    );
    post_hooks.spawn_background(HookEvent::PostWorktreeRemove, context);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_list_worktrees(
    repo_path: String,
) -> Result<Vec<crate::worktree::Worktree>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let wt = crate::services::setup::resolve_wt_binary();
        roux_core::list_worktrees_enriched(&repo_path, wt.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("list_worktrees task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_list_branches(repo_path: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        svc::list_branches(&repo_path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("list_branches task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn git_init(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || svc::git_init(&path).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("git_init task panicked: {e}"))?
}

/// Resolve a worktree-base-path template (`{project_dir}`, `{git_root}`,
/// `{project_name}`, `{home}`, leading `~/`) against a sample repo path so
/// Settings can show a live preview.
#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_preview_worktree_base(template: String, repo_path: String) -> String {
    roux_core::preview_worktree_base(&template, &repo_path)
}

/// Result of probing the user's environment for a usable worktrunk install.
/// Either field can be populated independently: the binary may be on PATH
/// without any project config, or a project may carry `.config/wt.toml`
/// without the user having the CLI installed.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktrunkDetection {
    /// Resolved binary path, when detection succeeds and the version meets
    /// `roux_worktrunk::MIN_WT_VERSION`. `null` when no usable wt is found.
    pub binary_path: Option<String>,
    /// Human-readable version string (e.g. "0.44.0") when binary_path is set.
    pub version: Option<String>,
    /// True when `{repo_path}/.config/wt.toml` exists as a file.
    pub has_config: bool,
}

/// Hook definition extracted from a worktrunk config file.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktrunkHookDef {
    /// "user" or "project".
    pub source: String,
    /// Absolute path to the config file this hook lives in.
    pub config_path: String,
    /// e.g. "post-start", "pre-merge".
    pub name: String,
    /// Displayable command value — a plain string for simple hooks, a
    /// JSON-encoded string for array/object values.
    pub command: String,
}

/// Summary of where worktrunk's configs live on disk.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktrunkConfigSummary {
    pub user_path: String,
    pub user_exists: bool,
    pub project_path: String,
    pub project_exists: bool,
}

/// One log-file entry (command log or diagnostic).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktrunkLogEntry {
    pub file: String,
    pub path: String,
    pub size: u64,
    pub modified_at: Option<u64>,
}

/// Hook-output log entry with extra fields identifying which hook fired.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktrunkHookOutputEntry {
    pub file: String,
    pub path: String,
    pub size: u64,
    pub modified_at: Option<u64>,
    pub branch: String,
    pub source: String,
    pub hook_type: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktrunkLogsSummary {
    pub command_log: Vec<WorktrunkLogEntry>,
    pub hook_output: Vec<WorktrunkHookOutputEntry>,
    pub diagnostic: Vec<WorktrunkLogEntry>,
}

/// Everything the Worktrunk sidebar panel needs in a single call.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktrunkDiagnostics {
    pub hooks: Vec<WorktrunkHookDef>,
    pub config: WorktrunkConfigSummary,
    pub logs: WorktrunkLogsSummary,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_worktrunk_diagnostics(
    repo_path: String,
) -> Result<WorktrunkDiagnostics, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let wt = crate::services::setup::resolve_wt_binary().ok_or_else(|| {
            "wt is not installed or is below the minimum supported version".to_string()
        })?;
        let repo = PathBuf::from(&repo_path);

        let show = roux_worktrunk::show_config(&wt, &repo, &[]).map_err(|e| e.to_string())?;
        let hooks = roux_worktrunk::extract_hook_defs(&show)
            .into_iter()
            .map(|h| WorktrunkHookDef {
                source: h.source,
                config_path: h.config_path,
                name: h.name,
                command: h.command,
            })
            .collect();
        let config = WorktrunkConfigSummary {
            user_path: show.user.path,
            user_exists: show.user.exists,
            project_path: show.project.path,
            project_exists: show.project.exists,
        };

        let raw_logs = roux_worktrunk::list_logs(&wt, &repo, &[]).map_err(|e| e.to_string())?;
        let logs = WorktrunkLogsSummary {
            command_log: raw_logs
                .command_log
                .into_iter()
                .map(|e| WorktrunkLogEntry {
                    file: e.file,
                    path: e.path,
                    size: e.size,
                    modified_at: e.modified_at,
                })
                .collect(),
            hook_output: raw_logs
                .hook_output
                .into_iter()
                .map(|e| WorktrunkHookOutputEntry {
                    file: e.file,
                    path: e.path,
                    size: e.size,
                    modified_at: e.modified_at,
                    branch: e.branch,
                    source: e.source,
                    hook_type: e.hook_type,
                    name: e.name,
                })
                .collect(),
            diagnostic: raw_logs
                .diagnostic
                .into_iter()
                .map(|e| WorktrunkLogEntry {
                    file: e.file,
                    path: e.path,
                    size: e.size,
                    modified_at: e.modified_at,
                })
                .collect(),
        };

        Ok::<WorktrunkDiagnostics, String>(WorktrunkDiagnostics { hooks, config, logs })
    })
    .await
    .map_err(|e| format!("worktrunk_diagnostics task panicked: {e}"))?
}

/// Read a single worktrunk log file, capped at 256 KiB. Returns `None`
/// when the file doesn't exist (was rotated / pruned between listing
/// and read).
///
/// Defense-in-depth: even though the UI only supplies paths it received
/// from `cmd_worktrunk_diagnostics`, we refuse any path whose canonical
/// form does not live under `<repo_path>/.git/wt/logs/`. That way a
/// compromised frontend / XSS can't turn this into an arbitrary-file
/// read primitive.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_worktrunk_read_log(
    repo_path: String,
    path: String,
) -> Result<Option<String>, String> {
    const MAX_BYTES: u64 = 256 * 1024;
    tauri::async_runtime::spawn_blocking(move || {
        let target = PathBuf::from(&path);
        let logs_root = PathBuf::from(&repo_path).join(".git").join("wt").join("logs");

        // Canonicalize both so `..` traversal and symlink trickery can't
        // smuggle a path out of logs_root. `logs_root` may not exist yet
        // on a fresh repo; in that case any read must be refused.
        let canonical_root = logs_root
            .canonicalize()
            .map_err(|_| "worktrunk logs directory does not exist for this repo".to_string())?;
        let canonical_target =
            target.canonicalize().map_err(|e| format!("cannot resolve log path: {e}"))?;

        if !canonical_target.starts_with(&canonical_root) {
            return Err(format!("refusing to read {path}: not under {}", canonical_root.display()));
        }

        roux_worktrunk::read_log_file(&canonical_target, MAX_BYTES).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("worktrunk_read_log task panicked: {e}"))?
}

/// Open a terminal at `path`. Used by the Worktrunk panel's
/// right-click context menu.
///
/// macOS: `open -a Terminal <path>` — always Apple Terminal. (The
/// user's "default terminal" preference on macOS is not exposed via a
/// stable API, so we pick Terminal.app deliberately.)
/// Linux: best-effort `xdg-terminal-exec` if available, else
/// `x-terminal-emulator` (Debian/Ubuntu wrapper); returns an error
/// if neither resolves.
/// Windows: `wt.exe -d <path>` (Windows Terminal) — falls back to
/// `cmd /c start cmd /k "cd /d <path>"` when Windows Terminal is
/// absent or fails.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_open_terminal_at(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::process::Command;

        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .args(["-a", "Terminal"])
                .arg(&path)
                .status()
                .map_err(|e| format!("open failed: {e}"))
                .and_then(
                    |s| {
                        if s.success() {
                            Ok(())
                        } else {
                            Err(format!("open exited with {s}"))
                        }
                    },
                )
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(status) =
                Command::new("xdg-terminal-exec").env("TERMINAL_EXEC_WORKDIR", &path).status()
            {
                if status.success() {
                    return Ok(());
                }
            }
            Command::new("x-terminal-emulator")
                .arg(format!("--working-directory={path}"))
                .status()
                .map_err(|e| format!("no terminal found: {e}"))
                .and_then(|s| {
                    if s.success() {
                        Ok(())
                    } else {
                        Err(format!("x-terminal-emulator exited with {s}"))
                    }
                })
        }

        #[cfg(target_os = "windows")]
        {
            // Windows Terminal: require exit success, not just spawn.
            // `.status().is_ok()` only verifies the process started; a
            // missing `wt.exe` that somehow returned non-zero would
            // bypass the cmd.exe fallback and silently succeed.
            if let Ok(status) = Command::new("wt.exe").args(["-d", &path]).status() {
                if status.success() {
                    return Ok(());
                }
            }

            // cmd.exe fallback: paths with spaces or `&`/`|`/`^` would
            // break (or worse, inject) an unquoted `cd /d`. Wrap the
            // path in quotes and escape embedded quotes by doubling.
            let escaped_path = path.replace('"', "\"\"");
            let cd_command = format!("cd /d \"{escaped_path}\"");
            Command::new("cmd")
                .args(["/c", "start", "cmd", "/k", &cd_command])
                .status()
                .map_err(|e| format!("cmd failed: {e}"))
                .and_then(
                    |s| {
                        if s.success() {
                            Ok(())
                        } else {
                            Err(format!("cmd exited with {s}"))
                        }
                    },
                )
        }
    })
    .await
    .map_err(|e| format!("open_terminal task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_open_path_in_finder(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::process::Command;

        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .arg(&path)
                .status()
                .map_err(|e| format!("open failed: {e}"))
                .and_then(|s| {
                    if s.success() {
                        Ok(())
                    } else {
                        Err(format!("open exited with {s}"))
                    }
                })
        }

        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open")
                .arg(&path)
                .status()
                .map_err(|e| format!("xdg-open failed: {e}"))
                .and_then(|s| {
                    if s.success() {
                        Ok(())
                    } else {
                        Err(format!("xdg-open exited with {s}"))
                    }
                })
        }

        #[cfg(target_os = "windows")]
        {
            Command::new("explorer")
                .arg("/select,")
                .arg(&path)
                .status()
                .map_err(|e| format!("explorer failed: {e}"))
                .and_then(|s| {
                    if s.success() {
                        Ok(())
                    } else {
                        Err(format!("explorer exited with {s}"))
                    }
                })
        }
    })
    .await
    .map_err(|e| format!("open_path_in_finder task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_detect_worktrunk(repo_path: Option<String>) -> WorktrunkDetection {
    tauri::async_runtime::spawn_blocking(move || detect_worktrunk_inner(repo_path))
        .await
        .unwrap_or(WorktrunkDetection { binary_path: None, version: None, has_config: false })
}

fn detect_worktrunk_inner(repo_path: Option<String>) -> WorktrunkDetection {
    let wt = crate::services::setup::resolve_wt_binary();
    let (binary_path, version) = match wt {
        Some(w) => (Some(w.path.to_string_lossy().into_owned()), Some(w.version.to_string())),
        None => (None, None),
    };
    let has_config = repo_path
        .as_deref()
        .map(|p| roux_worktrunk::detect_wt_config(&PathBuf::from(p)))
        .unwrap_or(false);
    WorktrunkDetection { binary_path, version, has_config }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_create_context_sets_worktrunk_provider_hooks() {
        let context = build_post_worktree_create_context(
            WorktreeProvider::Worktrunk,
            true,
            "/repo",
            "feat/x",
            "/repo/.worktrees/feat-x",
        );

        assert_eq!(context.provider.as_deref(), Some("worktrunk"));
        assert!(context.worktrunk);
        assert_eq!(context.repo_path.as_deref(), Some("/repo"));
        assert_eq!(context.worktree_path.as_deref(), Some("/repo/.worktrees/feat-x"));
        assert_eq!(context.branch.as_deref(), Some("feat/x"));
        assert_eq!(context.cwd.as_deref(), Some("/repo/.worktrees/feat-x"));
        assert_eq!(context.provider_hooks_ran, vec!["pre-start", "post-start"]);
    }

    #[test]
    fn post_create_context_uses_git_when_worktrunk_unavailable() {
        let context = build_post_worktree_create_context(
            WorktreeProvider::Worktrunk,
            false,
            "/repo",
            "feat/x",
            "/repo/.worktrees/feat-x",
        );

        assert_eq!(context.provider.as_deref(), Some("git"));
        assert!(!context.worktrunk);
        assert!(context.provider_hooks_ran.is_empty());
    }

    #[test]
    fn post_remove_context_sets_worktrunk_provider_hooks() {
        let context = build_post_worktree_remove_context(
            WorktreeProvider::Worktrunk,
            true,
            "/repo",
            "/repo/.worktrees/feat-x",
        );

        assert_eq!(context.provider.as_deref(), Some("worktrunk"));
        assert!(context.worktrunk);
        assert_eq!(context.repo_path.as_deref(), Some("/repo"));
        assert_eq!(context.worktree_path.as_deref(), Some("/repo/.worktrees/feat-x"));
        assert_eq!(context.cwd.as_deref(), Some("/repo/.worktrees/feat-x"));
        assert_eq!(context.provider_hooks_ran, vec!["pre-remove", "post-remove"]);
    }
}
