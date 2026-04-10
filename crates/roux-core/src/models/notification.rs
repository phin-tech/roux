use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub created_at: u64,
    pub level: NotificationLevel,
    pub source: NotificationSource,
    pub title: String,
    pub subtitle: Option<String>,
    pub body: Option<String>,
    pub session_id: Option<String>,
    pub read: bool,
    pub actions: Vec<NotificationAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NotificationLevel {
    Info,
    Success,
    Attention,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum NotificationSource {
    Hook {
        provider: String,
    },
    Watch {
        #[serde(rename = "watchId")]
        watch_id: String,
    },
    Task {
        #[serde(rename = "paneId")]
        pane_id: String,
    },
    Cli,
    Osc {
        code: u16,
        #[serde(rename = "senderId")]
        sender_id: Option<String>,
    },
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationAction {
    pub id: String,
    pub label: String,
    pub kind: ActionKind,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ActionKind {
    FocusSession {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    FocusPane {
        #[serde(rename = "paneId")]
        pane_id: String,
    },
    OpenUrl {
        url: String,
    },
    OpenPath {
        path: String,
    },
    RunCommand {
        #[serde(rename = "commandId")]
        command_id: String,
    },
    RetryWatch {
        #[serde(rename = "watchId")]
        watch_id: String,
    },
    Dismiss,
    DismissSource,
    MarkRead,
}

/// Write-side struct used by ingress paths. The store fills in id, created_at, and read=false.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRequest {
    pub level: NotificationLevel,
    pub source: NotificationSource,
    pub title: String,
    pub subtitle: Option<String>,
    pub body: Option<String>,
    pub session_id: Option<String>,
    pub actions: Vec<NotificationAction>,
}

/// Event emitted to the frontend on store mutations.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum NotificationEvent {
    Added { notification: Notification },
    Updated { notification: Notification },
    Read { id: String },
    ReadAll { session_id: Option<String> },
    Removed { id: String },
    Cleared { session_id: Option<String> },
}
