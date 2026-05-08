use std::collections::{HashMap, VecDeque};

use roux_core::{Event, EventBuilder, EventValidationError, ReadState};

use crate::aliases::ProjectFilter;

const DEFAULT_CAPACITY: usize = 5000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PostError {
    #[error(transparent)]
    Validation(#[from] EventValidationError),
}

/// Append-only event store with per-recipient read/ack state. Events are
/// keyed by `id`; `ReadState` is keyed by `(recipient, event_id)` so a
/// single broadcast event can have N independent cursors without
/// duplicating the payload.
///
/// Mirrors the `NotificationStore` shape: in-memory primary, intended to
/// be wrapped by a `Manager` for persistence + Tauri event emission.
pub struct EventStore {
    events: VecDeque<Event>,
    /// Indexed lookup keyed by `(recipient_alias, event_id)`.
    read_state: HashMap<(String, String), ReadState>,
    capacity: usize,
    max_age_ms: Option<u64>,
}

impl EventStore {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY, None)
    }

    pub fn with_capacity(capacity: usize, max_age_ms: Option<u64>) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity.min(256)),
            read_state: HashMap::new(),
            capacity,
            max_age_ms,
        }
    }

    pub fn from_entries(events: Vec<Event>, read_state: Vec<ReadState>) -> Self {
        let mut store = Self::new();
        for e in events {
            store.events.push_back(e);
        }
        for s in read_state {
            store
                .read_state
                .insert((s.recipient.clone(), s.event_id.clone()), s);
        }
        store.evict_overflow();
        store
    }

    /// Append `builder`'s event. Caller passes a generated `id` and `now_ms`
    /// (the store doesn't depend on the uuid crate so roux-core stays free
    /// of id-generation deps). Returns the stored event on success.
    pub fn post(
        &mut self,
        builder: EventBuilder,
        id: String,
        now_ms: u64,
    ) -> Result<Event, PostError> {
        let event = builder.build_with(id, now_ms)?;
        self.events.push_back(event.clone());
        self.evict_overflow();
        Ok(event)
    }

    /// Drop excess events past the capacity / age caps. Also drops orphaned
    /// `ReadState` rows so the indexed lookup never points at a missing event.
    fn evict_overflow(&mut self) {
        while self.events.len() > self.capacity {
            if let Some(dropped) = self.events.pop_front() {
                self.read_state.retain(|(_, eid), _| eid != &dropped.id);
            }
        }
        if let Some(max_age) = self.max_age_ms {
            let now = now_ms();
            let cutoff = now.saturating_sub(max_age);
            while let Some(front) = self.events.front() {
                if front.created_at < cutoff {
                    let dropped = self.events.pop_front().expect("front existed");
                    self.read_state.retain(|(_, eid), _| eid != &dropped.id);
                } else {
                    break;
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.events.iter()
    }

    pub fn all_read_state(&self) -> impl Iterator<Item = &ReadState> {
        self.read_state.values()
    }

    /// Per-recipient state for a specific event, if any has been recorded.
    pub fn read_state(&self, event_id: &str, recipient: &str) -> Option<&ReadState> {
        self.read_state.get(&(recipient.to_string(), event_id.to_string()))
    }

    fn read_state_mut_or_insert(&mut self, event_id: &str, recipient: &str) -> &mut ReadState {
        self.read_state
            .entry((recipient.to_string(), event_id.to_string()))
            .or_insert_with(|| ReadState::new(event_id, recipient))
    }

    fn is_read_by(&self, event_id: &str, recipient: &str) -> bool {
        self.read_state(event_id, recipient).map(|s| s.is_read()).unwrap_or(false)
    }

    /// Events addressed `to=<recipient>` matching the project filter. Oldest
    /// first (insertion order). `unread_only` filters to events without a
    /// `read_at` timestamp for this recipient.
    pub fn list_for_recipient(
        &self,
        recipient: &str,
        unread_only: bool,
        project_filter: ProjectFilter<'_>,
    ) -> Vec<Event> {
        self.events
            .iter()
            .filter(|e| e.to.as_deref() == Some(recipient))
            .filter(|e| project_filter.matches(e.project_id.as_deref()))
            .filter(|e| !unread_only || !self.is_read_by(&e.id, recipient))
            .cloned()
            .collect()
    }

    /// Events on `topic` (exact match in Phase 2; glob support is a
    /// follow-up). Includes events that also have `to` set.
    pub fn list_for_topic(&self, topic: &str, project_filter: ProjectFilter<'_>) -> Vec<Event> {
        self.events
            .iter()
            .filter(|e| e.topic.as_deref() == Some(topic))
            .filter(|e| project_filter.matches(e.project_id.as_deref()))
            .cloned()
            .collect()
    }

    /// Firehose view: every event matching the project filter, newest first.
    pub fn list_all(&self, project_filter: ProjectFilter<'_>, limit: Option<usize>) -> Vec<Event> {
        let iter = self
            .events
            .iter()
            .rev()
            .filter(|e| project_filter.matches(e.project_id.as_deref()));
        match limit {
            Some(n) => iter.take(n).cloned().collect(),
            None => iter.cloned().collect(),
        }
    }

    /// Events sent BY `sender` (matches `from`), newest first. Each row pairs
    /// the event with its `ReadState` for `recipient_filter` so the sender
    /// can see read/ack state. When `recipient_filter` is `None`, every
    /// event the sender posted is returned without per-recipient pairing.
    pub fn list_sent_by(
        &self,
        sender: &str,
        recipient_filter: Option<&str>,
        limit: Option<usize>,
    ) -> Vec<(Event, Option<ReadState>)> {
        let mut out: Vec<(Event, Option<ReadState>)> = Vec::new();
        for e in self.events.iter().rev() {
            if e.from.as_deref() != Some(sender) {
                continue;
            }
            let state = match recipient_filter {
                Some(r) => self.read_state(&e.id, r).cloned(),
                None => e
                    .to
                    .as_deref()
                    .and_then(|t| self.read_state(&e.id, t).cloned()),
            };
            out.push((e.clone(), state));
            if let Some(n) = limit {
                if out.len() >= n {
                    break;
                }
            }
        }
        out
    }

    /// Idempotently mark `event_id` as read for `recipient`. Returns true
    /// when state changed (i.e. it wasn't already read).
    pub fn mark_read(&mut self, event_id: &str, recipient: &str, now_ms: u64) -> bool {
        // Don't materialize ReadState for events that don't exist.
        if self.get(event_id).is_none() {
            return false;
        }
        let state = self.read_state_mut_or_insert(event_id, recipient);
        if state.read_at.is_some() {
            return false;
        }
        state.read_at = Some(now_ms);
        true
    }

    /// Ack with optional result string. Sets `acked_at` (and `read_at` if
    /// not already set — ack implies read). Returns true on state change.
    pub fn ack(
        &mut self,
        event_id: &str,
        recipient: &str,
        result: Option<String>,
        now_ms: u64,
    ) -> bool {
        if self.get(event_id).is_none() {
            return false;
        }
        let state = self.read_state_mut_or_insert(event_id, recipient);
        if state.acked_at.is_some() {
            // Allow updating the result string; it's the more informative
            // change. But don't bump `acked_at` again.
            if result.is_some() && state.ack_result != result {
                state.ack_result = result;
                return true;
            }
            return false;
        }
        state.acked_at = Some(now_ms);
        if state.read_at.is_none() {
            state.read_at = Some(now_ms);
        }
        state.ack_result = result;
        true
    }

    pub fn unread_count(&self, recipient: &str, project_filter: ProjectFilter<'_>) -> usize {
        self.events
            .iter()
            .filter(|e| e.to.as_deref() == Some(recipient))
            .filter(|e| project_filter.matches(e.project_id.as_deref()))
            .filter(|e| !self.is_read_by(&e.id, recipient))
            .count()
    }

    /// Clear read events for `recipient`. Removes their `ReadState` rows
    /// where `read_at` is set; the underlying events stay so other
    /// recipients can still see them. Returns the count of removed rows.
    pub fn clear_read(&mut self, recipient: &str) -> usize {
        let before = self.read_state.len();
        self.read_state
            .retain(|(r, _), s| !(r == recipient && s.read_at.is_some()));
        before - self.read_state.len()
    }
}

impl Default for EventStore {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::EventKind;

    fn post_to(
        store: &mut EventStore,
        id: &str,
        recipient: &str,
        body: &str,
        from: Option<&str>,
        project: Option<&str>,
    ) -> Event {
        let mut b = EventBuilder::new(body).to(recipient).kind(EventKind::Task);
        if let Some(f) = from {
            b = b.from(f);
        }
        if let Some(p) = project {
            b = b.project_id(p);
        }
        store.post(b, id.to_string(), 1000).unwrap()
    }

    #[test]
    fn post_appends_event_and_returns_it() {
        let mut store = EventStore::new();
        let posted = post_to(&mut store, "evt-1", "reviewer", "hello", None, None);
        assert_eq!(posted.id, "evt-1");
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("evt-1").map(|e| e.id.clone()), Some("evt-1".to_string()));
    }

    #[test]
    fn post_validates_addressing() {
        let mut store = EventStore::new();
        let err = store
            .post(EventBuilder::new("hi"), "evt-1".into(), 0)
            .unwrap_err();
        assert!(matches!(err, PostError::Validation(_)));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn list_for_recipient_returns_only_addressed_events() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "builder", "b", None, None);
        post_to(&mut store, "e3", "reviewer", "c", None, None);

        let mine = store.list_for_recipient("reviewer", false, ProjectFilter::Any);
        let ids: Vec<_> = mine.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e1", "e3"]);
    }

    #[test]
    fn list_for_recipient_unread_only_filters_read_events() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        store.mark_read("e1", "reviewer", 2000);

        let unread = store.list_for_recipient("reviewer", true, ProjectFilter::Any);
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, "e2");
    }

    #[test]
    fn list_for_recipient_respects_project_filter() {
        let mut store = EventStore::new();
        post_to(&mut store, "g", "reviewer", "global", None, None);
        post_to(&mut store, "a", "reviewer", "in-a", None, Some("proj-a"));
        post_to(&mut store, "b", "reviewer", "in-b", None, Some("proj-b"));

        let only_a = store.list_for_recipient(
            "reviewer",
            false,
            ProjectFilter::Exact(Some("proj-a")),
        );
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].id, "a");

        let global_only = store.list_for_recipient("reviewer", false, ProjectFilter::Exact(None));
        assert_eq!(global_only.len(), 1);
        assert_eq!(global_only[0].id, "g");
    }

    #[test]
    fn list_for_topic_returns_events_with_matching_topic() {
        let mut store = EventStore::new();
        let e1 = EventBuilder::new("build a").topic("build.completed");
        let e2 = EventBuilder::new("build b").topic("build.failed");
        let e3 = EventBuilder::new("hi").to("reviewer").topic("build.completed");
        store.post(e1, "e1".into(), 1).unwrap();
        store.post(e2, "e2".into(), 2).unwrap();
        store.post(e3, "e3".into(), 3).unwrap();

        let completed = store.list_for_topic("build.completed", ProjectFilter::Any);
        let ids: Vec<_> = completed.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e1", "e3"]);
    }

    #[test]
    fn list_all_returns_newest_first() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "first", None, None);
        post_to(&mut store, "e2", "reviewer", "second", None, None);
        post_to(&mut store, "e3", "reviewer", "third", None, None);

        let all = store.list_all(ProjectFilter::Any, None);
        let ids: Vec<_> = all.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e3", "e2", "e1"]);
    }

    #[test]
    fn list_all_respects_limit() {
        let mut store = EventStore::new();
        for i in 0..5 {
            post_to(&mut store, &format!("e{i}"), "reviewer", "x", None, None);
        }
        let limited = store.list_all(ProjectFilter::Any, Some(2));
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].id, "e4");
        assert_eq!(limited[1].id, "e3");
    }

    #[test]
    fn mark_read_is_idempotent() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        assert!(store.mark_read("e1", "reviewer", 1000));
        assert!(!store.mark_read("e1", "reviewer", 2000), "second call should report no change");
        let state = store.read_state("e1", "reviewer").unwrap();
        assert_eq!(state.read_at, Some(1000), "first read_at should be preserved");
    }

    #[test]
    fn mark_read_for_unknown_event_is_noop() {
        let mut store = EventStore::new();
        assert!(!store.mark_read("nope", "reviewer", 1000));
        assert!(store.read_state("nope", "reviewer").is_none(), "no orphan state row");
    }

    #[test]
    fn ack_implies_read() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        assert!(store.ack("e1", "reviewer", Some("done".into()), 1500));
        let state = store.read_state("e1", "reviewer").unwrap();
        assert!(state.is_read());
        assert!(state.is_acked());
        assert_eq!(state.read_at, Some(1500));
        assert_eq!(state.acked_at, Some(1500));
        assert_eq!(state.ack_result.as_deref(), Some("done"));
    }

    #[test]
    fn ack_after_read_preserves_read_at() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        store.mark_read("e1", "reviewer", 1000);
        assert!(store.ack("e1", "reviewer", None, 2000));
        let state = store.read_state("e1", "reviewer").unwrap();
        assert_eq!(state.read_at, Some(1000));
        assert_eq!(state.acked_at, Some(2000));
    }

    #[test]
    fn double_ack_is_noop_unless_result_changes() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        store.ack("e1", "reviewer", Some("first".into()), 1000);
        assert!(!store.ack("e1", "reviewer", None, 2000), "redundant ack returns false");
        // Updating the result is allowed.
        assert!(store.ack("e1", "reviewer", Some("better".into()), 3000));
        let state = store.read_state("e1", "reviewer").unwrap();
        assert_eq!(state.acked_at, Some(1000), "acked_at preserved across re-ack");
        assert_eq!(state.ack_result.as_deref(), Some("better"));
    }

    #[test]
    fn unread_count_drops_after_read() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        post_to(&mut store, "e3", "builder", "c", None, None);
        assert_eq!(store.unread_count("reviewer", ProjectFilter::Any), 2);
        store.mark_read("e1", "reviewer", 1500);
        assert_eq!(store.unread_count("reviewer", ProjectFilter::Any), 1);
    }

    #[test]
    fn clear_read_removes_only_read_rows_for_recipient() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        post_to(&mut store, "e3", "builder", "c", None, None);
        store.mark_read("e1", "reviewer", 1000);
        store.mark_read("e3", "builder", 1000);

        let cleared = store.clear_read("reviewer");
        assert_eq!(cleared, 1);
        assert!(store.read_state("e1", "reviewer").is_none());
        // Builder's state is untouched.
        assert!(store.read_state("e3", "builder").is_some());
        // Underlying event is preserved.
        assert!(store.get("e1").is_some());
    }

    #[test]
    fn capacity_eviction_drops_orphan_read_state() {
        let mut store = EventStore::with_capacity(2, None);
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        store.mark_read("e1", "reviewer", 500);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        post_to(&mut store, "e3", "reviewer", "c", None, None);
        // e1 should be evicted now; its read_state row should also be gone.
        assert_eq!(store.len(), 2);
        assert!(store.get("e1").is_none());
        assert!(store.read_state("e1", "reviewer").is_none(), "orphan state must be cleaned up");
    }

    #[test]
    fn list_sent_by_pairs_with_recipient_state() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", Some("me"), None);
        post_to(&mut store, "e2", "builder", "b", Some("me"), None);
        store.mark_read("e1", "reviewer", 1500);

        let sent = store.list_sent_by("me", None, None);
        assert_eq!(sent.len(), 2);
        // newest first
        assert_eq!(sent[0].0.id, "e2");
        assert_eq!(sent[1].0.id, "e1");
        assert!(sent[0].1.is_none(), "builder hasn't read yet");
        assert!(sent[1].1.as_ref().unwrap().is_read());
    }

    #[test]
    fn list_sent_by_recipient_filter_overrides_default() {
        let mut store = EventStore::new();
        // Event addressed to reviewer; sender wants to see qa's view.
        post_to(&mut store, "e1", "reviewer", "a", Some("me"), None);
        store.mark_read("e1", "qa", 1500); // pretend qa is also tracking it

        let sent_qa_view = store.list_sent_by("me", Some("qa"), None);
        assert_eq!(sent_qa_view.len(), 1);
        assert!(sent_qa_view[0].1.as_ref().unwrap().is_read());
    }

    #[test]
    fn from_entries_round_trips_events_and_state() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        store.mark_read("e1", "reviewer", 1000);

        let events: Vec<_> = store.events().cloned().collect();
        let states: Vec<_> = store.all_read_state().cloned().collect();
        let restored = EventStore::from_entries(events, states);
        assert_eq!(restored.len(), 2);
        assert!(restored.read_state("e1", "reviewer").unwrap().is_read());
        assert!(restored.read_state("e2", "reviewer").is_none());
    }
}
