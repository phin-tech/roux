use serde::{Deserialize, Serialize};

/// Board column / workflow position of a work item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemStatus {
    #[default]
    Todo,
    Doing,
    Review,
    Done,
}

impl WorkItemStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Review => "review",
            Self::Done => "done",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(Self::Todo),
            "doing" => Some(Self::Doing),
            "review" => Some(Self::Review),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

impl std::fmt::Display for WorkItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Pointer to an item's identity in an external system (e.g. a future
/// GitHub or Linear adapter). Only `provider` + `external_id` together
/// form the dedup key; `url` is informational.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRef {
    pub provider: String,
    pub external_id: String,
    pub url: Option<String>,
}

/// A durable unit of intended work. Cards outlive the sessions that run
/// them — a card is born with no session (`Todo`) and session binding is
/// set by the explicit `work-item-dispatch` action.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub id: String,
    pub project_id: Option<String>,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub status: WorkItemStatus,
    /// Bound agent session — set when `work-item-dispatch` fires.
    pub session_id: Option<String>,
    /// External system identity fields (provider, external_id, external_url)
    /// are de-normalised onto the item so the frontend never needs to
    /// join with a separate refs table.
    pub provider: Option<String>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub sort_order: f64,
    pub pinned_pr_url: Option<String>,
    /// Reserved for future cost-capture; never read in v1.
    pub cost: Option<f64>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Input shape for creating / importing a work item. All fields except
/// `title` are optional; the store fills defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemInput {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub status: Option<WorkItemStatus>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub external_ref: Option<ExternalRef>,
    #[serde(default)]
    pub sort_order: Option<f64>,
}

/// Broadcast event emitted after every successful mutation.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkItemEvent {
    Created { item: WorkItem },
    Updated { item: WorkItem },
    Moved { id: String, status: WorkItemStatus, sort_order: f64 },
    Deleted { id: String },
    Imported { ids: Vec<String> },
    SessionBound { id: String, session_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_status_round_trips() {
        for (s, expected) in [
            ("todo", WorkItemStatus::Todo),
            ("doing", WorkItemStatus::Doing),
            ("review", WorkItemStatus::Review),
            ("done", WorkItemStatus::Done),
        ] {
            let status = WorkItemStatus::from_str_opt(s).unwrap();
            assert_eq!(status, expected);
            assert_eq!(status.as_str(), s);
        }
    }

    #[test]
    fn work_item_status_unknown_returns_none() {
        assert!(WorkItemStatus::from_str_opt("backlog").is_none());
        assert!(WorkItemStatus::from_str_opt("").is_none());
    }

    #[test]
    fn work_item_event_serialises_as_tagged_camel_case() {
        let ev = WorkItemEvent::Deleted { id: "i-1".into() };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"deleted\""), "got: {json}");
    }
}
