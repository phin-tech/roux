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
    /// named smol VM instead of on the host. Cleared by sending
    /// `cmd_set_session_smol_machine(id, None)`. Field outlives a smolvm
    /// uninstall — spawn-time defense in `pty.rs` falls back to a clear
    /// "smolvm not installed" error rather than silently running on host.
    #[serde(default)]
    pub smol_machine_name: Option<String>,
}
