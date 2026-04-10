use serde::Serialize;

/// Payload emitted when a PTY session exits (event: `session-exit:{id}`).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionExitPayload {
    pub code: Option<u32>,
    pub generation: u64,
    pub reason: SessionExitReason,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SessionExitReason {
    Exit,
    IoError,
    Killed,
}

/// Payload emitted for socket/CLI-initiated commands (event: `roux-command`).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouxCommand {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pty_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

impl RouxCommand {
    pub fn new(action: &str) -> Self {
        Self {
            action: action.to_string(),
            session_id: None,
            pane_id: None,
            pty_id: None,
            direction: None,
            command: None,
            working_dir: None,
        }
    }

    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn pane_id(mut self, id: impl Into<String>) -> Self {
        self.pane_id = Some(id.into());
        self
    }

    pub fn pty_id(mut self, id: impl Into<String>) -> Self {
        self.pty_id = Some(id.into());
        self
    }

    pub fn direction(mut self, dir: impl Into<String>) -> Self {
        self.direction = Some(dir.into());
        self
    }

    pub fn command(mut self, cmd: impl Into<String>) -> Self {
        self.command = Some(cmd.into());
        self
    }

    pub fn working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }
}
