use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
    sessions: Arc<Mutex<Vec<Session>>>,
    dirty: Arc<AtomicBool>,
}

impl SessionStore {
    pub fn load_persisted() -> Self {
        let path = Self::persistence_path();
        let sessions = if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let mut sessions: Vec<Session> = serde_json::from_str(&content).unwrap_or_default();
            // Mark all restored sessions as disconnected
            for s in &mut sessions {
                s.status = "disconnected".to_string();
            }
            sessions
        } else {
            Vec::new()
        };
        let store = Self {
            sessions: Arc::new(Mutex::new(sessions)),
            dirty: Arc::new(AtomicBool::new(true)),
        };
        store.start_persist_thread();
        store
    }

    /// Background thread that writes to disk at most every 500ms when dirty.
    fn start_persist_thread(&self) {
        let sessions = Arc::clone(&self.sessions);
        let dirty = Arc::clone(&self.dirty);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            if dirty.swap(false, Ordering::AcqRel) {
                let snapshot = {
                    let guard = sessions.lock().unwrap();
                    guard.clone()
                };
                Self::write_to_disk(&snapshot);
            }
        });
    }

    pub fn add(&self, session: Session) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.push(session);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn remove(&self, id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|s| s.id != id);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.lock().unwrap().iter().find(|s| s.id == id).cloned()
    }

    pub fn update_status(&self, id: &str, status: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
            s.status = status.to_string();
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions.lock().unwrap().clone()
    }

    fn persistence_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("roux").join("sessions.json")
    }

    fn write_to_disk(sessions: &[Session]) {
        let path = Self::persistence_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(sessions) {
            let _ = fs::write(&path, json);
        }
    }
}
