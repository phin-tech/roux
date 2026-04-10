use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Idle,
    Thinking,
    Generating,
    Error,
    Disconnected,
    Attention,
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Idle
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Thinking => write!(f, "thinking"),
            Self::Generating => write!(f, "generating"),
            Self::Error => write!(f, "error"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::Attention => write!(f, "attention"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    pub repo_root: String,
    pub worktree_path: String,
    pub branch: String,
    pub is_worktree: bool,
    pub status: SessionStatus,
    pub model: Option<String>,
    pub cost: Option<f64>,
    pub created_at: u64,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub is_git_repo: bool,
}
