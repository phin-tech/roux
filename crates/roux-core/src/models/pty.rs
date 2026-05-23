/// Role of a PTY within its session.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PtyRole {
    /// Main Claude/shell for the session.
    SessionPrimary,
    /// Additional shells, e.g. spawned from a split pane.
    Secondary,
}

/// Lifecycle status of a PTY.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type")]
pub enum PtyStatus {
    /// PTY is running and attached to a pane.
    RunningAttached { pane_id: String },
    /// PTY is running but not currently attached to any pane.
    RunningDetached { since_ms: u64 },
    /// PTY process has exited.
    Exited { code: Option<i32>, at_ms: u64 },
}

/// Serializable PTY snapshot for frontend consumption.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PtyInfo {
    pub id: String,
    pub session_id: Option<String>,
    pub role: PtyRole,
    pub status: PtyStatus,
    pub name: Option<String>,
    pub working_dir: Option<String>,
    pub profile: Option<String>,
    pub unread_output: bool,
    pub bell_pending: bool,
}
