use crate::pr;
use crate::services::setup as svc;

#[tauri::command]
#[specta::specta]
pub(crate) fn check_gh_installed() -> bool {
    svc::is_command_available("gh")
}

#[tauri::command]
#[specta::specta]
pub(crate) fn lookup_pr(repo_path: Option<String>, url: String) -> Result<pr::PrInfo, String> {
    pr::lookup_pr(repo_path.as_deref(), &url).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn fetch_pr_branch(
    repo_path: String,
    number: u32,
    head_ref: String,
    is_cross_repository: bool,
) -> Result<String, String> {
    pr::fetch_pr_branch(&repo_path, number, &head_ref, is_cross_repository)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn clone_repo(
    owner: String,
    repo: String,
    target_dir: String,
) -> Result<String, String> {
    pr::clone_repo(&owner, &repo, &target_dir).map_err(|e| e.to_string())
}
