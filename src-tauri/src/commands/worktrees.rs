use crate::state::AppState;

#[tauri::command]
pub(crate) fn cmd_create_worktree(
    repo_path: String,
    branch: String,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap();
    let base_path = settings.worktree_base_path.as_deref();
    crate::worktree::create_worktree(&repo_path, &branch, base_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cmd_remove_worktree(worktree_path: String) -> Result<(), String> {
    crate::worktree::remove_worktree(&worktree_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cmd_list_worktrees(repo_path: String) -> Result<Vec<crate::worktree::Worktree>, String> {
    crate::worktree::list_worktrees(&repo_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cmd_list_branches(repo_path: String) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to list branches: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(branches)
}

#[tauri::command]
pub(crate) fn git_init(path: String) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&path)
        .output()
        .map_err(|e| format!("Failed to run git init: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
