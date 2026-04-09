use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

/// Load persisted sessions from disk. All restored sessions are marked as "disconnected".
pub fn load_persisted_sessions() -> Vec<Session> {
    let path = persistence_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut sessions: Vec<Session> = serde_json::from_str(&content).unwrap_or_default();
        for s in &mut sessions {
            s.status = "disconnected".to_string();
        }
        sessions
    } else {
        Vec::new()
    }
}

pub fn persistence_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("roux").join("sessions.json")
}


