use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Categorical event kind. Lets the UI default-filter the main inbox to
/// human-actionable items (`Task`, `Question`) and route `Fyi` / `Signal`
/// noise to the firehose tab. Senders pick the kind; storage is the same
/// for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    /// Direct task handoff: "please do X."
    #[default]
    Task,
    /// Reply to a Task with the outcome.
    Result,
    /// Asks for input. Surfaces in the human's main inbox by default.
    Question,
    /// Passive notification, no reply expected.
    Fyi,
    /// Bus-style ambient signal (e.g. `build.completed`).
    Signal,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EventValidationError {
    #[error("event must address at least one of `to` or `topic`")]
    NoAddressing,
    #[error("event body is empty")]
    EmptyBody,
}

/// Append-only event in the unified store. Both mailbox-style direct
/// addressing (`to=<alias>`) and bus-style topic addressing (`topic=...`)
/// live in the same row; an event may use either, both, or only `topic`.
///
/// Per-recipient read/ack state is split into `ReadState` so the same
/// event can serve N subscribers without copying.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Unique id (uuid v4 in current implementation; ordering is by
    /// `created_at`, not id).
    pub id: String,
    /// Unix epoch milliseconds.
    pub created_at: u64,

    // ── Addressing axes ─────────────────────────────────────────────
    /// Recipient alias. `Some` for mailbox-style direct addressing,
    /// `None` for pure-topic broadcast events.
    #[serde(default)]
    pub to: Option<String>,
    /// Dotted topic name (e.g. `repo-a.build.completed`). `Some` to
    /// participate in bus-style fan-out; `None` for pure direct mail.
    #[serde(default)]
    pub topic: Option<String>,
    /// Sender alias or session id.
    #[serde(default)]
    pub from: Option<String>,

    // ── Categorical axes ────────────────────────────────────────────
    pub kind: EventKind,
    /// Thread key. New events copy from the event they're replying to;
    /// originals leave it `None`.
    #[serde(default)]
    pub correlation_id: Option<String>,
    /// Project scope for filtering. `None` = global.
    #[serde(default)]
    pub project_id: Option<String>,

    // ── Payload ─────────────────────────────────────────────────────
    /// Optional short subject line for inbox display.
    #[serde(default)]
    pub subject: Option<String>,
    /// Free-form body text.
    pub body: String,
    /// Optional structured JSON payload. Convention encourages
    /// `{ task, context, expectsReply }` but nothing is enforced.
    #[serde(default)]
    pub structured: Option<Value>,

    // ── Lifecycle ───────────────────────────────────────────────────
    /// Set when the sender retracts (unsends) this event before any
    /// recipient has acked. Retracted events are filtered out of
    /// recipient inbox views and the firehose but stay in
    /// `events.jsonl` for audit (a retract marker row is applied at
    /// load time). The sender's `mailbox sent` view still surfaces
    /// them with this timestamp visible.
    #[serde(default)]
    pub retracted_at: Option<u64>,
}

impl Event {
    /// True when the sender has unsent this event. Retracted events
    /// are filtered out of inbox/firehose views.
    pub fn is_retracted(&self) -> bool {
        self.retracted_at.is_some()
    }
}

/// Per-recipient mutable state for an event. Split from `Event` so a
/// single broadcast event can have N independent cursors without
/// duplicating the payload.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadState {
    pub event_id: String,
    /// Alias (or session id) that owns this state row.
    pub recipient: String,
    /// First time the recipient saw the event. Set automatically on
    /// `mailbox read`. `Some` is weaker than acked.
    #[serde(default)]
    pub read_at: Option<u64>,
    /// Time the recipient acked. `Some` means processed/done.
    #[serde(default)]
    pub acked_at: Option<u64>,
    /// Optional short result string returned with the ack
    /// (`mailbox ack <id> --result "PR merged"`).
    #[serde(default)]
    pub ack_result: Option<String>,
    /// Set when the recipient has cleared this event from their inbox
    /// view via `mailbox clear`. The underlying event is preserved
    /// (other recipients can still see it); cleared events drop from
    /// `list_for_recipient` and don't count as unread for this
    /// recipient. Distinct from `read_at` so the prior read state isn't
    /// lost — without this marker, "clear read" would delete `read_at`
    /// and the event would re-surface as unread on the next list call.
    #[serde(default)]
    pub cleared_at: Option<u64>,
}

