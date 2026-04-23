use std::path::PathBuf;

use serde::Serialize;

use crate::services::worktrees as svc;
use crate::state::AppState;

// Each command below that shells out to `git` / `wt` is declared `async fn`
// and wraps the subprocess work in `tauri::async_runtime::spawn_blocking`.
// Without this, `wt remove` (which can run user-defined post-remove hooks
// for seconds) blocks Tauri's webview thread and the user sees a macOS
// beachball until the subprocess returns.

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
        (
            settings.worktree_base_path.clone(),
            settings.worktree_provider,
        )
    };
    let fetch_first = fetch_first.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        if fetch_first {
            roux_core::fetch_origin(&repo_path).map_err(|e| e.to_string())?;
        }
        let wt = crate::services::setup::resolve_wt_binary();
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
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_remove_worktree(
    repo_path: String,
    worktree_path: String,
    also_branch: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let provider = state.settings.lock().unwrap().worktree_provider;
    let also_branch = also_branch.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        let wt = crate::services::setup::resolve_wt_binary();
        roux_core::remove_worktree_with_provider(
            &repo_path,
            &worktree_path,
            also_branch,
            provider,
            wt.as_ref(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("remove_worktree task panicked: {e}"))?
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
    tauri::async_runtime::spawn_blocking(move || {
        svc::git_init(&path).map_err(|e| e.to_string())
    })
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
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_worktrunk_read_log(path: String) -> Result<Option<String>, String> {
    // Safety: the panel only ever passes paths it received from
    // `cmd_worktrunk_diagnostics`, which come directly from `wt config
    // state logs` — i.e. worktrunk-owned paths under `.git/wt/logs/`.
    // We still read via `std::fs` from the exact path supplied; no
    // shell interpolation, no symlink expansion beyond what the OS does
    // natively.
    const MAX_BYTES: u64 = 256 * 1024;
    tauri::async_runtime::spawn_blocking(move || {
        roux_worktrunk::read_log_file(&PathBuf::from(path), MAX_BYTES).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("worktrunk_read_log task panicked: {e}"))?
}

/// Open the host OS's default terminal at `path`. Used by the
/// Worktrunk panel's right-click context menu.
///
/// macOS: `open -a Terminal <path>` — respects the user's default
/// Terminal app binding.
/// Linux: best-effort `xdg-terminal-exec` if available, else
/// `x-terminal-emulator` (Debian/Ubuntu wrapper); returns an error
/// if neither resolves.
/// Windows: `wt.exe -d <path>` (Windows Terminal) — falls back to
/// `cmd /c start cmd /k "cd /d <path>"` when Windows Terminal is
/// absent.
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
            if let Ok(status) = Command::new("xdg-terminal-exec")
                .env("TERMINAL_EXEC_WORKDIR", &path)
                .status()
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
            if Command::new("wt.exe").args(["-d", &path]).status().is_ok() {
                return Ok(());
            }
            Command::new("cmd")
                .args(["/c", "start", "cmd", "/k", &format!("cd /d {path}")])
                .status()
                .map_err(|e| format!("cmd failed: {e}"))
                .and_then(|s| {
                    if s.success() {
                        Ok(())
                    } else {
                        Err(format!("cmd exited with {s}"))
                    }
                })
        }
    })
    .await
    .map_err(|e| format!("open_terminal task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_detect_worktrunk(repo_path: Option<String>) -> WorktrunkDetection {
    tauri::async_runtime::spawn_blocking(move || detect_worktrunk_inner(repo_path))
        .await
        .unwrap_or(WorktrunkDetection {
            binary_path: None,
            version: None,
            has_config: false,
        })
}

fn detect_worktrunk_inner(repo_path: Option<String>) -> WorktrunkDetection {
    let wt = crate::services::setup::resolve_wt_binary();
    let (binary_path, version) = match wt {
        Some(w) => (
            Some(w.path.to_string_lossy().into_owned()),
            Some(w.version.to_string()),
        ),
        None => (None, None),
    };
    let has_config = repo_path
        .as_deref()
        .map(|p| roux_worktrunk::detect_wt_config(&PathBuf::from(p)))
        .unwrap_or(false);
    WorktrunkDetection { binary_path, version, has_config }
}
