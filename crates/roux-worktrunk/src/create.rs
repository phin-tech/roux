use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::detect::WtBinary;
use crate::exec::WtError;

/// Options for [`create_worktree`].
#[derive(Debug, Default, Clone)]
pub struct CreateOpts<'a> {
    /// Base branch or ref (e.g. "main", "origin/main"). When `None`, wt's
    /// default is used (typically the default branch).
    pub base: Option<&'a str>,
    /// Extra environment variables passed to the spawned `wt`. Production
    /// callers leave this empty; tests use it to override `HOME` so wt
    /// ignores the developer's user-level `~/.config/wt.toml`.
    pub env: Vec<(String, OsString)>,
}

/// Create (or check out) a worktree for `branch` via `wt switch --create
/// --no-cd`.
///
/// Semantics:
/// - If `branch` already has a worktree, this is a no-op and returns the
///   existing path.
/// - Otherwise runs `wt switch --create [--base <base>] --no-cd <branch>`
///   in `repo_path`.
/// - On success, re-runs `wt list --format=json` to recover the created
///   worktree's absolute path (wt's stdout on create is human-readable
///   and not a stable contract to parse).
pub fn create_worktree(
    wt: &WtBinary,
    repo_path: &Path,
    branch: &str,
    opts: &CreateOpts,
) -> Result<PathBuf, WtError> {
    // No-op path: branch already has a worktree on disk per `wt list`.
    if let Ok(items) = list_worktrees_with_env(wt, repo_path, &opts.env) {
        if let Some(existing) = items.iter().find(|i| i.branch.as_deref() == Some(branch)) {
            if let Some(p) = existing.path.clone() {
                return Ok(p);
            }
        }
    }

    let mut cmd = Command::new(&wt.path);
    cmd.current_dir(repo_path).args(["switch", "--create", "--no-cd"]);
    if let Some(base) = opts.base {
        cmd.args(["--base", base]);
    }
    cmd.arg(branch);
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }

    let out = cmd.output().map_err(|source| WtError::Spawn { source })?;
    if !out.status.success() {
        return Err(WtError::NonZeroExit {
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }

    // Recover the created worktree's path by re-listing with the same
    // env (so wt sees the same config). We match by branch name —
    // `--no-cd` mode doesn't flip `is_current` reliably.
    let items = list_worktrees_with_env(wt, repo_path, &opts.env)?;
    items
        .into_iter()
        .find(|i| i.branch.as_deref() == Some(branch))
        .and_then(|i| i.path)
        .ok_or_else(|| WtError::NotFound {
            path: format!(
                "wt reported success but no worktree named {branch:?} is listed"
            ),
        })
}

/// `list_worktrees` variant that forwards extra env to the spawned `wt`.
/// Internal helper used by [`create_worktree`] so the re-list phase sees
/// the same config as the create invocation.
fn list_worktrees_with_env(
    wt: &WtBinary,
    repo_path: &Path,
    env: &[(String, OsString)],
) -> Result<Vec<crate::schema::WtItem>, WtError> {
    let mut cmd = Command::new(&wt.path);
    cmd.current_dir(repo_path).args(["list", "--format=json"]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd.output().map_err(|source| WtError::Spawn { source })?;
    if !output.status.success() {
        return Err(WtError::NonZeroExit {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    serde_json::from_slice::<Vec<crate::schema::WtItem>>(&output.stdout)
        .map_err(|source| WtError::Parse { source })
}
