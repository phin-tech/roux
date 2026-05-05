use crate::pr;
use crate::services::setup as svc;

#[tauri::command]
#[specta::specta]
pub(crate) fn check_gh_installed() -> bool {
    svc::is_gh_available()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn lookup_pr(
    repo_path: Option<String>,
    url: String,
) -> Result<pr::PrInfo, String> {
    pr::lookup_pr(repo_path.as_deref(), &url).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn fetch_pr_branch(
    repo_path: String,
    number: u32,
    head_ref: String,
    is_cross_repository: bool,
) -> Result<String, String> {
    pr::fetch_pr_branch(&repo_path, number, &head_ref, is_cross_repository)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn clone_repo(
    owner: String,
    repo: String,
    target_dir: String,
) -> Result<String, String> {
    pr::clone_repo(&owner, &repo, &target_dir).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn lookup_pr_for_branch(
    repo_path: String,
    branch: String,
) -> Result<Option<pr::PrInfo>, String> {
    pr::lookup_pr_for_branch(&repo_path, &branch).await.map_err(|e| e.to_string())
}
