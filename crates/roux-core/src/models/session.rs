use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    pub repo_root: String,
    pub worktree_path: String,
    pub branch: String,
    pub is_worktree: bool,
    pub status: String,
    pub model: Option<String>,
    pub cost: Option<f64>,
    pub created_at: u64,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub is_git_repo: bool,
}
