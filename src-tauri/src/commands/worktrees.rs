use crate::services::worktrees as svc;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
pub(crate) fn cmd_remove_worktree(worktree_path: String) -> Result<(), String> {
    crate::worktree::remove_worktree(&worktree_path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_list_worktrees(
    repo_path: String,
) -> Result<Vec<crate::worktree::Worktree>, String> {
    crate::worktree::list_worktrees(&repo_path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_list_branches(repo_path: String) -> Result<Vec<String>, String> {
    svc::list_branches(&repo_path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn git_init(path: String) -> Result<(), String> {
    svc::git_init(&path).map_err(|e| e.to_string())
}
