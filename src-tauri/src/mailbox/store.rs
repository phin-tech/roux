use std::collections::{HashMap, VecDeque};

use roux_core::{topic_matches, Event, EventBuilder, EventValidationError, ReadState};

use crate::aliases::ProjectFilter;

/// Empty pattern slice — convenience for callers that don't have
/// subscriptions wired (most existing tests, the in-memory mailbox
/// manager, the bus-tail handler). Equivalent to "this recipient is not
/// subscribed to anything", so subscription semantics are a no-op.
pub const NO_SUBSCRIPTIONS: &[String] = &[];

const DEFAULT_CAPACITY: usize = 5000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PostError {
    #[error(transparent)]
    Validation(#[from] EventValidationError),
    /// Caller passed an `id` that already exists in the store. Should
    /// never happen with the uuid v4 generator the manager uses, but
    /// keeping it as a typed error guards against id-collision races
    /// in tests and future schemes (deterministic ids, etc.).
    #[error("duplicate event id: {0}")]
    DuplicateId(String),
    /// Disk persistence failed after the in-memory append. The manager
    /// rolls the in-memory state back before returning this so the UI
    /// doesn't observe a "Posted" that disappears on restart.
    #[error("persistence failed: {0}")]
    Persist(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RetractError {
    /// No event with that id exists in the store.
    #[error("event not found: {0}")]
    NotFound(String),
    /// Only the original sender may retract a posted event. Anyone
    /// else trying to unsend is blocked here.
    #[error("only the sender ({event_sender:?}) can retract event {event_id}; got {caller:?}")]
    NotSender { event_id: String, event_sender: Option<String>, caller: String },
    /// At least one recipient has acked the event. Retraction is only
    /// allowed before delivery is confirmed — once anyone acked, the
    /// audit trail must reflect that they saw it.
    #[error("event {0} already acked by a recipient; retraction blocked")]
    AlreadyAcked(String),
    /// The event was already retracted. Idempotent retract isn't
    /// useful (no state changes) so the second call is rejected; the
    /// manager surfaces this so callers don't double-emit Tauri events.
    #[error("event {0} already retracted")]
    AlreadyRetracted(String),
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
    /// Rejects duplicate ids — the store is keyed by id, so collisions
    /// would conflate ReadState rows across distinct events.
    pub fn post(
        &mut self,
        builder: EventBuilder,
        id: String,
        now_ms: u64,
    ) -> Result<Event, PostError> {
        if self.get(&id).is_some() {
            return Err(PostError::DuplicateId(id));
        }
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

    /// Remove an event by id. Used by `MailboxManager::post` to roll
    /// back an in-memory append when the disk persist fails. Returns
    /// true if the event existed.
    pub fn remove_event(&mut self, id: &str) -> bool {
        if let Some(pos) = self.events.iter().position(|e| e.id == id) {
            self.events.remove(pos);
            // Drop any ReadState rows that pointed at this event so the
            // index never holds dangling refs.
            self.read_state.retain(|(_, eid), _| eid != id);
            true
        } else {
            false
        }
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

    fn is_cleared_by(&self, event_id: &str, recipient: &str) -> bool {
        self.read_state(event_id, recipient).map(|s| s.is_cleared()).unwrap_or(false)
    }

    /// Events visible to `recipient`, oldest-first. An event is visible
    /// when either:
    ///
    /// - `e.to == recipient` (direct mail), or
    /// - `e.topic` matches any pattern in `subscribed_patterns` (bus
    ///   subscription delivery).
    ///
    /// `unread_only` filters to events without a `read_at` timestamp for
    /// this recipient. **Cleared events are always filtered out** —
    /// `clear_read` is meant to hide them.
    ///
    /// Caller is responsible for sourcing `subscribed_patterns` —
    /// typically the manager looks them up via the SubscriptionStore
    /// scoped to the project filter. Pass `NO_SUBSCRIPTIONS` when the
    /// recipient has no subscriptions or to preserve pre-subscription
    /// semantics.
    pub fn list_for_recipient(
        &self,
        recipient: &str,
        subscribed_patterns: &[String],
        unread_only: bool,
        project_filter: ProjectFilter<'_>,
    ) -> Vec<Event> {
        self.events
            .iter()
            // Retracted events disappear from recipient inboxes — the
            // sender unsent them before any ack confirmed delivery.
            .filter(|e| !e.is_retracted())
            .filter(|e| event_visible_to(e, recipient, subscribed_patterns))
            .filter(|e| project_filter.matches(e.project_id.as_deref()))
            .filter(|e| !self.is_cleared_by(&e.id, recipient))
            .filter(|e| !unread_only || !self.is_read_by(&e.id, recipient))
            .cloned()
            .collect()
    }

    /// Events on `topic` (exact match in Phase 2; glob support is a
    /// follow-up). Includes events that also have `to` set. Retracted
    /// events are filtered out — see `list_for_recipient`.
    pub fn list_for_topic(&self, topic: &str, project_filter: ProjectFilter<'_>) -> Vec<Event> {
        self.events
            .iter()
            .filter(|e| !e.is_retracted())
            .filter(|e| e.topic.as_deref() == Some(topic))
            .filter(|e| project_filter.matches(e.project_id.as_deref()))
            .cloned()
            .collect()
    }

    /// Firehose view: every event matching the project filter, newest
    /// first. Retracted events are skipped — `list_sent_by` is the
    /// only view that surfaces them, since the sender wants to know
    /// what they unsent.
    pub fn list_all(&self, project_filter: ProjectFilter<'_>, limit: Option<usize>) -> Vec<Event> {
        let iter = self
            .events
            .iter()
            .rev()
            .filter(|e| !e.is_retracted())
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

    /// True when `recipient` is allowed to mutate ReadState for
    /// `event_id`. Permitted when:
    ///
    /// - `event.to == recipient` (direct mail), or
    /// - `event.to.is_none()` and the event has a topic (pure bus
    ///   broadcast — anyone can track their own state), or
    /// - the recipient is subscribed to a pattern matching the event's
    ///   topic (delivery via subscription).
    ///
    /// The caller threads `subscribed_patterns` so the store stays
    /// dependency-free of the subscription module.
    fn recipient_owns(
        &self,
        event_id: &str,
        recipient: &str,
        subscribed_patterns: &[String],
    ) -> bool {
        let Some(event) = self.get(event_id) else {
            return false;
        };
        match event.to.as_deref() {
            Some(addressed) if addressed == recipient => true,
            Some(_) => topic_matches_any(event.topic.as_deref(), subscribed_patterns),
            // Pure topic events have no addressed recipient; any caller
            // can track their own read state against them.
            None => true,
        }
    }

    /// Idempotently mark `event_id` as read for `recipient`. Returns true
    /// when state changed (i.e. it wasn't already read). Returns false
    /// when `recipient` doesn't own the event — that prevents a caller
    /// from creating a bogus ReadState row that `list_sent_by` would
    /// later report back to the sender as if a stranger had read their
    /// direct mail.
    pub fn mark_read(
        &mut self,
        event_id: &str,
        recipient: &str,
        subscribed_patterns: &[String],
        now_ms: u64,
    ) -> bool {
        if !self.recipient_owns(event_id, recipient, subscribed_patterns) {
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
    /// Refuses to mutate state for callers that don't own the event (see
    /// `mark_read`).
    pub fn ack(
        &mut self,
        event_id: &str,
        recipient: &str,
        subscribed_patterns: &[String],
        result: Option<String>,
        now_ms: u64,
    ) -> bool {
        if !self.recipient_owns(event_id, recipient, subscribed_patterns) {
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

    pub fn unread_count(
        &self,
        recipient: &str,
        subscribed_patterns: &[String],
        project_filter: ProjectFilter<'_>,
    ) -> usize {
        self.events
            .iter()
            .filter(|e| !e.is_retracted())
            .filter(|e| event_visible_to(e, recipient, subscribed_patterns))
            .filter(|e| project_filter.matches(e.project_id.as_deref()))
            .filter(|e| !self.is_cleared_by(&e.id, recipient))
            .filter(|e| !self.is_read_by(&e.id, recipient))
            .count()
    }

    /// Mark read `ReadState` rows for `recipient` as cleared, scoped to
    /// the project filter so a clear action in project A doesn't blow
    /// away read state for an alias of the same name in project B.
    /// Cleared events drop out of `list_for_recipient` and don't count
    /// as unread; the underlying events are preserved so other
    /// recipients still see them. Returns the count of newly-cleared
    /// rows.
    ///
    /// We deliberately *do not* delete `ReadState` rows — that would
    /// erase `read_at` and the next `list_for_recipient` call would
    /// re-surface the events as unread (the symptom of the prior bug).
    /// Adding a separate `cleared_at` marker keeps the prior read state
    /// intact while making the rows invisible to the recipient.
    pub fn clear_read(
        &mut self,
        recipient: &str,
        project_filter: ProjectFilter<'_>,
        now_ms: u64,
    ) -> usize {
        // Build the set of event ids that match the project filter so we
        // can scope the ReadState mutation. Iterating `read_state` and
        // event lookups separately keeps the borrow checker happy.
        let scoped_event_ids: std::collections::HashSet<&String> = self
            .events
            .iter()
            .filter(|e| project_filter.matches(e.project_id.as_deref()))
            .map(|e| &e.id)
            .collect();

        let mut cleared = 0;
        for ((r, eid), state) in self.read_state.iter_mut() {
            if r == recipient
                && state.read_at.is_some()
                && state.cleared_at.is_none()
                && scoped_event_ids.contains(eid)
            {
                state.cleared_at = Some(now_ms);
                cleared += 1;
            }
        }
        cleared
    }

    /// Retract (unsend) `event_id` on behalf of `caller`. Only the
    /// original sender may retract, and only before any recipient has
    /// acked. Sets `retracted_at = Some(now_ms)` on the in-memory
    /// event and returns the updated copy.
    pub fn retract(
        &mut self,
        event_id: &str,
        caller: &str,
        now_ms: u64,
    ) -> Result<Event, RetractError> {
        // Look up the event first; emit a precise error before any
        // mutation so the manager can persist or skip cleanly.
        let any_acked = self
            .read_state
            .iter()
            .any(|((_, eid), s)| eid == event_id && s.acked_at.is_some());
        let event = self
            .events
            .iter_mut()
            .find(|e| e.id == event_id)
            .ok_or_else(|| RetractError::NotFound(event_id.to_string()))?;
        if event.from.as_deref() != Some(caller) {
            return Err(RetractError::NotSender {
                event_id: event_id.to_string(),
                event_sender: event.from.clone(),
                caller: caller.to_string(),
            });
        }
        if event.is_retracted() {
            return Err(RetractError::AlreadyRetracted(event_id.to_string()));
        }
        if any_acked {
            return Err(RetractError::AlreadyAcked(event_id.to_string()));
        }
        event.retracted_at = Some(now_ms);
        Ok(event.clone())
    }

    /// Apply a retracted_at timestamp to an existing event without
    /// any caller/ack checks. Used by the persistence loader to
    /// replay retract marker rows. Returns true when the event
    /// existed and was updated; false silently when the event isn't
    /// in memory (the marker was for an evicted-from-RAM event).
    pub fn apply_retract_marker(&mut self, event_id: &str, retracted_at: u64) -> bool {
        if let Some(event) = self.events.iter_mut().find(|e| e.id == event_id) {
            event.retracted_at = Some(retracted_at);
            return true;
        }
        false
    }

    /// Mutable lookup by id. Used by the manager to roll back an
    /// in-memory retract when the persistence write fails.
    pub fn events_mut_find(&mut self, event_id: &str) -> Option<&mut Event> {
        self.events.iter_mut().find(|e| e.id == event_id)
    }

    /// Dismiss `event_id` from `recipient`'s inbox view. Unlike
    /// `clear_read` (which acts on a project-scoped batch of *read*
    /// rows), this targets a single event regardless of read state.
    /// `recipient_owns` still gatekeeps so a stranger can't fabricate
    /// a ReadState row that `list_sent_by` would surface back to the
    /// sender.
    ///
    /// Returns `true` when the cleared_at flag flipped (false when
    /// already cleared, when the recipient doesn't own the event,
    /// or when the event itself doesn't exist).
    pub fn dismiss(
        &mut self,
        event_id: &str,
        recipient: &str,
        subscribed_patterns: &[String],
        now_ms: u64,
    ) -> bool {
        if !self.recipient_owns(event_id, recipient, subscribed_patterns) {
            return false;
        }
        let state = self.read_state_mut_or_insert(event_id, recipient);
        if state.cleared_at.is_some() {
            return false;
        }
        state.cleared_at = Some(now_ms);
        true
    }
}

impl Default for EventStore {
    fn default() -> Self {
        Self::new()
    }
}

/// True if `event` is visible to `recipient` — either addressed
/// directly or matches a subscription pattern. Hot-path helper kept
/// adjacent to `EventStore` so the visibility rule lives in one place.
fn event_visible_to(event: &Event, recipient: &str, subscribed_patterns: &[String]) -> bool {
    if event.to.as_deref() == Some(recipient) {
        return true;
    }
    topic_matches_any(event.topic.as_deref(), subscribed_patterns)
}

fn topic_matches_any(topic: Option<&str>, patterns: &[String]) -> bool {
    let Some(t) = topic else {
        return false;
    };
    patterns.iter().any(|p| topic_matches(p, t))
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

        let mine = store.list_for_recipient("reviewer", NO_SUBSCRIPTIONS, false, ProjectFilter::Any);
        let ids: Vec<_> = mine.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e1", "e3"]);
    }

    #[test]
    fn list_for_recipient_unread_only_filters_read_events() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 2000);

        let unread = store.list_for_recipient("reviewer", NO_SUBSCRIPTIONS, true, ProjectFilter::Any);
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
            NO_SUBSCRIPTIONS,
            false,
            ProjectFilter::Exact(Some("proj-a")),
        );
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].id, "a");

        let global_only = store.list_for_recipient("reviewer", NO_SUBSCRIPTIONS, false, ProjectFilter::Exact(None));
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
        assert!(store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1000));
        assert!(!store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 2000), "second call should report no change");
        let state = store.read_state("e1", "reviewer").unwrap();
        assert_eq!(state.read_at, Some(1000), "first read_at should be preserved");
    }

    #[test]
    fn mark_read_for_unknown_event_is_noop() {
        let mut store = EventStore::new();
        assert!(!store.mark_read("nope", "reviewer", NO_SUBSCRIPTIONS, 1000));
        assert!(store.read_state("nope", "reviewer").is_none(), "no orphan state row");
    }

    #[test]
    fn ack_implies_read() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        assert!(store.ack("e1", "reviewer", NO_SUBSCRIPTIONS, Some("done".into()), 1500));
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
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1000);
        assert!(store.ack("e1", "reviewer", NO_SUBSCRIPTIONS, None, 2000));
        let state = store.read_state("e1", "reviewer").unwrap();
        assert_eq!(state.read_at, Some(1000));
        assert_eq!(state.acked_at, Some(2000));
    }

    #[test]
    fn double_ack_is_noop_unless_result_changes() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        store.ack("e1", "reviewer", NO_SUBSCRIPTIONS, Some("first".into()), 1000);
        assert!(!store.ack("e1", "reviewer", NO_SUBSCRIPTIONS, None, 2000), "redundant ack returns false");
        // Updating the result is allowed.
        assert!(store.ack("e1", "reviewer", NO_SUBSCRIPTIONS, Some("better".into()), 3000));
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
        assert_eq!(store.unread_count("reviewer", NO_SUBSCRIPTIONS, ProjectFilter::Any), 2);
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1500);
        assert_eq!(store.unread_count("reviewer", NO_SUBSCRIPTIONS, ProjectFilter::Any), 1);
    }

    #[test]
    fn clear_read_marks_cleared_without_resurrecting_unread() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        post_to(&mut store, "e3", "builder", "c", None, None);
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1000);
        store.mark_read("e3", "builder", NO_SUBSCRIPTIONS, 1000);

        let cleared = store.clear_read("reviewer", ProjectFilter::Any, 5000);
        assert_eq!(cleared, 1);

        // Cleared event still has its ReadState row — but with cleared_at set.
        let state = store.read_state("e1", "reviewer").unwrap();
        assert_eq!(state.read_at, Some(1000), "prior read state preserved");
        assert_eq!(state.cleared_at, Some(5000));
        assert!(state.is_cleared());

        // Cleared events drop out of list_for_recipient.
        let visible: Vec<_> = store
            .list_for_recipient("reviewer", NO_SUBSCRIPTIONS, false, ProjectFilter::Any)
            .iter()
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(visible, vec!["e2"], "e1 cleared, e2 still visible");

        // And don't show up as unread either (regression of prior bug).
        assert_eq!(store.unread_count("reviewer", NO_SUBSCRIPTIONS, ProjectFilter::Any), 1);

        // Builder's state is untouched.
        assert!(store.read_state("e3", "builder").is_some());
        // Underlying event is preserved.
        assert!(store.get("e1").is_some());
    }

    #[test]
    fn capacity_eviction_drops_orphan_read_state() {
        let mut store = EventStore::with_capacity(2, None);
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 500);
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
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1500);

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
        // Topic-only event (no `to`); any recipient can track read state
        // for it because there's no addressed owner to gatekeep against.
        // The sender then filters list_sent_by by an explicit recipient
        // to see that subscriber's view.
        let topic_event = EventBuilder::new("build green")
            .topic("repo-a.build.completed")
            .from("me")
            .kind(EventKind::Signal);
        store.post(topic_event, "e1".into(), 1000).unwrap();
        store.mark_read("e1", "qa", NO_SUBSCRIPTIONS, 1500); // qa subscribes to the topic

        let sent_qa_view = store.list_sent_by("me", Some("qa"), None);
        assert_eq!(sent_qa_view.len(), 1);
        assert!(sent_qa_view[0].1.as_ref().unwrap().is_read());
    }

    #[test]
    fn mark_read_rejects_non_addressed_recipient() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", Some("me"), None);
        // qa is NOT the addressed recipient — can't fabricate read state.
        assert!(!store.mark_read("e1", "qa", NO_SUBSCRIPTIONS, 1500));
        assert!(store.read_state("e1", "qa").is_none());
        // The actual recipient still works fine.
        assert!(store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1500));
    }

    #[test]
    fn from_entries_round_trips_events_and_state() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "a", None, None);
        post_to(&mut store, "e2", "reviewer", "b", None, None);
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1000);

        let events: Vec<_> = store.events().cloned().collect();
        let states: Vec<_> = store.all_read_state().cloned().collect();
        let restored = EventStore::from_entries(events, states);
        assert_eq!(restored.len(), 2);
        assert!(restored.read_state("e1", "reviewer").unwrap().is_read());
        assert!(restored.read_state("e2", "reviewer").is_none());
    }

    // ── Subscription-aware visibility ──────────────────────────────

    fn post_topic(store: &mut EventStore, id: &str, topic: &str) -> Event {
        let b = EventBuilder::new("body").topic(topic).kind(EventKind::Signal);
        store.post(b, id.to_string(), 1000).unwrap()
    }

    fn post_topic_to(
        store: &mut EventStore,
        id: &str,
        topic: &str,
        addressed: &str,
    ) -> Event {
        let b = EventBuilder::new("body")
            .to(addressed)
            .topic(topic)
            .kind(EventKind::Task);
        store.post(b, id.to_string(), 1000).unwrap()
    }

    #[test]
    fn list_for_recipient_unions_addressed_and_subscribed_topics() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "auditor", "direct", None, None);
        post_topic(&mut store, "e2", "repo-a.build.completed");
        post_topic(&mut store, "e3", "repo-a.build.failed");

        let patterns = vec!["**.completed".to_string()];
        let events =
            store.list_for_recipient("auditor", &patterns, false, ProjectFilter::Any);
        let ids: Vec<_> = events.iter().map(|e| e.id.clone()).collect();
        // e1 is direct mail, e2 matches the pattern, e3 doesn't.
        assert_eq!(ids, vec!["e1", "e2"]);
    }

    #[test]
    fn list_for_recipient_no_subscriptions_keeps_legacy_semantics() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "auditor", "direct", None, None);
        post_topic(&mut store, "e2", "build.completed");

        let events =
            store.list_for_recipient("auditor", NO_SUBSCRIPTIONS, false, ProjectFilter::Any);
        let ids: Vec<_> = events.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e1"], "no subs → topic event invisible");
    }

    #[test]
    fn subscriber_can_mark_read_topic_event() {
        let mut store = EventStore::new();
        post_topic(&mut store, "e1", "build.completed");

        let patterns = vec!["**.completed".to_string()];
        // Subscriber can ack/read.
        assert!(store.mark_read("e1", "auditor", &patterns, 2000));
        assert!(store.read_state("e1", "auditor").unwrap().is_read());
    }

    #[test]
    fn subscriber_can_mark_read_addressed_topic_event() {
        // Event is addressed to `reviewer` AND has a topic. Pre-fix,
        // `auditor` could not write read state because to=reviewer
        // gatekeeps. With a matching subscription, `auditor` can.
        let mut store = EventStore::new();
        post_topic_to(&mut store, "e1", "build.completed", "reviewer");

        let auditor_patterns = vec!["**.completed".to_string()];
        assert!(store.mark_read("e1", "auditor", &auditor_patterns, 2000));
        // And the addressed recipient still works without subscriptions.
        assert!(store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 2000));
    }

    #[test]
    fn non_subscriber_still_cannot_mark_read_addressed_event() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "direct", None, None);

        // `qa` has subscriptions, but none matches this event's (absent) topic.
        let patterns = vec!["**.completed".to_string()];
        assert!(!store.mark_read("e1", "qa", &patterns, 2000));
        assert!(store.read_state("e1", "qa").is_none());
    }

    #[test]
    fn unread_count_includes_subscribed_events() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "auditor", "direct", None, None);
        post_topic(&mut store, "e2", "build.completed");
        post_topic(&mut store, "e3", "build.failed");

        let patterns = vec!["**.completed".to_string()];
        let unread = store.unread_count("auditor", &patterns, ProjectFilter::Any);
        assert_eq!(unread, 2, "direct (e1) + matching topic (e2)");

        // Read the topic event; unread drops by one.
        store.mark_read("e2", "auditor", &patterns, 2000);
        assert_eq!(store.unread_count("auditor", &patterns, ProjectFilter::Any), 1);
    }

    #[test]
    fn clear_read_works_for_subscribed_topic_events() {
        let mut store = EventStore::new();
        post_topic(&mut store, "e1", "build.completed");

        let patterns = vec!["**.completed".to_string()];
        store.mark_read("e1", "auditor", &patterns, 1000);
        let cleared = store.clear_read("auditor", ProjectFilter::Any, 5000);
        assert_eq!(cleared, 1);
        assert!(store.read_state("e1", "auditor").unwrap().is_cleared());

        // Cleared topic event drops out of list_for_recipient.
        let events =
            store.list_for_recipient("auditor", &patterns, false, ProjectFilter::Any);
        assert!(events.is_empty());
    }

    // ── Retract (unsend) ───────────────────────────────────────────

    #[test]
    fn retract_sets_retracted_at_when_caller_is_sender() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", Some("me"), None);
        let updated = store.retract("e1", "me", 5000).unwrap();
        assert_eq!(updated.retracted_at, Some(5000));
        assert!(store.get("e1").unwrap().is_retracted());
    }

    #[test]
    fn retract_rejects_non_sender() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", Some("me"), None);
        let err = store.retract("e1", "stranger", 5000).unwrap_err();
        assert!(matches!(err, RetractError::NotSender { .. }));
        assert!(!store.get("e1").unwrap().is_retracted());
    }

    #[test]
    fn retract_rejects_already_acked() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", Some("me"), None);
        // Reviewer acks → sender can no longer unsend.
        store.ack("e1", "reviewer", NO_SUBSCRIPTIONS, Some("done".into()), 1000);
        let err = store.retract("e1", "me", 5000).unwrap_err();
        assert!(matches!(err, RetractError::AlreadyAcked(_)));
        assert!(!store.get("e1").unwrap().is_retracted());
    }

    #[test]
    fn retract_idempotent_returns_already_retracted() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", Some("me"), None);
        store.retract("e1", "me", 5000).unwrap();
        let err = store.retract("e1", "me", 6000).unwrap_err();
        assert!(matches!(err, RetractError::AlreadyRetracted(_)));
    }

    #[test]
    fn retract_rejects_missing_event() {
        let mut store = EventStore::new();
        let err = store.retract("ghost", "me", 5000).unwrap_err();
        assert!(matches!(err, RetractError::NotFound(_)));
    }

    #[test]
    fn retracted_event_disappears_from_recipient_inbox() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "live", Some("me"), None);
        post_to(&mut store, "e2", "reviewer", "to-retract", Some("me"), None);
        store.retract("e2", "me", 5000).unwrap();
        let inbox = store.list_for_recipient("reviewer", NO_SUBSCRIPTIONS, false, ProjectFilter::Any);
        let ids: Vec<_> = inbox.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e1"]);
    }

    #[test]
    fn retracted_event_excluded_from_unread_count_and_firehose() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "live", Some("me"), None);
        post_to(&mut store, "e2", "reviewer", "to-retract", Some("me"), None);
        store.retract("e2", "me", 5000).unwrap();
        assert_eq!(store.unread_count("reviewer", NO_SUBSCRIPTIONS, ProjectFilter::Any), 1);
        let firehose = store.list_all(ProjectFilter::Any, None);
        let ids: Vec<_> = firehose.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e1"]);
    }

    #[test]
    fn list_sent_by_still_surfaces_retracted_for_sender() {
        // The sender wants to know what they unsent. list_sent_by
        // intentionally ignores the retract filter.
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "to-retract", Some("me"), None);
        store.retract("e1", "me", 5000).unwrap();
        let sent = store.list_sent_by("me", None, None);
        assert_eq!(sent.len(), 1);
        assert!(sent[0].0.is_retracted());
    }

    #[test]
    fn apply_retract_marker_idempotently_sets_timestamp() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", Some("me"), None);
        assert!(store.apply_retract_marker("e1", 5000));
        assert_eq!(store.get("e1").unwrap().retracted_at, Some(5000));
        // Marker for an evicted/missing event is a silent no-op.
        assert!(!store.apply_retract_marker("ghost", 5000));
    }

    // ── Dismiss (per-event recipient-side hide) ────────────────────

    #[test]
    fn dismiss_sets_cleared_at_for_unread_event() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        assert!(store.dismiss("e1", "reviewer", NO_SUBSCRIPTIONS, 5000));
        assert!(store.read_state("e1", "reviewer").unwrap().is_cleared());
    }

    #[test]
    fn dismissed_event_disappears_from_inbox() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        post_to(&mut store, "e2", "reviewer", "y", None, None);
        store.dismiss("e1", "reviewer", NO_SUBSCRIPTIONS, 5000);
        let ids: Vec<_> = store
            .list_for_recipient("reviewer", NO_SUBSCRIPTIONS, false, ProjectFilter::Any)
            .iter()
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(ids, vec!["e2"]);
    }

    #[test]
    fn dismiss_rejects_non_recipient() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        // qa is not the addressed recipient and has no subscription —
        // can't fabricate a ReadState row.
        assert!(!store.dismiss("e1", "qa", NO_SUBSCRIPTIONS, 5000));
        assert!(store.read_state("e1", "qa").is_none());
    }

    #[test]
    fn dismiss_idempotent_returns_false_on_repeat() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        assert!(store.dismiss("e1", "reviewer", NO_SUBSCRIPTIONS, 5000));
        assert!(!store.dismiss("e1", "reviewer", NO_SUBSCRIPTIONS, 6000));
    }

    #[test]
    fn dismiss_works_after_read_without_overwriting_read_at() {
        let mut store = EventStore::new();
        post_to(&mut store, "e1", "reviewer", "x", None, None);
        store.mark_read("e1", "reviewer", NO_SUBSCRIPTIONS, 1000);
        assert!(store.dismiss("e1", "reviewer", NO_SUBSCRIPTIONS, 5000));
        let state = store.read_state("e1", "reviewer").unwrap();
        assert_eq!(state.read_at, Some(1000), "prior read_at preserved");
        assert_eq!(state.cleared_at, Some(5000));
    }
}
