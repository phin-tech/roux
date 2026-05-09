use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use roux_core::{Event, EventBuilder, MailboxEvent, ReadState};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

use crate::aliases::ProjectFilter;
use crate::subscriptions::SubscriptionManager;

use super::persistence::{
    self, append_event_to, load_events_from, load_read_state_from, save_read_state_to,
};
use super::store::{EventStore, PostError};

/// Tauri event name emitted on every mailbox mutation. Frontend listens
/// here to update the inbox / firehose without polling.
pub const MAILBOX_EVENT: &str = "mailbox-event";

/// In-process broadcast capacity. Each `mailbox watch` socket connection
/// holds one receiver. 256 buffered events is well above any realistic
/// burst — if a slow consumer ever falls behind, `broadcast::Receiver`
/// surfaces a `Lagged` error and the watch handler can decide whether
/// to reconnect or drop.
const BROADCAST_CAPACITY: usize = 256;

struct MailboxPaths {
    events: PathBuf,
    read_state: PathBuf,
}

/// Clonable handle over the categorical event store. Persistence layout:
///
/// - On `post`: append a single row to `events.jsonl` (cheap, ordered).
/// - On `mark_read` / `ack` / `clear_read`: full rewrite of
///   `read_state.json`. The state file stays small even at developer-tool
///   scale (a few thousand rows max).
///
/// The events file grows monotonically — the in-memory store applies
/// retention caps at load time, but the on-disk audit log is preserved.
///
/// `subscriptions` is optional so test fixtures and other call paths can
/// construct a manager without a live subscription store. When absent,
/// recipient pattern lookup is empty (legacy exact-match semantics).
///
/// `broadcast_tx` is always present and fires on every mutation so
/// in-process consumers (`mailbox watch` socket handler, future
/// internal listeners) can subscribe without going through Tauri.
#[derive(Clone)]
pub struct MailboxManager {
    inner: Arc<Mutex<EventStore>>,
    paths: Option<Arc<MailboxPaths>>,
    subscriptions: Option<SubscriptionManager>,
    broadcast_tx: broadcast::Sender<MailboxEvent>,
}

impl MailboxManager {
    /// Production constructor: loads from the default config-dir paths.
    pub fn load() -> Self {
        Self::load_from(persistence::events_path(), persistence::read_state_path())
    }

