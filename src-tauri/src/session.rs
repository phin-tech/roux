use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

pub struct SessionStore {
    sessions: Mutex<Vec<Session>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
        }
    }

    pub fn load_persisted() -> Self {
        let path = Self::persistence_path();
        let sessions = if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let mut sessions: Vec<Session> =
                serde_json::from_str(&content).unwrap_or_default();
            // Mark all restored sessions as disconnected
            for s in &mut sessions {
                s.status = "disconnected".to_string();
            }
            sessions
        } else {
            Vec::new()
        };
        Self {
            sessions: Mutex::new(sessions),
        }
    }

    pub fn add(&self, session: Session) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.push(session);
        Self::persist(&sessions);
    }

    pub fn remove(&self, id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|s| s.id != id);
        Self::persist(&sessions);
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions.lock().unwrap().clone()
    }

    pub fn update_status(&self, id: &str, status: &str, model: Option<String>, cost: Option<f64>) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
            session.status = status.to_string();
            if let Some(m) = model {
                session.model = Some(m);
            }
            if let Some(c) = cost {
                session.cost = Some(c);
            }
        }
        Self::persist(&sessions);
    }

    fn persistence_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("roux").join("sessions.json")
    }

    fn persist(sessions: &[Session]) {
        let path = Self::persistence_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(sessions) {
            let _ = fs::write(&path, json);
        }
    }
}
