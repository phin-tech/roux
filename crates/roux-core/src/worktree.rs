use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

use crate::models::{Worktree, WorktreeProvider, WorktrunkMetadata};

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("Failed to run git: {source}")]
    RunGit {
        #[source]
        source: std::io::Error,
    },
    #[error("git worktree add failed: {stderr}")]
    AddFailed { stderr: String },
    #[error("git worktree remove failed: {stderr}")]
    RemoveFailed { stderr: String },
    #[error("git worktree list failed: {stderr}")]
    ListFailed { stderr: String },
    #[error("git fetch origin failed: {stderr}")]
    FetchFailed { stderr: String },
    #[error("Invalid start point: '{start_point}' does not resolve to a commit")]
    InvalidStartPoint { start_point: String },
    /// `wt` refused to remove a worktree because it is locked. Surface this
    /// to the user so they can unlock deliberately — see issue #101.
    #[error("worktree is locked (wt): {reason}")]
    WorktrunkLocked { reason: String },
}

fn sanitize_branch_for_path(branch: &str) -> String {
    branch
        .replace('/', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

fn repo_name(repo_path: &str) -> String {
    Path::new(repo_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string())
}

/// Resolve the `git_root` for a repo path by asking git. Falls back to the
/// path itself if the shell-out fails (e.g. in tests, or when the directory
/// does not yet exist).
fn git_root(repo_path: &str) -> String {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(repo_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| repo_path.to_string())
}

/// Expand `{project_dir}`, `{git_root}`, `{project_name}`, `{home}` and a
/// leading `~` / `~/` in a worktree-base-path template relative to
/// `repo_path`. Pure-ish: shells out to git only when `{git_root}` is used.
pub fn expand_base_template(template: &str, repo_path: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let home_str = home.to_string_lossy().to_string();
    let project_name = repo_name(repo_path);

    // Tilde only expands at the very start — matches shell semantics and
    // avoids accidentally rewriting `foo/~bar`.
    let (prefix, rest) = if template == "~" {
        (home_str.clone(), String::new())
    } else if let Some(rest) = template.strip_prefix("~/") {
        (home_str.clone(), format!("/{}", rest))
    } else {
        (String::new(), template.to_string())
    };

    // Lazy: only resolve git_root if the template actually references it.
    let git_root_str = if rest.contains("{git_root}") || prefix.contains("{git_root}") {
        git_root(repo_path)
    } else {
        String::new()
    };

    let expanded = format!("{}{}", prefix, rest)
        .replace("{project_dir}", repo_path)
        .replace("{git_root}", &git_root_str)
        .replace("{project_name}", &project_name)
        .replace("{home}", &home_str);

    PathBuf::from(expanded)
}

/// Produce the resolved base directory a user's template would land in for
/// a given repo, without picking a concrete worktree leaf. Used by the
/// Settings UI preview.
pub fn preview_worktree_base(template: &str, repo_path: &str) -> String {
    if template.is_empty() {
        return Path::new(repo_path)
            .parent()
            .unwrap_or(Path::new("."))
            .to_string_lossy()
            .to_string();
    }
    expand_base_template(template, repo_path).to_string_lossy().to_string()
}

fn resolve_worktree_path(repo_path: &str, branch: &str, base_path: Option<&str>) -> PathBuf {
    let sanitized = sanitize_branch_for_path(branch);
    let name = repo_name(repo_path);
    let dir_name = format!("{}-{}", name, sanitized);

    let base = match base_path {
        Some(p) if !p.is_empty() => expand_base_template(p, repo_path),
        _ => Path::new(repo_path).parent().unwrap_or(Path::new(".")).to_path_buf(),
    };

    let mut target = base.join(&dir_name);
    let mut suffix = 2;
    while target.exists() {
        target = base.join(format!("{}-{}", dir_name, suffix));
        suffix += 1;
    }
    target
}

fn branch_exists(repo_path: &str, branch: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .current_dir(repo_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `true` iff `rev` resolves to a commit in `repo_path`. The `^{commit}`
/// peel rejects refs that exist but don't point at a commit (e.g. annotated
/// tag objects pointing at trees/blobs, or `HEAD` in an unborn repo). This
/// keeps our `InvalidStartPoint` error text truthful ("does not resolve to
/// a commit") and avoids handing git a non-commit start point for
/// `worktree add -b`.
fn rev_exists(repo_path: &str, rev: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", &format!("{}^{{commit}}", rev)])
        .current_dir(repo_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Runs `git fetch origin` in `repo_path`. Used before creating a worktree
/// off a remote ref (e.g. `origin/main`) so the ref exists locally.
pub fn fetch_origin(repo_path: &str) -> Result<(), WorktreeError> {
    let output = Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(repo_path)
        .output()
        .map_err(|source| WorktreeError::RunGit { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorktreeError::FetchFailed { stderr: stderr.to_string() });
    }
    Ok(())
}

/// Create a new worktree for `branch` under a base path derived from
/// `base_path` (filesystem location — see `resolve_worktree_path`).
///
/// Semantics:
/// - If `branch` is already checked out in an existing worktree, return that
///   path (no-op).
/// - If `branch` exists in the repo, run `git worktree add <path> <branch>`.
///   `start_point` is ignored because git checks out the existing branch.
/// - If `branch` does not exist and `start_point` is `Some(sp)`, run
///   `git worktree add -b <branch> <path> <sp>` (new branch from `sp`).
/// - If `branch` does not exist and `start_point` is `None`, run
///   `git worktree add -b <branch> <path>` (new branch from HEAD).
///
/// Create a worktree using the requested provider.
///
/// - `provider = Git` (or `Auto` with `wt = None`) → native `git worktree add`.
/// - `provider = Worktrunk` (or `Auto` with `wt = Some`) → shell out to
///   `wt switch --create`. On any wt failure, log and fall back to native git
///   so worktree creation never breaks entirely.
pub fn create_worktree_with_provider(
    repo_path: &str,
    branch: &str,
    base_path: Option<&str>,
    start_point: Option<&str>,
    provider: WorktreeProvider,
    wt: Option<&roux_worktrunk::WtBinary>,
) -> Result<String, WorktreeError> {
    let use_wt = match provider {
        WorktreeProvider::Git => false,
        WorktreeProvider::Worktrunk => wt.is_some(),
        WorktreeProvider::Auto => wt.is_some(),
    };

    if use_wt {
        if let Some(wt) = wt {
            let opts = roux_worktrunk::CreateOpts {
                base: start_point,
                env: Vec::new(),
            };
            match roux_worktrunk::create_worktree(wt, Path::new(repo_path), branch, &opts) {
                Ok(path) => return Ok(path.to_string_lossy().into_owned()),
                Err(err) => {
                    eprintln!(
                        "roux-worktrunk: create failed ({err}); falling back to native git for {repo_path}"
                    );
                    // Fall through to native path.
                }
            }
        }
    }

    create_worktree(repo_path, branch, base_path, start_point)
}

pub fn create_worktree(
    repo_path: &str,
    branch: &str,
    base_path: Option<&str>,
    start_point: Option<&str>,
) -> Result<String, WorktreeError> {
    // Check if the branch is already checked out in an existing worktree
    if let Ok(worktrees) = list_worktrees(repo_path) {
        if let Some(wt) = worktrees.iter().find(|wt| wt.branch == branch) {
            return Ok(wt.path.clone());
        }
    }

    let target = resolve_worktree_path(repo_path, branch, base_path);
    let target_str = target.to_string_lossy().to_string();

    let output = if branch_exists(repo_path, branch) {
        Command::new("git")
            .args(["worktree", "add", &target_str, branch])
            .current_dir(repo_path)
            .output()
            .map_err(|source| WorktreeError::RunGit { source })?
    } else if let Some(sp) = start_point {
        if !rev_exists(repo_path, sp) {
            return Err(WorktreeError::InvalidStartPoint {
                start_point: sp.to_string(),
            });
        }
        Command::new("git")
            .args(["worktree", "add", "-b", branch, &target_str, sp])
            .current_dir(repo_path)
            .output()
            .map_err(|source| WorktreeError::RunGit { source })?
    } else {
        Command::new("git")
            .args(["worktree", "add", "-b", branch, &target_str])
            .current_dir(repo_path)
            .output()
            .map_err(|source| WorktreeError::RunGit { source })?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorktreeError::AddFailed { stderr: stderr.to_string() });
    }

    Ok(target_str)
}

/// Remove a worktree using the requested provider.
///
/// - `provider = Git` (or `Auto` + `wt = None`) → native `git worktree remove --force`.
/// - `provider = Worktrunk` (or `Auto` + `wt = Some`) → `wt remove` via
///   [`roux_worktrunk::remove_worktree`], which honors lock semantics:
///   a locked worktree raises `WorktrunkLocked` instead of being silently forced.
///
/// Fallback: on non-lock wt failures, falls through to native git so
/// removal doesn't get stuck when `wt` has a transient issue. Lock
/// errors DO propagate — the caller is meant to surface them to the
/// user per issue #101's "GUI cleanup defaults must be more
/// conservative than terminal cleanup" principle.
pub fn remove_worktree_with_provider(
    repo_path: &str,
    worktree_path: &str,
    also_branch: bool,
    provider: WorktreeProvider,
    wt: Option<&roux_worktrunk::WtBinary>,
) -> Result<(), WorktreeError> {
    let use_wt = match provider {
        WorktreeProvider::Git => false,
        WorktreeProvider::Worktrunk | WorktreeProvider::Auto => wt.is_some(),
    };

    if use_wt {
        if let Some(wt) = wt {
            let opts = roux_worktrunk::RemoveOpts {
                also_branch,
                force: false,
                env: Vec::new(),
            };
            match roux_worktrunk::remove_worktree(
                wt,
                Path::new(repo_path),
                Path::new(worktree_path),
                &opts,
            ) {
                Ok(()) => return Ok(()),
                Err(roux_worktrunk::WtError::Locked { reason }) => {
                    // Locks are user-visible: do NOT silently fall back.
                    return Err(WorktreeError::WorktrunkLocked { reason });
                }
                Err(err) => {
                    eprintln!(
                        "roux-worktrunk: remove failed ({err}); falling back to native git for {worktree_path}"
                    );
                    // Fall through to native.
                }
            }
        }
    }

    // Capture the branch name BEFORE removing the worktree — once the
    // worktree directory is gone, `rev-parse` has nothing to resolve
    // against.
    let branch = if also_branch { resolve_worktree_branch(worktree_path) } else { None };

    remove_worktree(worktree_path)?;

    if let Some(branch) = branch {
        // Best-effort: if wt's fallback path already deleted the branch,
        // `git branch -D` exits non-zero — that's fine.
        let _ = Command::new("git")
            .args(["branch", "-D", &branch])
            .current_dir(repo_path)
            .output();
    }

    Ok(())
}

/// Resolve the branch checked out in a worktree, skipping detached-HEAD
/// states. Returns `None` when the worktree is detached, unreadable, or
/// already gone.
fn resolve_worktree_branch(worktree_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

pub fn remove_worktree(worktree_path: &str) -> Result<(), WorktreeError> {
    let output = Command::new("git")
        .args(["worktree", "remove", worktree_path, "--force"])
        .output()
        .map_err(|source| WorktreeError::RunGit { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorktreeError::RemoveFailed { stderr: stderr.to_string() });
    }

    Ok(())
}

/// List worktrees, enriching each entry with metadata from
/// `wt list --format=json` when `wt` is supplied. Falls back to the
/// native porcelain path on any `wt` error, so users without a
/// functioning `wt` see no regression.
///
/// When `wt` is `None`, this is identical to [`list_worktrees`] plus
/// a `worktrunk: None` field on every entry.
pub fn list_worktrees_enriched(
    repo_path: &str,
    wt: Option<&roux_worktrunk::WtBinary>,
) -> Result<Vec<Worktree>, WorktreeError> {
    let Some(wt) = wt else {
        return list_worktrees(repo_path);
    };

    match roux_worktrunk::list_worktrees(wt, Path::new(repo_path)) {
        Ok(items) => Ok(items.into_iter().filter_map(wt_item_to_worktree).collect()),
        Err(err) => {
            eprintln!(
                "roux-worktrunk: list failed ({err}); falling back to native git for {repo_path}"
            );
            list_worktrees(repo_path)
        }
    }
}

fn wt_item_to_worktree(item: roux_worktrunk::WtItem) -> Option<Worktree> {
    // Entries without a path are branch-only (no worktree on disk). Skip
    // them to match the native porcelain behavior which only reports
    // materialised worktrees.
    let path = item.path.as_ref()?.to_string_lossy().into_owned();
    let branch = item.branch.clone().unwrap_or_else(|| "HEAD".to_string());
    let ci_status = item.ci.as_ref().map(|c| c.status.clone());
    let ci_url = item.ci.as_ref().and_then(|c| c.url.clone());
    let ci_stale = item.ci.as_ref().map(|c| c.stale).unwrap_or(false);
    let metadata = WorktrunkMetadata {
        dirty: item.is_dirty(),
        ahead: item.ahead(),
        behind: item.behind(),
        locked: item.is_locked(),
        lock_reason: item.lock_reason().map(String::from),
        prunable: item.is_prunable(),
        prunable_reason: item.prunable_reason().map(String::from),
        is_current: item.is_current,
        is_previous: item.is_previous,
        dev_server_url: item.url.clone(),
        main_state: item.main_state.clone(),
        ci_status,
        ci_url,
        ci_stale,
    };
    Some(Worktree {
        path,
        branch,
        is_main: item.is_main,
        worktrunk: Some(metadata),
    })
}

pub fn list_worktrees(repo_path: &str) -> Result<Vec<Worktree>, WorktreeError> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .map_err(|source| WorktreeError::RunGit { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorktreeError::ListFailed { stderr: stderr.to_string() });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_porcelain(&stdout))
}

fn parse_porcelain(stdout: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;
    let mut is_bare = false;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
            current_branch = None;
            is_bare = false;
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            // branch refs/heads/main -> main
            current_branch =
                Some(branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref).to_string());
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() {
            if let Some(path) = current_path.take() {
                if !is_bare {
                    let branch = current_branch.take().unwrap_or_else(|| "HEAD".to_string());
                    let is_main = worktrees.is_empty(); // first entry is main worktree
                    worktrees.push(Worktree { path, branch, is_main, worktrunk: None });
                }
            }
            current_branch = None;
            is_bare = false;
        }
    }

    // Handle last entry (no trailing blank line)
    if let Some(path) = current_path {
        if !is_bare {
            let branch = current_branch.unwrap_or_else(|| "HEAD".to_string());
            let is_main = worktrees.is_empty();
            worktrees.push(Worktree { path, branch, is_main, worktrunk: None });
        }
    }

    worktrees
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_sanitize_simple_branch() {
        assert_eq!(sanitize_branch_for_path("main"), "main");
    }

    #[test]
    fn test_sanitize_slashes_to_dashes() {
        assert_eq!(sanitize_branch_for_path("feature/auth"), "feature-auth");
    }

    #[test]
    fn test_sanitize_nested_slashes() {
        assert_eq!(sanitize_branch_for_path("feature/auth/oauth2"), "feature-auth-oauth2");
    }

    #[test]
    fn test_sanitize_strips_invalid_chars() {
        assert_eq!(sanitize_branch_for_path("fix@bug#1"), "fixbug1");
    }

    #[test]
    fn test_sanitize_preserves_dots_underscores() {
        assert_eq!(sanitize_branch_for_path("v1.0_release"), "v1.0_release");
    }

    #[test]
    fn test_repo_name_from_path() {
        assert_eq!(repo_name("/Users/dev/src/my-project"), "my-project");
    }

    #[test]
    fn test_repo_name_trailing_slash() {
        // PathBuf handles this
        assert_eq!(repo_name("/Users/dev/src/my-project"), "my-project");
    }

    #[test]
    fn test_resolve_worktree_path_with_base() {
        let path = resolve_worktree_path("/home/dev/repo", "feature/auth", Some("/tmp/worktrees"));
        let expected = PathBuf::from("/tmp/worktrees/repo-feature-auth");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_resolve_worktree_path_without_base() {
        let path = resolve_worktree_path("/home/dev/repo", "main", None);
        // Should be adjacent to repo: /home/dev/repo-main
        let expected = PathBuf::from("/home/dev/repo-main");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_resolve_worktree_path_tilde() {
        let path = resolve_worktree_path("/tmp/repo", "main", Some("~/worktrees"));
        let home = dirs::home_dir().unwrap();
        let expected = home.join("worktrees").join("repo-main");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_expand_base_template_project_name() {
        let p = expand_base_template("/tmp/{project_name}/.worktrees", "/Users/dev/my-project");
        assert_eq!(p, PathBuf::from("/tmp/my-project/.worktrees"));
    }

    #[test]
    fn test_expand_base_template_project_dir() {
        let p = expand_base_template("{project_dir}/.worktrees", "/Users/dev/my-project");
        assert_eq!(p, PathBuf::from("/Users/dev/my-project/.worktrees"));
    }

    #[test]
    fn test_expand_base_template_home() {
        let home = dirs::home_dir().unwrap();
        let p = expand_base_template("{home}/worktrees/{project_name}", "/tmp/repo");
        assert_eq!(p, home.join("worktrees").join("repo"));
    }

    #[test]
    fn test_expand_base_template_tilde_not_in_middle() {
        let p = expand_base_template("/tmp/~foo", "/tmp/repo");
        assert_eq!(p, PathBuf::from("/tmp/~foo"));
    }

    #[test]
    fn test_preview_empty_template_uses_parent() {
        let p = preview_worktree_base("", "/Users/dev/repo");
        assert_eq!(p, "/Users/dev");
    }

    #[test]
    fn test_resolve_worktree_path_with_template_base() {
        let path = resolve_worktree_path(
            "/Users/dev/repo",
            "feature/auth",
            Some("{project_dir}/.worktrees"),
        );
        assert_eq!(path, PathBuf::from("/Users/dev/repo/.worktrees/repo-feature-auth"));
    }

    #[test]
    fn test_resolve_worktree_path_bare_tilde() {
        let path = resolve_worktree_path("/tmp/repo", "main", Some("~"));
        let home = dirs::home_dir().unwrap();
        let expected = home.join("repo-main");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_parse_porcelain_single_worktree() {
        // Simulate parsing porcelain output directly
        let porcelain = "worktree /home/dev/repo\nbranch refs/heads/main\n\n";
        let worktrees = parse_porcelain(porcelain);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].path, "/home/dev/repo");
        assert_eq!(worktrees[0].branch, "main");
        assert!(worktrees[0].is_main);
    }

    #[test]
    fn test_parse_porcelain_multiple_worktrees() {
        let porcelain = "worktree /home/dev/repo\nbranch refs/heads/main\n\nworktree /tmp/repo-feature\nbranch refs/heads/feature\n\n";
        let worktrees = parse_porcelain(porcelain);
        assert_eq!(worktrees.len(), 2);
        assert!(worktrees[0].is_main);
        assert!(!worktrees[1].is_main);
        assert_eq!(worktrees[1].branch, "feature");
    }

    #[test]
    fn test_parse_porcelain_bare_repo() {
        let porcelain =
            "worktree /home/dev/repo.git\nbare\n\nworktree /tmp/wt\nbranch refs/heads/main\n\n";
        let worktrees = parse_porcelain(porcelain);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].path, "/tmp/wt");
    }

    #[test]
    fn test_parse_porcelain_no_trailing_newline() {
        let porcelain = "worktree /home/dev/repo\nbranch refs/heads/main";
        let worktrees = parse_porcelain(porcelain);
        assert_eq!(worktrees.len(), 1);
    }

    #[test]
    fn worktree_error_display_keeps_existing_messages() {
        let error = WorktreeError::RunGit { source: io::Error::other("boom") };

        assert_eq!(error.to_string(), "Failed to run git: boom");
    }

    // ---- Integration-style tests against a real temp git repo ----
    //
    // Git invocations here deliberately ignore the host's global and
    // system config. Without this, dev machines that sign commits via
    // SSH agents (e.g. 1Password) flake under the parallel test load
    // because the agent can't keep up with simultaneous signing
    // requests. Test commits need neither signing nor credential
    // helpers; dropping global config makes the tests hermetic.

    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("failed to invoke git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn git_stdout(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("failed to invoke git");
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Initialise a temp git repo with a `main` branch and one commit.
    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "t@t.test"]);
        git(dir, &["config", "user.name", "Test"]);
        git(dir, &["commit", "--allow-empty", "-m", "init"]);
    }

    #[test]
    fn create_worktree_with_start_point_branches_from_it() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        // Create a second branch with an extra commit; leave HEAD on it so
        // HEAD != main. If start_point worked, the new worktree should point
        // at main's tip, not HEAD.
        git(&repo, &["checkout", "-q", "-b", "other"]);
        git(&repo, &["commit", "--allow-empty", "-m", "other-c1"]);
        let main_tip = git_stdout(&repo, &["rev-parse", "main"]);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        let wt_path =
            create_worktree(&repo_str, "feature-from-main", Some(&base_str), Some("main"))
                .expect("create_worktree should succeed");

        let wt_head = git_stdout(Path::new(&wt_path), &["rev-parse", "HEAD"]);
        assert_eq!(wt_head, main_tip, "worktree HEAD should match main's tip");
    }

    #[test]
    fn create_worktree_ignores_start_point_when_branch_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        git(&repo, &["checkout", "-q", "-b", "feature"]);
        git(&repo, &["commit", "--allow-empty", "-m", "feature-c1"]);
        let feature_tip = git_stdout(&repo, &["rev-parse", "feature"]);
        git(&repo, &["checkout", "-q", "main"]);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        // Even though start_point=main, since `feature` exists, we check it
        // out and land on feature's tip.
        let wt_path = create_worktree(&repo_str, "feature", Some(&base_str), Some("main"))
            .expect("create_worktree should succeed");

        let wt_head = git_stdout(Path::new(&wt_path), &["rev-parse", "HEAD"]);
        assert_eq!(wt_head, feature_tip);
    }

    #[test]
    fn create_worktree_invalid_start_point_returns_typed_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        let err = create_worktree(
            &repo_str,
            "will-not-create",
            Some(&base_str),
            Some("definitely-not-a-ref"),
        )
        .expect_err("should fail with InvalidStartPoint");

        match err {
            WorktreeError::InvalidStartPoint { start_point } => {
                assert_eq!(start_point, "definitely-not-a-ref");
            }
            other => panic!("expected InvalidStartPoint, got {:?}", other),
        }
    }

    #[test]
    fn create_worktree_start_point_accepts_annotated_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        // Annotated tags are a separate object type but peel to a commit via
        // `^{commit}`, so they should be accepted as a start point.
        git(&repo, &["tag", "-a", "v1", "-m", "first tag"]);
        let tag_commit = git_stdout(&repo, &["rev-parse", "v1^{commit}"]);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        let wt_path = create_worktree(&repo_str, "feature-from-tag", Some(&base_str), Some("v1"))
            .expect("create_worktree should accept annotated tag as start point");

        let wt_head = git_stdout(Path::new(&wt_path), &["rev-parse", "HEAD"]);
        assert_eq!(wt_head, tag_commit);
    }

    fn broken_wt_binary() -> roux_worktrunk::WtBinary {
        // Path points at a binary that definitely doesn't exist, so any
        // actual shell-out will fail. Used to prove Git-provider and
        // Auto+fallback behavior in a deterministic way.
        roux_worktrunk::WtBinary {
            path: PathBuf::from("/this/path/definitely/does/not/exist/wt"),
            version: semver::Version::parse("99.0.0").unwrap(),
        }
    }

    #[test]
    fn create_with_provider_git_ignores_wt_binary_entirely() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        let broken = broken_wt_binary();
        let path = create_worktree_with_provider(
            &repo_str,
            "feat-git-only",
            Some(&base_str),
            None,
            WorktreeProvider::Git,
            Some(&broken),
        )
        .expect("Git provider must succeed and ignore wt binary");

        assert!(Path::new(&path).is_dir());
        // Verify the path is under our explicit base (proves git path was used,
        // not wt's default layout).
        assert!(
            path.starts_with(&base_str),
            "Git provider should honor base_path; got {path}"
        );
    }

    #[test]
    fn create_with_provider_auto_and_none_wt_uses_native() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        let path = create_worktree_with_provider(
            &repo_str,
            "feat-auto-no-wt",
            Some(&base_str),
            None,
            WorktreeProvider::Auto,
            None,
        )
        .expect("Auto with no wt must fall through to native git");
        assert!(Path::new(&path).is_dir());
        assert!(path.starts_with(&base_str));
    }

    #[test]
    fn create_with_provider_auto_falls_back_to_git_when_wt_spawn_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        let broken = broken_wt_binary();
        let path = create_worktree_with_provider(
            &repo_str,
            "feat-fallback",
            Some(&base_str),
            None,
            WorktreeProvider::Auto,
            Some(&broken),
        )
        .expect("Auto must fall back to native when wt spawn fails");

        assert!(Path::new(&path).is_dir());
        assert!(
            path.starts_with(&base_str),
            "fallback must land at the git base path; got {path}"
        );
    }

    #[test]
    fn create_with_provider_worktrunk_falls_back_to_git_when_wt_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();

        let broken = broken_wt_binary();
        let path = create_worktree_with_provider(
            &repo_str,
            "feat-wtrunk-fallback",
            Some(&base_str),
            None,
            WorktreeProvider::Worktrunk,
            Some(&broken),
        )
        .expect("Worktrunk provider must still fall back on wt failure");

        assert!(Path::new(&path).is_dir());
        assert!(path.starts_with(&base_str));
    }

    #[test]
    fn list_worktrees_enriched_with_none_matches_legacy_listing_plus_worktrunk_none() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let base = tmp.path().join("wts");
        std::fs::create_dir_all(&base).unwrap();
        let repo_str = repo.to_string_lossy().to_string();
        let base_str = base.to_string_lossy().to_string();
        create_worktree(&repo_str, "feature", Some(&base_str), None).expect("create");

        let legacy = list_worktrees(&repo_str).expect("legacy list");
        let enriched = list_worktrees_enriched(&repo_str, None).expect("enriched list");

        assert_eq!(legacy.len(), enriched.len());
        for (l, e) in legacy.iter().zip(enriched.iter()) {
            assert_eq!(l.path, e.path);
            assert_eq!(l.branch, e.branch);
            assert_eq!(l.is_main, e.is_main);
            assert!(
                e.worktrunk.is_none(),
                "enriched with None wt must carry worktrunk=None; got {:?}",
                e.worktrunk
            );
        }
    }
}
