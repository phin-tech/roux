use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use roux_core::Project;

pub use roux_core::Session;

/// Load persisted sessions from disk. Active sessions are marked "disconnected"
/// (their PTYs are dead after an app restart); archived sessions preserve
/// whatever status was on disk — the UI ignores `status` for archived rows
/// and renders "Closed Xh ago" instead.
pub fn load_persisted_sessions(projects: &[Project]) -> Vec<Session> {
    let path = persistence_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut sessions: Vec<Session> = serde_json::from_str(&content).unwrap_or_default();
        for s in &mut sessions {
            if !s.archived {
                s.status = roux_core::SessionStatus::Disconnected;
            }
        }
        if clear_stale_project_refs(&mut sessions, projects) {
            write_to_path(&sessions, &path);
        }
        sessions
    } else {
        Vec::new()
    }
}

pub fn clear_stale_project_refs(sessions: &mut [Session], projects: &[Project]) -> bool {
    let project_ids: HashSet<&str> = projects.iter().map(|p| p.id.as_str()).collect();
    let mut changed = false;
    for session in sessions {
        if let Some(project_id) = session.project_id.as_deref() {
            if !project_ids.contains(project_id) {
                session.project_id = None;
                session.blueprint_id = None;
                changed = true;
            }
        }
    }
    changed
}

pub fn persistence_path() -> PathBuf {
    crate::paths::roux_config_dir().join("sessions.json")
}

fn write_to_path(sessions: &[Session], path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(sessions) {
        let _ = fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project(id: &str) -> Project {
        Project {
            id: id.to_string(),
            name: id.to_string(),
            repo_roots: Vec::new(),
            context_paths: Vec::new(),
            session_blueprints: Vec::new(),
            project_prompt: String::new(),
        }
    }

    fn make_session(id: &str, project_id: Option<&str>) -> Session {
        Session {
            id: id.to_string(),
            name: id.to_string(),
            repo_root: "/tmp/repo".to_string(),
            worktree_path: "/tmp/repo".to_string(),
            branch: "main".to_string(),
            is_worktree: false,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: project_id.map(str::to_string),
            is_git_repo: false,
            name_override: None,
            primary_pty_id: None,
            archived: false,
            ended_at: None,
            blueprint_id: project_id.map(|_| "bp-1".to_string()),
            pinned_pr_url: None,
            smol_machine_name: None,
        }
    }

    #[test]
    fn clear_stale_project_refs_removes_unknown_project_refs() {
        let mut sessions = vec![
            make_session("kept", Some("proj-1")),
            make_session("cleared", Some("missing")),
            make_session("untagged", None),
        ];

        let changed = clear_stale_project_refs(&mut sessions, &[make_project("proj-1")]);

        assert!(changed);
        assert_eq!(sessions[0].project_id.as_deref(), Some("proj-1"));
        assert_eq!(sessions[0].blueprint_id.as_deref(), Some("bp-1"));
        assert!(sessions[1].project_id.is_none());
        assert!(sessions[1].blueprint_id.is_none());
        assert!(sessions[2].project_id.is_none());
    }

    #[test]
    fn clear_stale_project_refs_reports_no_change_when_refs_are_valid() {
        let mut sessions = vec![make_session("kept", Some("proj-1"))];

        let changed = clear_stale_project_refs(&mut sessions, &[make_project("proj-1")]);

        assert!(!changed);
        assert_eq!(sessions[0].project_id.as_deref(), Some("proj-1"));
        assert_eq!(sessions[0].blueprint_id.as_deref(), Some("bp-1"));
    }
}
