use serde::de;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::session::Session;

/// Board column / workflow position of a work item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemStatus {
    #[default]
    Todo,
    Ready,
    Doing,
    Review,
    Done,
}

impl WorkItemStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Todo => "todo",
            Self::Ready => "ready",
            Self::Doing => "doing",
            Self::Review => "review",
            Self::Done => "done",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(Self::Todo),
            "ready" => Some(Self::Ready),
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
/// set by daemon-owned `work-item-start` after prompt dispatch succeeds.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub id: String,
    pub project_id: Option<String>,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub status: WorkItemStatus,
    /// Repo to use when starting the card. If unset, the daemon derives it from
    /// the attached project.
    pub repo_path: Option<String>,
    /// Autonomous agent profile used by daemon-owned Start.
    pub agent_profile: Option<String>,
    /// Base ref for the card's dedicated implementation worktree.
    pub base_branch: Option<String>,
    /// Dedicated implementation worktree path. Set by daemon Start and reused
    /// by retries/restarts unless the user explicitly chooses a fresh start.
    pub worktree_path: Option<String>,
    /// Requested branch for the implementation worktree. When set, Start
    /// creates or reuses that branch instead of generating one from the card.
    pub branch: Option<String>,
    /// Run `git fetch origin` before resolving `base_branch` for Start.
    pub fetch_first: Option<bool>,
    /// Last daemon-owned Start failure. The frontend renders this as the
    /// card-level start error; cleared by successful Start/config updates.
    pub start_error: Option<String>,
    /// Bound agent session — set when `work-item-start` succeeds.
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
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkItemInputPresence {
    pub body: bool,
    pub repo_path: bool,
    pub agent_profile: bool,
    pub base_branch: bool,
    pub worktree_path: bool,
    pub branch: bool,
    pub fetch_first: bool,
    pub start_error: bool,
    pub project_id: bool,
    pub parent_id: bool,
}

/// Input shape for creating / importing a work item. All fields except
/// `title` are optional; the store fills defaults.
#[derive(Debug, Clone, Default, specta::Type)]
pub struct WorkItemInput {
    pub title: String,
    pub body: Option<String>,
    pub status: Option<WorkItemStatus>,
    pub repo_path: Option<String>,
    pub agent_profile: Option<String>,
    pub base_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub fetch_first: Option<bool>,
    pub start_error: Option<String>,
    pub project_id: Option<String>,
    pub parent_id: Option<String>,
    pub external_ref: Option<ExternalRef>,
    pub sort_order: Option<f64>,
    #[specta(skip)]
    pub field_presence: WorkItemInputPresence,
}

impl WorkItemInput {
    pub fn body_present(&self) -> bool {
        self.field_presence.body || self.body.is_some()
    }

    pub fn repo_path_present(&self) -> bool {
        self.field_presence.repo_path || self.repo_path.is_some()
    }

    pub fn agent_profile_present(&self) -> bool {
        self.field_presence.agent_profile || self.agent_profile.is_some()
    }

    pub fn base_branch_present(&self) -> bool {
        self.field_presence.base_branch || self.base_branch.is_some()
    }

    pub fn worktree_path_present(&self) -> bool {
        self.field_presence.worktree_path || self.worktree_path.is_some()
    }

    pub fn branch_present(&self) -> bool {
        self.field_presence.branch || self.branch.is_some()
    }

    pub fn fetch_first_present(&self) -> bool {
        self.field_presence.fetch_first || self.fetch_first.is_some()
    }

    pub fn start_error_present(&self) -> bool {
        self.field_presence.start_error || self.start_error.is_some()
    }

    pub fn project_id_present(&self) -> bool {
        self.field_presence.project_id || self.project_id.is_some()
    }

    pub fn parent_id_present(&self) -> bool {
        self.field_presence.parent_id || self.parent_id.is_some()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkItemInputSerde {
    title: String,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    body: Option<Option<String>>,
    #[serde(default)]
    status: Option<WorkItemStatus>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    repo_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    agent_profile: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    base_branch: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    worktree_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    branch: Option<Option<String>>,
    #[serde(default, alias = "fetch_first", deserialize_with = "deserialize_nullable_field")]
    fetch_first: Option<Option<bool>>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    start_error: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    project_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_field")]
    parent_id: Option<Option<String>>,
    #[serde(default)]
    external_ref: Option<ExternalRef>,
    #[serde(default)]
    sort_order: Option<f64>,
}

fn deserialize_nullable_field<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some).map_err(de::Error::custom)
}

