use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::detect::WtBinary;
use crate::exec::WtError;

/// Options for [`remove_worktree`].
///
/// GUI callers should leave `force = false`: issue #101's "GUI cleanup
/// defaults must be more conservative than terminal cleanup" principle.
/// Forcing removal of a locked worktree discards data the user's hook
/// (or another worktrunk client) was protecting.
#[derive(Debug, Default, Clone)]
pub struct RemoveOpts {
    /// When `true`, also delete the branch if it's merged. Matches
    /// `wt remove` default behavior. When `false`, passes
    /// `--no-delete-branch` — the GUI default.
    pub also_branch: bool,
    /// When `true`, passes `--force` to override untracked-file and
    /// lock protections. Exposed for non-GUI escape hatches.
    pub force: bool,
    /// Extra environment for the spawned `wt`. Tests use this to
    /// override `HOME` for hermetic layout.
    pub env: Vec<(String, OsString)>,
}

/// Remove the worktree at `worktree_path` via `wt remove`.
///
/// `wt remove` operates on branch names, not paths. This wrapper looks
/// up the branch that owns `worktree_path` via `wt list --format=json`
/// first, then runs `wt remove [--no-delete-branch] [--force] <branch>`
/// from `repo_path`.
pub fn remove_worktree(
    wt: &WtBinary,
    repo_path: &Path,
    worktree_path: &Path,
    opts: &RemoveOpts,
) -> Result<(), WtError> {
    // Look up the branch that owns this worktree.
    let items = list_worktrees_with_env(wt, repo_path, &opts.env)?;
    let entry = items
        .into_iter()
        .find(|i| i.path.as_ref().map(|p| paths_eq(p, worktree_path)).unwrap_or(false));
    let Some(entry) = entry else {
        return Err(WtError::NotFound { path: worktree_path.to_string_lossy().into_owned() });
    };
    let Some(branch) = entry.branch.as_deref() else {
        return Err(WtError::NotFound {
            path: format!("{} (detached HEAD, cannot remove via wt)", worktree_path.display()),
        });
    };

    let mut cmd = Command::new(&wt.path);
    cmd.current_dir(repo_path).arg("remove");
    if !opts.also_branch {
        cmd.arg("--no-delete-branch");
    }
    if opts.force {
        cmd.arg("--force");
    }
    cmd.arg(branch);
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }

    let out = cmd.output().map_err(|source| WtError::Spawn { source })?;
    if out.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // Heuristics for `wt remove` refusals. If these substrings change
    // upstream we fall through to `NonZeroExit`, which callers can
    // still inspect.
    let lower = stderr.to_lowercase();
    if !opts.force {
        if lower.contains("locked") {
            return Err(WtError::Locked { reason: stderr });
        }
        if lower.contains("uncommitted changes") || lower.contains("uncommitted change") {
            return Err(WtError::Dirty { reason: stderr });
        }
    }
    Err(WtError::NonZeroExit { status: out.status.code().unwrap_or(-1), stderr })
}

fn paths_eq(a: &Path, b: &Path) -> bool {
    // `wt list` reports canonical paths (e.g. `/private/var/...` on
    // macOS). Test-supplied paths may be non-canonical. Normalize both
    // sides where possible.
    let a_norm = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b_norm = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    a_norm == b_norm
}

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
