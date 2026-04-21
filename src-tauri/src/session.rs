use std::fs;
use std::path::PathBuf;

pub use roux_core::Session;

/// Load persisted sessions from disk. Active sessions are marked "disconnected"
/// (their PTYs are dead after an app restart); archived sessions preserve
/// whatever status was on disk — the UI ignores `status` for archived rows
/// and renders "Closed Xh ago" instead.
pub fn load_persisted_sessions() -> Vec<Session> {
    let path = persistence_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut sessions: Vec<Session> = serde_json::from_str(&content).unwrap_or_default();
        for s in &mut sessions {
            if !s.archived {
                s.status = roux_core::SessionStatus::Disconnected;
            }
        }
        sessions
    } else {
        Vec::new()
    }
}

pub fn persistence_path() -> PathBuf {
    crate::paths::roux_config_dir().join("sessions.json")
}