impl<'de> Deserialize<'de> for WorkItemInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = WorkItemInputSerde::deserialize(deserializer)?;
        Ok(Self {
            title: raw.title,
            body: raw.body.clone().flatten(),
            status: raw.status,
            repo_path: raw.repo_path.clone().flatten(),
            agent_profile: raw.agent_profile.clone().flatten(),
            base_branch: raw.base_branch.clone().flatten(),
            worktree_path: raw.worktree_path.clone().flatten(),
            branch: raw.branch.clone().flatten(),
            fetch_first: raw.fetch_first.flatten(),
            start_error: raw.start_error.clone().flatten(),
            project_id: raw.project_id.clone().flatten(),
            parent_id: raw.parent_id.clone().flatten(),
            external_ref: raw.external_ref,
            sort_order: raw.sort_order,
            field_presence: WorkItemInputPresence {
                body: raw.body.is_some(),
                repo_path: raw.repo_path.is_some(),
                agent_profile: raw.agent_profile.is_some(),
                base_branch: raw.base_branch.is_some(),
                worktree_path: raw.worktree_path.is_some(),
                branch: raw.branch.is_some(),
                fetch_first: raw.fetch_first.is_some(),
                start_error: raw.start_error.is_some(),
                project_id: raw.project_id.is_some(),
                parent_id: raw.parent_id.is_some(),
            },
        })
    }
}

impl Serialize for WorkItemInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("title", &self.title)?;
        serialize_optional_entry(&mut map, "body", &self.body, self.body_present())?;
        serialize_optional_entry(&mut map, "status", &self.status, self.status.is_some())?;
        serialize_optional_entry(&mut map, "repoPath", &self.repo_path, self.repo_path_present())?;
        serialize_optional_entry(
            &mut map,
            "agentProfile",
            &self.agent_profile,
            self.agent_profile_present(),
        )?;
        serialize_optional_entry(
            &mut map,
            "baseBranch",
            &self.base_branch,
            self.base_branch_present(),
        )?;
        serialize_optional_entry(
            &mut map,
            "worktreePath",
            &self.worktree_path,
            self.worktree_path_present(),
        )?;
        serialize_optional_entry(&mut map, "branch", &self.branch, self.branch_present())?;
        serialize_optional_entry(
            &mut map,
            "fetchFirst",
            &self.fetch_first,
            self.fetch_first_present(),
        )?;
        serialize_optional_entry(
            &mut map,
            "startError",
            &self.start_error,
            self.start_error_present(),
        )?;
        serialize_optional_entry(
            &mut map,
            "projectId",
            &self.project_id,
            self.project_id_present(),
        )?;
        serialize_optional_entry(&mut map, "parentId", &self.parent_id, self.parent_id_present())?;
        serialize_optional_entry(
            &mut map,
            "externalRef",
            &self.external_ref,
            self.external_ref.is_some(),
        )?;
        serialize_optional_entry(
            &mut map,
            "sortOrder",
            &self.sort_order,
            self.sort_order.is_some(),
        )?;
        map.end()
    }
}

