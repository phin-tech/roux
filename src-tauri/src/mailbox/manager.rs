use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use roux_core::{Event, EventBuilder, MailboxEvent, ReadState};
use tauri::{AppHandle, Emitter};

use crate::aliases::ProjectFilter;

use super::persistence::{
    self, append_event_to, load_events_from, load_read_state_from, save_read_state_to,
};
use super::store::{EventStore, PostError};

/// Tauri event name emitted on every mailbox mutation. Frontend listens
/// here to update the inbox / firehose without polling.
pub const MAILBOX_EVENT: &str = "mailbox-event";

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
#[derive(Clone)]
pub struct MailboxManager {
    inner: Arc<Mutex<EventStore>>,
    paths: Option<Arc<MailboxPaths>>,
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
        Self {
            inner: Arc::new(Mutex::new(store)),
            paths: Some(Arc::new(MailboxPaths {
                events: events_path,
                read_state: read_state_path,
            })),
        }
    }

    /// In-memory only. No load, no persist on mutation. For tests that
    /// don't care about disk IO.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self { inner: Arc::new(Mutex::new(EventStore::new())), paths: None }
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
        if let Some(paths) = self.paths.as_ref() {
            if let Err(e) = append_event_to(&paths.events, &event) {
                eprintln!(
                    "[roux] mailbox events.jsonl append failed at {}: {e}",
                    paths.events.display()
                );
            }
        }
        if let Some(app) = app {
            let _ = app.emit(MAILBOX_EVENT, &MailboxEvent::Posted { event: event.clone() });
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
        let changed = {
            let mut store = self.inner.lock().expect("event store poisoned");
            store.mark_read(event_id, recipient, now)
        };
        if changed {
            self.persist_read_state();
            if let Some(app) = app {
                let _ = app.emit(
                    MAILBOX_EVENT,
                    &MailboxEvent::Read {
                        event_id: event_id.to_string(),
                        recipient: recipient.to_string(),
                    },
                );
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
        let changed = {
            let mut store = self.inner.lock().expect("event store poisoned");
            store.ack(event_id, recipient, result.clone(), now)
        };
        if changed {
            self.persist_read_state();
            if let Some(app) = app {
                let _ = app.emit(
                    MAILBOX_EVENT,
                    &MailboxEvent::Acked {
                        event_id: event_id.to_string(),
                        recipient: recipient.to_string(),
                        result,
                    },
                );
            }
        }
        changed
    }

    pub fn clear_read(&self, recipient: &str, app: Option<&AppHandle>) -> usize {
        let removed = {
            let mut store = self.inner.lock().expect("event store poisoned");
            store.clear_read(recipient)
        };
        if removed > 0 {
            self.persist_read_state();
            if let Some(app) = app {
                let _ = app.emit(
                    MAILBOX_EVENT,
                    &MailboxEvent::Cleared {
                        recipient: recipient.to_string(),
                        count: removed as u32,
                    },
                );
            }
        }
        removed
    }

    pub fn list_for_recipient(
        &self,
        recipient: &str,
        unread_only: bool,
        project_filter: ProjectFilter<'_>,
    ) -> Vec<Event> {
        let store = self.inner.lock().expect("event store poisoned");
        store.list_for_recipient(recipient, unread_only, project_filter)
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
        let store = self.inner.lock().expect("event store poisoned");
        store.unread_count(recipient, project_filter)
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
    fn clear_read_returns_count_and_persists() {
        let dir = tempdir().unwrap();
        let (e, r) = paths(dir.path());
        let mgr = MailboxManager::load_from(e, r);
        let e1 = mgr.post(task("a"), None).unwrap();
        let e2 = mgr.post(task("b"), None).unwrap();
        mgr.mark_read(&e1.id, "reviewer", None);
        mgr.mark_read(&e2.id, "reviewer", None);

        let removed = mgr.clear_read("reviewer", None);
        assert_eq!(removed, 2);
        assert!(mgr.read_state(&e1.id, "reviewer").is_none());
        assert!(mgr.read_state(&e2.id, "reviewer").is_none());
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
}
