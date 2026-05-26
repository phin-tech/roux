use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    #[default]
    Idle,
    Thinking,
    Generating,
    Error,
    Disconnected,
    Attention,
}

impl SessionStatus {
    /// Map a raw hook status string (as written by `roux hook <status>`) to a
    /// `SessionStatus`. "working" and "generating" both map to `Generating`
    /// (the hook writer uses "working"; "generating" is the normalised form).
    /// Unknown strings map to `Idle`.
    pub fn from_hook_status(raw: &str) -> Self {
        match raw {
            "working" | "generating" => Self::Generating,
            "idle" => Self::Idle,
            "attention" => Self::Attention,
            "error" => Self::Error,
            "disconnected" => Self::Disconnected,
            _ => Self::Idle,
        }
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

/// Map a raw hook status string to its canonical string form (the same
/// normalisation used by `StatusUpdate.status` in the desktop). This
/// pure function lives in `roux-core` so both the daemon watcher and the
/// desktop `file_status` source share identical canonicalisation logic.
pub fn map_hook_status(raw: &str) -> &str {
    match raw {
        "working" | "generating" => "generating",
        "idle" => "idle",
        "attention" => "attention",
        "error" => "error",
        "disconnected" => "disconnected",
        _ => "idle",
    }
}

/// Broadcast event emitted by the session service whenever a session's
/// status actually changes (compare-before-assign — no-ops are dropped).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatusEvent {
    pub session_id: String,
    pub status: SessionStatus,
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
    #[serde(default)]
    pub name_override: Option<String>,
    /// ID of the primary PTY for this session. Set at session creation,
    /// kept as `None` for sessions restored from disk that haven't reconnected yet.
    #[serde(default)]
    pub primary_pty_id: Option<String>,
    /// Soft-delete flag. Archived sessions are retained in `sessions.json`
    /// for the history view but filtered out of the active `list_sessions`
    /// query. Restore flips this back to `false`.
    #[serde(default)]
    pub archived: bool,
    /// Unix epoch seconds of when this session was archived. `None` for
    /// active sessions; set when `archived` flips to `true`.
    #[serde(default)]
    pub ended_at: Option<u64>,
    /// Project session-blueprint id this session was spawned from. Lets the
    /// sidebar collapse the dimmed blueprint row while a live session is up
    /// and respawn it when the live session is killed.
    #[serde(default)]
    pub blueprint_id: Option<String>,
    /// User-pinned PR URL or shortform for this session. When set, the
    /// status bar uses it directly instead of running the branch-based
    /// `gh pr list --head` discovery — useful for cross-repo PRs and for
    /// cases where the local branch was renamed after the PR was opened.
    #[serde(default)]
    pub pinned_pr_url: Option<String>,
    /// When set, every PTY spawned for this session runs via
    /// `smolvm machine exec --name <smol_machine_name> ...` inside the
    /// named smol VM instead of on the host. Cleared by invoking the
    /// `set_session_smol_machine` Tauri command with a `None` machine
    /// name. Field outlives a smolvm uninstall — spawn-time defense in
    /// `pty.rs` falls back to a clear "smolvm not installed" error
    /// rather than silently running on host.
    #[serde(default)]
    pub smol_machine_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_hook_status_maps_all_known_values() {
        assert_eq!(SessionStatus::from_hook_status("working"), SessionStatus::Generating);
        assert_eq!(SessionStatus::from_hook_status("generating"), SessionStatus::Generating);
        assert_eq!(SessionStatus::from_hook_status("idle"), SessionStatus::Idle);
        assert_eq!(SessionStatus::from_hook_status("attention"), SessionStatus::Attention);
        assert_eq!(SessionStatus::from_hook_status("error"), SessionStatus::Error);
        assert_eq!(SessionStatus::from_hook_status("disconnected"), SessionStatus::Disconnected);
    }

    #[test]
    fn from_hook_status_unknown_maps_to_idle() {
        assert_eq!(SessionStatus::from_hook_status(""), SessionStatus::Idle);
        assert_eq!(SessionStatus::from_hook_status("thinking"), SessionStatus::Idle);
        assert_eq!(SessionStatus::from_hook_status("bogus"), SessionStatus::Idle);
    }

    #[test]
    fn map_hook_status_normalises_working_to_generating() {
        assert_eq!(map_hook_status("working"), "generating");
        assert_eq!(map_hook_status("idle"), "idle");
        assert_eq!(map_hook_status("attention"), "attention");
        assert_eq!(map_hook_status("error"), "error");
        assert_eq!(map_hook_status("disconnected"), "disconnected");
    }

    #[test]
    fn map_hook_status_unknown_maps_to_idle() {
        assert_eq!(map_hook_status("unknown"), "idle");
        assert_eq!(map_hook_status("generating"), "generating");
    }

    #[test]
    fn session_status_event_round_trips_json() {
        let event =
            SessionStatusEvent { session_id: "s-1".to_string(), status: SessionStatus::Generating };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: SessionStatusEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.session_id, "s-1");
        assert_eq!(decoded.status, SessionStatus::Generating);
    }
}
