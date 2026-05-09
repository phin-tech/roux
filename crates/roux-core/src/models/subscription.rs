use serde::{Deserialize, Serialize};

/// A persistent subscription that ties an alias to a topic glob pattern.
/// When `bus publish` fires an event whose topic matches `pattern`, the
/// subscriber alias receives the event in its mailbox (subject to project
/// scope) and a `MailboxEvent::TopicDelivered` is emitted for the UI.
///
/// Patterns are MQTT-style globs validated by `topic_glob::validate_topic_pattern`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BusSubscription {
    /// Stable id (uuid v4). Used to delete a subscription without
    /// guessing on alias+pattern equality.
    pub id: String,
    /// Canonical alias receiving deliveries. Need not be currently bound
    /// to a pane — queued mail accrues for whoever claims it next.
    pub alias: String,
    /// Validated glob pattern. Empty / invalid patterns are rejected at
    /// the manager boundary.
    pub pattern: String,
    /// Project scope. `None` = global. A subscription only matches
    /// events whose `project_id` is `None` or equals this scope.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Unix epoch milliseconds.
    pub created_at: u64,
}

/// Tauri event emitted on subscription mutation. Mirrors `AliasEvent`
/// shape so the frontend can keep its subscriptions store in sync.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BusSubscriptionEvent {
    Created { subscription: BusSubscription },
    Removed { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> BusSubscription {
        BusSubscription {
            id: "sub-1".into(),
            alias: "auditor".into(),
            pattern: "*.build.completed".into(),
            project_id: Some("repo-a".into()),
            created_at: 12345,
        }
    }

    #[test]
    fn serde_round_trip_preserves_all_fields() {
        let original = fixture();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: BusSubscription = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn project_id_serializes_camel_case() {
        let json = serde_json::to_value(fixture()).unwrap();
        assert_eq!(json["projectId"], "repo-a");
        assert_eq!(json["createdAt"], 12345);
    }

    #[test]
    fn missing_project_id_defaults_to_none_on_load() {
        // Older / hand-rolled rows might omit projectId entirely.
        let raw = r#"{"id":"sub-1","alias":"a","pattern":"*","createdAt":1}"#;
        let parsed: BusSubscription = serde_json::from_str(raw).unwrap();
        assert!(parsed.project_id.is_none());
    }

    #[test]
    fn event_serializes_with_kind_tag() {
        let evt = BusSubscriptionEvent::Created { subscription: fixture() };
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["kind"], "created");
        assert!(json["subscription"].is_object());
    }

    #[test]
    fn removed_event_carries_id() {
        let evt = BusSubscriptionEvent::Removed { id: "sub-1".into() };
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["kind"], "removed");
        assert_eq!(json["id"], "sub-1");
    }
}