fn serialize_optional_entry<S, T>(
    map: &mut S,
    key: &'static str,
    value: &Option<T>,
    present: bool,
) -> Result<(), S::Error>
where
    S: SerializeMap,
    T: Serialize,
{
    if present {
        map.serialize_entry(key, value)?;
    }
    Ok(())
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
    RunCreated { run: WorkItemRun },
    RunUpdated { run: WorkItemRun },
    RunEventAppended { event: WorkItemRunEvent },
    DecisionCreated { decision: WorkItemDecision },
    DecisionResolved { decision: WorkItemDecision },
    DecisionTimedOut { decision: WorkItemDecision },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemRunStatus {
    Queued,
    Starting,
    #[default]
    Running,
    Blocked,
    Review,
    Failed,
    Stopped,
    Done,
}

impl WorkItemRunStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Done => "done",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(Self::Queued),
            "starting" => Some(Self::Starting),
            "running" => Some(Self::Running),
            "blocked" => Some(Self::Blocked),
            "review" => Some(Self::Review),
            "failed" => Some(Self::Failed),
            "stopped" => Some(Self::Stopped),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemRunKind {
    Planning,
    #[default]
    Implementation,
    Review,
}

impl WorkItemRunKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Planning => "planning",
            Self::Implementation => "implementation",
            Self::Review => "review",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "planning" => Some(Self::Planning),
            "implementation" => Some(Self::Implementation),
            "review" => Some(Self::Review),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemRun {
    pub id: String,
    pub work_item_id: String,
    pub kind: WorkItemRunKind,
    pub session_id: Option<String>,
    pub provider: Option<String>,
    pub profile_id: Option<String>,
    pub status: WorkItemRunStatus,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub cost: Option<f64>,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemStartResult {
    pub item: WorkItem,
    pub run: WorkItemRun,
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemPlanResult {
    pub item: WorkItem,
    pub run: WorkItemRun,
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemReviewAcceptResult {
    pub item: WorkItem,
    pub run: WorkItemRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemRunEventKind {
    Lifecycle,
    Text,
    ToolUse,
    ToolResult,
    Decision,
    DecisionResolved,
    DecisionTimedOut,
    Result,
    Error,
    StatusChanged,
}

impl WorkItemRunEventKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Lifecycle => "lifecycle",
            Self::Text => "text",
            Self::ToolUse => "tool_use",
            Self::ToolResult => "tool_result",
            Self::Decision => "decision",
            Self::DecisionResolved => "decision_resolved",
            Self::DecisionTimedOut => "decision_timed_out",
            Self::Result => "result",
            Self::Error => "error",
            Self::StatusChanged => "status_changed",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "lifecycle" => Some(Self::Lifecycle),
            "text" => Some(Self::Text),
            "tool_use" => Some(Self::ToolUse),
            "tool_result" => Some(Self::ToolResult),
            "decision" => Some(Self::Decision),
            "decision_resolved" => Some(Self::DecisionResolved),
            "decision_timed_out" => Some(Self::DecisionTimedOut),
            "result" => Some(Self::Result),
            "error" => Some(Self::Error),
            "status_changed" => Some(Self::StatusChanged),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemRunEvent {
    pub id: String,
    pub run_id: String,
    pub kind: WorkItemRunEventKind,
    pub payload: Value,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemDecisionStatus {
    #[default]
    Pending,
    Resolved,
    TimedOut,
}

impl WorkItemDecisionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::TimedOut => "timed_out",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "resolved" => Some(Self::Resolved),
            "timed_out" => Some(Self::TimedOut),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemDecisionOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemDecision {
    pub id: String,
    pub run_id: String,
    pub question: String,
    pub options: Vec<WorkItemDecisionOption>,
    pub default_value: Option<String>,
    pub timeout_at: Option<u64>,
    pub status: WorkItemDecisionStatus,
    pub resolved_value: Option<String>,
    pub resolved_by: Option<String>,
    pub created_at: u64,
    pub resolved_at: Option<u64>,
    pub updated_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_status_round_trips() {
        for (s, expected) in [
            ("todo", WorkItemStatus::Todo),
            ("ready", WorkItemStatus::Ready),
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

    #[test]
    fn work_item_run_status_round_trips() {
        for (s, expected) in [
            ("queued", WorkItemRunStatus::Queued),
            ("starting", WorkItemRunStatus::Starting),
            ("running", WorkItemRunStatus::Running),
            ("blocked", WorkItemRunStatus::Blocked),
            ("review", WorkItemRunStatus::Review),
            ("failed", WorkItemRunStatus::Failed),
            ("stopped", WorkItemRunStatus::Stopped),
            ("done", WorkItemRunStatus::Done),
        ] {
            let status = WorkItemRunStatus::from_str_opt(s).unwrap();
            assert_eq!(status, expected);
            assert_eq!(status.as_str(), s);
        }
    }

    #[test]
    fn work_item_decision_status_round_trips() {
        for (s, expected) in [
            ("pending", WorkItemDecisionStatus::Pending),
            ("resolved", WorkItemDecisionStatus::Resolved),
            ("timed_out", WorkItemDecisionStatus::TimedOut),
        ] {
            let status = WorkItemDecisionStatus::from_str_opt(s).unwrap();
            assert_eq!(status, expected);
            assert_eq!(status.as_str(), s);
        }
    }
}