impl ReadState {
    pub fn new(event_id: impl Into<String>, recipient: impl Into<String>) -> Self {
        Self {
            event_id: event_id.into(),
            recipient: recipient.into(),
            read_at: None,
            acked_at: None,
            ack_result: None,
            cleared_at: None,
        }
    }

    pub fn is_read(&self) -> bool {
        self.read_at.is_some()
    }

    pub fn is_acked(&self) -> bool {
        self.acked_at.is_some()
    }

    /// True when the recipient has cleared this event from their view.
    /// Cleared events are filtered out of `list_for_recipient` and don't
    /// count as unread.
    pub fn is_cleared(&self) -> bool {
        self.cleared_at.is_some()
    }
}

/// Tauri event emitted on every store mutation so the frontend can update
/// without polling. Mirrors the NotificationEvent / AliasEvent shape.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MailboxEvent {
    /// New event was appended.
    Posted { event: Event },
    /// Recipient marked an event read (or read an unread event for the first time).
    Read { event_id: String, recipient: String },
    /// Recipient acked an event with optional result.
    Acked {
        event_id: String,
        recipient: String,
        #[serde(default)]
        result: Option<String>,
    },
    /// Recipient cleared their inbox (or a project-scoped slice of it).
    Cleared { recipient: String, count: u32 },
    /// A `bus publish` matched a persistent subscription. Frontend uses
    /// this to bump unread counts for the subscriber alias and to surface
    /// the delivery in the recipient's inbox without an explicit
    /// `to=<alias>` on the underlying event.
    TopicDelivered {
        event_id: String,
        recipient: String,
        subscription_id: String,
    },
    /// Sender unsent the event. Recipients should drop it from their
    /// inbox view; the row stays in `events.jsonl` for audit.
    Retracted { event_id: String },
    /// Recipient dismissed a single event from their inbox without
    /// having read it (or after, doesn't matter). The event itself is
    /// preserved; only this recipient's view loses it.
    Dismissed { event_id: String, recipient: String },
}

/// Builder for new events. Validates at construction so callers get a
/// typed error rather than a silently malformed row in the store.
#[derive(Debug, Clone, Default)]
pub struct EventBuilder {
    pub to: Option<String>,
    pub topic: Option<String>,
    pub from: Option<String>,
    pub kind: EventKind,
    pub correlation_id: Option<String>,
    pub project_id: Option<String>,
    pub subject: Option<String>,
    pub body: String,
    pub structured: Option<Value>,
}

impl EventBuilder {
    pub fn new(body: impl Into<String>) -> Self {
        Self { body: body.into(), ..Default::default() }
    }

