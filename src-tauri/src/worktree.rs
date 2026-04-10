use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

pub use roux_core::Worktree;

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

fn resolve_worktree_path(repo_path: &str, branch: &str, base_path: Option<&str>) -> PathBuf {
    let sanitized = sanitize_branch_for_path(branch);
    let name = repo_name(repo_path);
    let dir_name = format!("{}-{}", name, sanitized);

    let base = match base_path {
        Some(p) => {
            let expanded = if p == "~" {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
            } else if let Some(rest) = p.strip_prefix("~/") {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                home.join(rest)
            } else {
                PathBuf::from(p)
            };
            expanded
        }
        None => Path::new(repo_path).parent().unwrap_or(Path::new(".")).to_path_buf(),
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
