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
    /// When set, a subsequent push with the same `dedup_key` updates this
    /// notification in place instead of creating a new one. Cleared when the
    /// user reads or dismisses the notification (tracked via `read`).
    #[serde(default)]
    pub dedup_key: Option<String>,
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
    /// Optional dedup key. When set, `NotificationManager::push` will update
    /// an existing unread notification carrying the same key instead of
    /// creating a new one. Useful for flood-prone sources like permission
    /// prompts that fire repeatedly for the same pane.
    #[serde(default)]
    pub dedup_key: Option<String>,
}

/// Event emitted to the frontend on store mutations.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum NotificationEvent {
    Added { notification: Notification },
    Updated { notification: Notification },
    Read { id: String },
    ReadAll { session_id: Option<String> },
    Removed { id: String },
    Cleared { session_id: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleared_event_serializes_session_id_as_camelcase() {
        // Regression: a missing `rename_all_fields = "camelCase"` once caused
        // this to emit `session_id`, and the frontend handler (which reads
        // `event.sessionId`) silently no-op'd — the clear button did nothing.
        let json =
            serde_json::to_string(&NotificationEvent::Cleared { session_id: None }).unwrap();
        assert!(json.contains("\"sessionId\""), "expected camelCase field in: {json}");
        assert!(!json.contains("\"session_id\""), "snake_case leaked in: {json}");
        assert!(json.contains("\"type\":\"cleared\""));
    }

    #[test]
    fn read_all_event_serializes_session_id_as_camelcase() {
        let json = serde_json::to_string(&NotificationEvent::ReadAll {
            session_id: Some("sess-1".into()),
        })
        .unwrap();
        assert!(json.contains("\"sessionId\":\"sess-1\""), "unexpected: {json}");
        assert!(!json.contains("\"session_id\""));
        assert!(json.contains("\"type\":\"readAll\""));
    }
}
