use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_main: bool,
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

fn resolve_worktree_path(
    repo_path: &str,
    branch: &str,
    base_path: Option<&str>,
) -> PathBuf {
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
) -> Result<String, String> {
    let target = resolve_worktree_path(repo_path, branch, base_path);
    let target_str = target.to_string_lossy().to_string();

    let output = if branch_exists(repo_path, branch) {
        Command::new("git")
            .args(["worktree", "add", &target_str, branch])
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?
    } else {
        Command::new("git")
            .args(["worktree", "add", "-b", branch, &target_str])
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {}", stderr));
    }

    Ok(target_str)
}

pub fn remove_worktree(worktree_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["worktree", "remove", worktree_path, "--force"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree remove failed: {}", stderr));
    }

    Ok(())
}

pub fn list_worktrees(repo_path: &str) -> Result<Vec<Worktree>, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree list failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
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
            current_branch = Some(
                branch_ref
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch_ref)
                    .to_string(),
            );
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() {
            if let Some(path) = current_path.take() {
                if !is_bare {
                    let branch = current_branch
                        .take()
                        .unwrap_or_else(|| "HEAD".to_string());
                    let is_main = worktrees.is_empty(); // first entry is main worktree
                    worktrees.push(Worktree {
                        path,
                        branch,
                        is_main,
                    });
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
            worktrees.push(Worktree {
                path,
                branch,
                is_main,
            });
        }
    }

    Ok(worktrees)
}