    pub fn to(mut self, alias: impl Into<String>) -> Self {
        self.to = Some(alias.into());
        self
    }
    pub fn topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }
    pub fn from(mut self, sender: impl Into<String>) -> Self {
        self.from = Some(sender.into());
        self
    }
    pub fn kind(mut self, kind: EventKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn correlation_id(mut self, cid: impl Into<String>) -> Self {
        self.correlation_id = Some(cid.into());
        self
    }
    pub fn project_id(mut self, pid: impl Into<String>) -> Self {
        self.project_id = Some(pid.into());
        self
    }
    pub fn subject(mut self, s: impl Into<String>) -> Self {
        self.subject = Some(s.into());
        self
    }
    pub fn structured(mut self, v: Value) -> Self {
        self.structured = Some(v);
        self
    }

    /// Validate addressing/body invariants but do NOT assign `id` /
    /// `created_at` — those come from the store on insert.
    pub fn validate(&self) -> Result<(), EventValidationError> {
        if self.to.is_none() && self.topic.is_none() {
            return Err(EventValidationError::NoAddressing);
        }
        if self.body.trim().is_empty() && self.structured.is_none() {
            return Err(EventValidationError::EmptyBody);
        }
        Ok(())
    }

    /// Build the Event with caller-supplied id and timestamp. Used by the
    /// store, which owns id assignment so the same uuid library doesn't
    /// have to be available in `roux-core`.
    pub fn build_with(
        self,
        id: impl Into<String>,
        created_at: u64,
    ) -> Result<Event, EventValidationError> {
        self.validate()?;
        Ok(Event {
            id: id.into(),
            created_at,
            to: self.to,
            topic: self.topic,
            from: self.from,
            kind: self.kind,
            correlation_id: self.correlation_id,
            project_id: self.project_id,
            subject: self.subject,
            body: self.body,
            structured: self.structured,
            retracted_at: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_validate_requires_addressing() {
        let e = EventBuilder::new("hello");
        assert_eq!(e.validate(), Err(EventValidationError::NoAddressing));
    }

    #[test]
    fn builder_validate_accepts_to_only() {
        let e = EventBuilder::new("hello").to("reviewer");
        assert!(e.validate().is_ok());
    }

    #[test]
    fn builder_validate_accepts_topic_only() {
        let e = EventBuilder::new("hello").topic("build.completed");
        assert!(e.validate().is_ok());
    }

    #[test]
    fn builder_validate_accepts_both() {
        let e = EventBuilder::new("hello").to("reviewer").topic("review.requested");
        assert!(e.validate().is_ok());
    }

    #[test]
    fn builder_validate_rejects_empty_body_without_structured() {
        let e = EventBuilder::new("   ").to("reviewer");
        assert_eq!(e.validate(), Err(EventValidationError::EmptyBody));
    }

    #[test]
    fn builder_validate_accepts_empty_body_with_structured_payload() {
        let e = EventBuilder::new("")
            .to("reviewer")
            .structured(serde_json::json!({"task": "review"}));
        assert!(e.validate().is_ok());
    }

    #[test]
    fn build_with_assigns_id_and_timestamp() {
        let event = EventBuilder::new("hello")
            .to("reviewer")
            .from("me")
            .kind(EventKind::Task)
            .build_with("evt-1", 12345)
            .unwrap();
        assert_eq!(event.id, "evt-1");
        assert_eq!(event.created_at, 12345);
        assert_eq!(event.to.as_deref(), Some("reviewer"));
        assert_eq!(event.from.as_deref(), Some("me"));
        assert_eq!(event.kind, EventKind::Task);
    }

    #[test]
    fn build_with_propagates_validation_failure() {
        let result = EventBuilder::new("hi").build_with("evt-1", 0);
        assert!(matches!(result, Err(EventValidationError::NoAddressing)));
    }

    #[test]
    fn builder_chains_correlation_and_project() {
        let event = EventBuilder::new("reply")
            .to("reviewer")
            .from("builder")
            .kind(EventKind::Result)
            .correlation_id("thread-1")
            .project_id("repo-a")
            .subject("done")
            .build_with("evt-2", 1)
            .unwrap();
        assert_eq!(event.correlation_id.as_deref(), Some("thread-1"));
        assert_eq!(event.project_id.as_deref(), Some("repo-a"));
        assert_eq!(event.subject.as_deref(), Some("done"));
    }

    #[test]
    fn read_state_starts_unread_unacked() {
        let state = ReadState::new("evt-1", "reviewer");
        assert!(!state.is_read());
        assert!(!state.is_acked());
    }

    #[test]
    fn read_state_serde_round_trip() {
        let mut state = ReadState::new("evt-1", "reviewer");
        state.read_at = Some(1000);
        state.acked_at = Some(1500);
        state.ack_result = Some("PR merged".to_string());
        let json = serde_json::to_string(&state).unwrap();
        let parsed: ReadState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn event_kind_default_is_task() {
        assert_eq!(EventKind::default(), EventKind::Task);
    }

    #[test]
    fn event_serde_round_trip_preserves_all_fields() {
        let original = EventBuilder::new("hello")
            .to("reviewer")
            .topic("review.requested")
            .from("builder")
            .kind(EventKind::Question)
            .correlation_id("thread-1")
            .project_id("repo-a")
            .subject("PR needs review")
            .structured(serde_json::json!({"pr_url": "https://example.com"}))
            .build_with("evt-1", 42)
            .unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn event_kind_serializes_camel_case() {
        let v = serde_json::to_string(&EventKind::Fyi).unwrap();
        assert_eq!(v, "\"fyi\"");
        let v = serde_json::to_string(&EventKind::Task).unwrap();
        assert_eq!(v, "\"task\"");
    }
}