    /// Load from explicit paths. Used by tests with a tempdir.
    pub fn load_from(events_path: PathBuf, read_state_path: PathBuf) -> Self {
        let events = load_events_from(&events_path);
        let read_state = load_read_state_from(&read_state_path);
        let store = EventStore::from_entries(events, read_state);
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            inner: Arc::new(Mutex::new(store)),
            paths: Some(Arc::new(MailboxPaths {
                events: events_path,
                read_state: read_state_path,
            })),
            subscriptions: None,
            broadcast_tx,
        }
    }

    /// Wire the subscription manager so topic-matched events become
    /// visible / ack-able to subscribers. Returns `self` for chaining
    /// at construction sites in `main.rs`.
    pub fn with_subscriptions(mut self, subscriptions: SubscriptionManager) -> Self {
        self.subscriptions = Some(subscriptions);
        self
    }

    /// In-memory only. No load, no persist on mutation. For tests that
    /// don't care about disk IO.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            inner: Arc::new(Mutex::new(EventStore::new())),
            paths: None,
            subscriptions: None,
            broadcast_tx,
        }
    }

    /// Subscribe to in-process mailbox events. Each call returns a
    /// fresh `broadcast::Receiver` — caller drops it when done. Used by
    /// the `mailbox watch` socket handler to push events to a long-
    /// lived CLI client without polling.
    pub fn subscribe_events(&self) -> broadcast::Receiver<MailboxEvent> {
        self.broadcast_tx.subscribe()
    }

    /// Send a `MailboxEvent` to in-process subscribers. Failures (no
    /// receivers, lagged consumer) are intentionally ignored — the Tauri
    /// emit and persistence are the durable paths; this channel is a
    /// best-effort fast path for live consumers.
    fn broadcast(&self, event: &MailboxEvent) {
        let _ = self.broadcast_tx.send(event.clone());
    }

    /// Patterns subscribed to by `recipient` in scopes compatible with
    /// `project_filter`. Returns an empty vec when no subscription
    /// manager is wired or the recipient has no subscriptions.
    ///
    /// When `project_filter` is `Any`, returns global subscriptions
    /// (`project_id == None`) only — scoped patterns must NOT cross
    /// project boundaries even on broad list calls. The `list_for_recipient`
    /// callers that need cross-project visibility iterate per-event scope
    /// via `patterns_for_event`.
    fn patterns_for(&self, recipient: &str, project_filter: ProjectFilter<'_>) -> Vec<String> {
        let Some(subs) = self.subscriptions.as_ref() else {
            return Vec::new();
        };
        match project_filter {
            ProjectFilter::Any => subs.patterns_for_alias(recipient, None),
            ProjectFilter::Exact(scope) => subs.patterns_for_alias(recipient, scope),
        }
    }

    /// Patterns for `recipient` scoped to a specific event. Used by the
    /// per-event ownership check (`mark_read`, `ack`) so a `p2`-scoped
    /// subscription can't authorize ReadState writes against a `p1`
    /// event. Caller passes the event so we can derive its `project_id`
    /// from a single `store.get` rather than asking the caller to.
    fn patterns_for_event(&self, recipient: &str, event: &Event) -> Vec<String> {
        let Some(subs) = self.subscriptions.as_ref() else {
            return Vec::new();
        };
        subs.patterns_for_alias(recipient, event.project_id.as_deref())
    }

    pub fn post(
        &self,
        builder: EventBuilder,
        app: Option<&AppHandle>,
    ) -> Result<Event, PostError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let event = {
            let mut store = self.inner.lock().expect("event store poisoned");
            store.post(builder, id, now)?
        };
        // Persist BEFORE emitting the success event. The mailbox's
        // durable-queue contract is the whole point — if the event
        // doesn't hit disk, the UI mustn't observe a "Posted" that
        // disappears on restart. Roll the in-memory event back on
        // persistence failure and surface the IO error.
        if let Some(paths) = self.paths.as_ref() {
            if let Err(e) = append_event_to(&paths.events, &event) {
                let mut store = self.inner.lock().expect("event store poisoned");
                store.remove_event(&event.id);
                return Err(PostError::Persist(format!(
                    "events.jsonl append failed at {}: {e}",
                    paths.events.display()
                )));
            }
        }
        let posted = MailboxEvent::Posted { event: event.clone() };
        self.broadcast(&posted);
        if let Some(app) = app {
            let _ = app.emit(MAILBOX_EVENT, &posted);
        }
        // Topic-event subscriptions: notify each matching subscriber.
        // Frontend uses these to bump the subscriber alias's unread
        // count and surface the delivery without a new mailbox row.
        // CLI watchers see the same events through `subscribe_events()`.
        if let (Some(topic), Some(subs)) = (event.topic.as_deref(), self.subscriptions.as_ref()) {
            for sub in subs.matching_topic(topic, event.project_id.as_deref()) {
                let delivered = MailboxEvent::TopicDelivered {
                    event_id: event.id.clone(),
                    recipient: sub.alias,
                    subscription_id: sub.id,
                };
                self.broadcast(&delivered);
                if let Some(app) = app {
                    let _ = app.emit(MAILBOX_EVENT, &delivered);
                }
            }
        }
        Ok(event)
    }

    pub fn mark_read(
        &self,
        event_id: &str,
        recipient: &str,
        app: Option<&AppHandle>,
    ) -> bool {
        let now = now_ms();
        // Patterns are scoped to the event's own project so a recipient's
        // p2-scoped subscription can't authorize ReadState writes against
        // a p1-scoped event. Empty if the event doesn't exist — store will
        // then refuse the write either way.
        let patterns = self
            .get(event_id)
            .map(|e| self.patterns_for_event(recipient, &e))
            .unwrap_or_default();
        let changed = {
            let mut store = self.inner.lock().expect("event store poisoned");
            store.mark_read(event_id, recipient, &patterns, now)
        };
        if changed {
            self.persist_read_state();
            let evt = MailboxEvent::Read {
                event_id: event_id.to_string(),
                recipient: recipient.to_string(),
            };
            self.broadcast(&evt);
            if let Some(app) = app {
                let _ = app.emit(MAILBOX_EVENT, &evt);
            }
        }
        changed
    }

    pub fn ack(
        &self,
        event_id: &str,
        recipient: &str,
        result: Option<String>,
        app: Option<&AppHandle>,
    ) -> bool {
        let now = now_ms();
        let patterns = self
            .get(event_id)
            .map(|e| self.patterns_for_event(recipient, &e))
            .unwrap_or_default();
        let changed = {
            let mut store = self.inner.lock().expect("event store poisoned");
            store.ack(event_id, recipient, &patterns, result.clone(), now)
        };
        if changed {
            self.persist_read_state();
            let evt = MailboxEvent::Acked {
                event_id: event_id.to_string(),
                recipient: recipient.to_string(),
                result,
            };
            self.broadcast(&evt);
            if let Some(app) = app {
                let _ = app.emit(MAILBOX_EVENT, &evt);
            }
        }
        changed
    }

    pub fn clear_read(
        &self,
        recipient: &str,
        project_filter: ProjectFilter<'_>,
        app: Option<&AppHandle>,
    ) -> usize {
        let now = now_ms();
        let cleared = {
            let mut store = self.inner.lock().expect("event store poisoned");
            store.clear_read(recipient, project_filter, now)
        };
        if cleared > 0 {
            self.persist_read_state();
            let evt = MailboxEvent::Cleared {
                recipient: recipient.to_string(),
                count: cleared as u32,
            };
            self.broadcast(&evt);
            if let Some(app) = app {
                let _ = app.emit(MAILBOX_EVENT, &evt);
            }
        }
        cleared
    }

    pub fn list_for_recipient(
        &self,
        recipient: &str,
        unread_only: bool,
        project_filter: ProjectFilter<'_>,
    ) -> Vec<Event> {
        let patterns = self.patterns_for(recipient, project_filter);
        let store = self.inner.lock().expect("event store poisoned");
        store.list_for_recipient(recipient, &patterns, unread_only, project_filter)
    }

    pub fn list_for_topic(
        &self,
        topic: &str,
        project_filter: ProjectFilter<'_>,
    ) -> Vec<Event> {
        let store = self.inner.lock().expect("event store poisoned");
        store.list_for_topic(topic, project_filter)
    }

    pub fn list_all(
        &self,
        project_filter: ProjectFilter<'_>,
        limit: Option<usize>,
    ) -> Vec<Event> {
        let store = self.inner.lock().expect("event store poisoned");
        store.list_all(project_filter, limit)
    }

    pub fn list_sent_by(
        &self,
        sender: &str,
        recipient_filter: Option<&str>,
        limit: Option<usize>,
    ) -> Vec<(Event, Option<ReadState>)> {
        let store = self.inner.lock().expect("event store poisoned");
        store.list_sent_by(sender, recipient_filter, limit)
    }

    pub fn unread_count(&self, recipient: &str, project_filter: ProjectFilter<'_>) -> usize {
        let patterns = self.patterns_for(recipient, project_filter);
        let store = self.inner.lock().expect("event store poisoned");
        store.unread_count(recipient, &patterns, project_filter)
    }

    pub fn get(&self, event_id: &str) -> Option<Event> {
        let store = self.inner.lock().expect("event store poisoned");
        store.get(event_id).cloned()
    }

    pub fn read_state(&self, event_id: &str, recipient: &str) -> Option<ReadState> {
        let store = self.inner.lock().expect("event store poisoned");
        store.read_state(event_id, recipient).cloned()
    }

    fn persist_read_state(&self) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let states: Vec<ReadState> = {
            let store = self.inner.lock().expect("event store poisoned");
            store.all_read_state().cloned().collect()
        };
        if let Err(e) = save_read_state_to(&paths.read_state, &states) {
            eprintln!(
                "[roux] mailbox read_state.json save failed at {}: {e}",
                paths.read_state.display()
            );
        }
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
    use tempfile::tempdir;

    fn paths(dir: &std::path::Path) -> (PathBuf, PathBuf) {
        (dir.join("events.jsonl"), dir.join("read_state.json"))
    }

    fn task(body: &str) -> EventBuilder {
        EventBuilder::new(body).to("reviewer").from("me").kind(EventKind::Task)
    }

    #[test]
    fn post_persists_to_events_jsonl() {
        let dir = tempdir().unwrap();
        let (e, r) = paths(dir.path());
        let mgr = MailboxManager::load_from(e.clone(), r);
        let event = mgr.post(task("hello"), None).unwrap();

        let raw = std::fs::read_to_string(&e).unwrap();
        assert!(raw.contains(&event.id), "events.jsonl must contain the new event id");
    }

    #[test]
    fn post_round_trips_through_disk_reload() {
        let dir = tempdir().unwrap();
        let (e, r) = paths(dir.path());
        let mgr1 = MailboxManager::load_from(e.clone(), r.clone());
        mgr1.post(task("first"), None).unwrap();
        mgr1.post(task("second"), None).unwrap();

        let mgr2 = MailboxManager::load_from(e, r);
        let mine = mgr2.list_for_recipient("reviewer", false, ProjectFilter::Any);
        assert_eq!(mine.len(), 2);
        let bodies: Vec<_> = mine.iter().map(|e| e.body.clone()).collect();
        assert_eq!(bodies, vec!["first", "second"]);
    }

    #[test]
    fn mark_read_persists_and_survives_reload() {
        let dir = tempdir().unwrap();
        let (e, r) = paths(dir.path());
        let mgr1 = MailboxManager::load_from(e.clone(), r.clone());
        let event = mgr1.post(task("hello"), None).unwrap();
        assert!(mgr1.mark_read(&event.id, "reviewer", None));

        let mgr2 = MailboxManager::load_from(e, r);
        let state = mgr2.read_state(&event.id, "reviewer").unwrap();
        assert!(state.is_read());
    }

    #[test]
    fn ack_persists_with_result_string() {
        let dir = tempdir().unwrap();
        let (e, r) = paths(dir.path());
        let mgr1 = MailboxManager::load_from(e.clone(), r.clone());
        let event = mgr1.post(task("hello"), None).unwrap();
        assert!(mgr1.ack(&event.id, "reviewer", Some("done!".into()), None));

        let mgr2 = MailboxManager::load_from(e, r);
        let state = mgr2.read_state(&event.id, "reviewer").unwrap();
        assert!(state.is_acked());
        assert_eq!(state.ack_result.as_deref(), Some("done!"));
    }

    #[test]
    fn clear_read_marks_cleared_and_persists() {
        let dir = tempdir().unwrap();
        let (e, r) = paths(dir.path());
        let mgr = MailboxManager::load_from(e, r);
        let e1 = mgr.post(task("a"), None).unwrap();
        let e2 = mgr.post(task("b"), None).unwrap();
        mgr.mark_read(&e1.id, "reviewer", None);
        mgr.mark_read(&e2.id, "reviewer", None);

        let cleared = mgr.clear_read("reviewer", ProjectFilter::Any, None);
        assert_eq!(cleared, 2);
        // Cleared events keep their ReadState (with cleared_at set) so
        // they don't re-surface as unread on the next list call.
        assert!(mgr.read_state(&e1.id, "reviewer").unwrap().is_cleared());
        assert!(mgr.read_state(&e2.id, "reviewer").unwrap().is_cleared());
    }

    #[test]
    fn unread_count_excludes_read() {
        let mgr = MailboxManager::in_memory();
        let e1 = mgr.post(task("a"), None).unwrap();
        let _e2 = mgr.post(task("b"), None).unwrap();
        assert_eq!(mgr.unread_count("reviewer", ProjectFilter::Any), 2);
        mgr.mark_read(&e1.id, "reviewer", None);
        assert_eq!(mgr.unread_count("reviewer", ProjectFilter::Any), 1);
    }

    #[test]
    fn list_all_returns_newest_first() {
        let mgr = MailboxManager::in_memory();
        mgr.post(task("first"), None).unwrap();
        mgr.post(task("second"), None).unwrap();
        let all = mgr.list_all(ProjectFilter::Any, None);
        assert_eq!(all[0].body, "second");
    }

    #[test]
    fn redundant_mark_read_does_not_persist() {
        let dir = tempdir().unwrap();
        let (e, r) = paths(dir.path());
        let mgr = MailboxManager::load_from(e, r.clone());
        let event = mgr.post(task("hello"), None).unwrap();
        mgr.mark_read(&event.id, "reviewer", None);
        let mtime_before = std::fs::metadata(&r).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Already read — should be a no-op (returns false).
        assert!(!mgr.mark_read(&event.id, "reviewer", None));
        let mtime_after = std::fs::metadata(&r).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after, "no-op mark_read must not rewrite read_state.json");
    }

    // ── Subscription wiring ─────────────────────────────────────────

    #[test]
    fn subscriber_sees_topic_event_in_inbox() {
        let mgr = MailboxManager::in_memory()
            .with_subscriptions(SubscriptionManager::in_memory());
        // Set up the subscription via the wired manager so the manager
        // sees it. Need to grab the inner manager handle for that.
        // (For tests we reach in via a helper.)
        let subs = mgr.subscriptions.as_ref().unwrap();
        subs.subscribe("auditor", "**.completed", None, None).unwrap();

        // Publish a topic event.
        let topic_event = EventBuilder::new("main is green")
            .topic("repo-a.build.completed")
            .from("builder")
            .kind(EventKind::Signal);
        mgr.post(topic_event, None).unwrap();

        // Auditor's inbox includes the topic event.
        let mine = mgr.list_for_recipient("auditor", false, ProjectFilter::Any);
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].topic.as_deref(), Some("repo-a.build.completed"));
    }

    #[test]
    fn unsubscribed_recipient_does_not_see_topic_events() {
        let mgr = MailboxManager::in_memory()
            .with_subscriptions(SubscriptionManager::in_memory());

        let topic_event = EventBuilder::new("hi")
            .topic("repo-a.build.completed")
            .kind(EventKind::Signal);
        mgr.post(topic_event, None).unwrap();

        let mine = mgr.list_for_recipient("auditor", false, ProjectFilter::Any);
        assert!(mine.is_empty());
    }

    #[test]
    fn subscriber_unread_count_includes_topic_matches() {
        let mgr = MailboxManager::in_memory()
            .with_subscriptions(SubscriptionManager::in_memory());
        mgr.subscriptions
            .as_ref()
            .unwrap()
            .subscribe("auditor", "**.completed", None, None)
            .unwrap();

        let topic_event = EventBuilder::new("hi")
            .topic("build.completed")
            .kind(EventKind::Signal);
        let event = mgr.post(topic_event, None).unwrap();

        assert_eq!(mgr.unread_count("auditor", ProjectFilter::Any), 1);
        // Auditor reads the topic event via the manager (subscription
        // ownership lets them).
        assert!(mgr.mark_read(&event.id, "auditor", None));
        assert_eq!(mgr.unread_count("auditor", ProjectFilter::Any), 0);
    }

    #[test]
    fn project_scoped_subscription_only_matches_in_scope() {
        let mgr = MailboxManager::in_memory()
            .with_subscriptions(SubscriptionManager::in_memory());
        mgr.subscriptions
            .as_ref()
            .unwrap()
            .subscribe("auditor", "*", Some("p1".into()), None)
            .unwrap();

        // Event in p1: matches.
        let in_p1 = EventBuilder::new("a")
            .topic("foo")
            .project_id("p1")
            .kind(EventKind::Signal);
        mgr.post(in_p1, None).unwrap();

        // Event in p2: must not match.
        let in_p2 = EventBuilder::new("b")
            .topic("foo")
            .project_id("p2")
            .kind(EventKind::Signal);
        mgr.post(in_p2, None).unwrap();

        let visible_p1 = mgr.list_for_recipient(
            "auditor",
            false,
            ProjectFilter::Exact(Some("p1")),
        );
        assert_eq!(visible_p1.len(), 1);

        let visible_p2 = mgr.list_for_recipient(
            "auditor",
            false,
            ProjectFilter::Exact(Some("p2")),
        );
        assert!(visible_p2.is_empty());
    }

    /// Regression for the cross-project authorization bug: a p2-scoped
    /// subscription must NOT let the recipient mark p1 events read.
    /// Pre-fix `patterns_for(Any)` flattened all the recipient's
    /// patterns and the store's ownership check granted access whenever
    /// any pattern matched the topic, regardless of project.
    #[test]
    fn cross_project_subscription_cannot_authorize_other_project_events() {
        let mgr = MailboxManager::in_memory()
            .with_subscriptions(SubscriptionManager::in_memory());
        // Auditor only subscribes inside p2.
        mgr.subscriptions
            .as_ref()
            .unwrap()
            .subscribe("auditor", "*", Some("p2".into()), None)
            .unwrap();

        // Bob posts a topic event in p1, addressed to reviewer (not auditor).
        let event = mgr
            .post(
                EventBuilder::new("p1 work")
                    .to("reviewer")
                    .topic("foo")
                    .from("bob")
                    .project_id("p1")
                    .kind(EventKind::Task),
                None,
            )
            .unwrap();

        // Auditor must NOT be able to mark this read — their p2 sub
        // doesn't apply to p1 events.
        assert!(
            !mgr.mark_read(&event.id, "auditor", None),
            "p2-scoped subscription must not authorize p1 event ownership",
        );
        assert!(mgr.read_state(&event.id, "auditor").is_none());
        assert!(
            !mgr.ack(&event.id, "auditor", Some("done".into()), None),
            "p2-scoped subscription must not authorize p1 event ack",
        );
    }

    // ── In-process broadcast (powers `mailbox watch`) ──────────────

    #[tokio::test]
    async fn broadcast_fires_on_post() {
        let mgr = MailboxManager::in_memory();
        let mut rx = mgr.subscribe_events();
        mgr.post(task("hello"), None).unwrap();
        match rx.recv().await.expect("broadcast must deliver") {
            MailboxEvent::Posted { event } => assert_eq!(event.body, "hello"),
            other => panic!("expected Posted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn broadcast_fires_topic_delivered_for_each_subscription() {
        let mgr = MailboxManager::in_memory()
            .with_subscriptions(SubscriptionManager::in_memory());
        mgr.subscriptions
            .as_ref()
            .unwrap()
            .subscribe("auditor", "**.completed", None, None)
            .unwrap();
        mgr.subscriptions
            .as_ref()
            .unwrap()
            .subscribe("watcher", "**", None, None)
            .unwrap();

        let mut rx = mgr.subscribe_events();
        let topic_event = EventBuilder::new("a")
            .topic("build.completed")
            .kind(EventKind::Signal);
        mgr.post(topic_event, None).unwrap();

        // First message: Posted. Then one TopicDelivered per matching sub.
        let mut delivered_recipients: Vec<String> = Vec::new();
        for _ in 0..3 {
            match rx.recv().await {
                Ok(MailboxEvent::Posted { .. }) => {}
                Ok(MailboxEvent::TopicDelivered { recipient, .. }) => {
                    delivered_recipients.push(recipient);
                }
                Ok(other) => panic!("unexpected {other:?}"),
                Err(e) => panic!("recv failed: {e:?}"),
            }
        }
        delivered_recipients.sort();
        assert_eq!(
            delivered_recipients,
            vec!["auditor".to_string(), "watcher".to_string()]
        );
    }

    #[tokio::test]
    async fn broadcast_fires_on_mark_read_and_ack() {
        let mgr = MailboxManager::in_memory();
        let event = mgr.post(task("hi"), None).unwrap();

        let mut rx = mgr.subscribe_events();
        mgr.mark_read(&event.id, "reviewer", None);
        mgr.ack(&event.id, "reviewer", Some("done".into()), None);

        let first = rx.recv().await.unwrap();
        assert!(matches!(first, MailboxEvent::Read { .. }));
        let second = rx.recv().await.unwrap();
        assert!(matches!(second, MailboxEvent::Acked { .. }));
    }

    #[tokio::test]
    async fn broadcast_each_subscriber_gets_independent_stream() {
        let mgr = MailboxManager::in_memory();
        let mut rx_a = mgr.subscribe_events();
        let mut rx_b = mgr.subscribe_events();
        mgr.post(task("hi"), None).unwrap();
        assert!(matches!(rx_a.recv().await.unwrap(), MailboxEvent::Posted { .. }));
        assert!(matches!(rx_b.recv().await.unwrap(), MailboxEvent::Posted { .. }));
    }
}
