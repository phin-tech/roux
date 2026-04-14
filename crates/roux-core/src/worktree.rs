use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

use crate::models::Worktree;

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

pub fn create_worktree(
    repo_path: &str,
    branch: &str,
    base_path: Option<&str>,
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
                    worktrees.push(Worktree { path, branch, is_main });
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
            worktrees.push(Worktree { path, branch, is_main });
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
}
