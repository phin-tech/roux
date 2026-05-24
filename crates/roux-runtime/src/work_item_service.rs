//! Work item service — clonable handle over the SQLite-backed work item store.
//!
//! `WorkItemHandle` is `Clone` (Arc-based) and its methods are synchronous
//! (the store is a Mutex-guarded SQLite connection). This mirrors the
//! `MailboxManager` pattern; it is **not** a channel-based actor service and
//! is **not** added to the `RuntimeHost.services` vec.
//!
//! All mutations broadcast a `WorkItemEvent` **after** the successful write
//! so listeners receive only persisted state (persist-before-broadcast).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::broadcast;
use uuid::Uuid;

use roux_core::{WorkItem, WorkItemEvent, WorkItemInput, WorkItemStatus};

use crate::work_item_store::WorkItemStore;

const BROADCAST_CAPACITY: usize = 256;

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone)]
pub struct WorkItemHandle {
    inner: Arc<Mutex<WorkItemStore>>,
    broadcast_tx: broadcast::Sender<WorkItemEvent>,
}

impl WorkItemHandle {
    pub fn open(path: &Path) -> Result<Self, String> {
        let store =
            WorkItemStore::open(path).map_err(|e| format!("failed to open board.db: {e}"))?;
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Ok(WorkItemHandle { inner: Arc::new(Mutex::new(store)), broadcast_tx })
    }

    pub fn in_memory() -> Self {
        let store = WorkItemStore::open_in_memory().expect("in-memory SQLite should always work");
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        WorkItemHandle { inner: Arc::new(Mutex::new(store)), broadcast_tx }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<WorkItemEvent> {
        self.broadcast_tx.subscribe()
    }

    fn broadcast(&self, event: WorkItemEvent) {
        let _ = self.broadcast_tx.send(event);
    }

    pub fn list(&self, project_id: Option<&str>) -> Result<Vec<WorkItem>, String> {
        self.inner
            .lock()
            .unwrap()
            .list(project_id)
            .map_err(|e| format!("work-item list: {e}"))
    }

    pub fn get(&self, id: &str) -> Result<Option<WorkItem>, String> {
        self.inner.lock().unwrap().get(id).map_err(|e| format!("work-item get: {e}"))
    }

    pub fn create(&self, input: WorkItemInput) -> Result<WorkItem, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .create(id, input, now)
            .map_err(|e| format!("work-item create: {e}"))?;
        self.broadcast(WorkItemEvent::Created { item: item.clone() });
        Ok(item)
    }

    pub fn update(&self, id: &str, input: WorkItemInput) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .update(id, input, now)
            .map_err(|e| format!("work-item update: {e}"))?;
        if let Some(ref i) = item {
            self.broadcast(WorkItemEvent::Updated { item: i.clone() });
        }
        Ok(item)
    }

    pub fn move_item(
        &self,
        id: &str,
        status: WorkItemStatus,
        sort_order: f64,
    ) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .move_item(id, status.clone(), sort_order, now)
            .map_err(|e| format!("work-item move: {e}"))?;
        if item.is_some() {
            self.broadcast(WorkItemEvent::Moved {
                id: id.to_string(),
                status,
                sort_order,
            });
        }
        Ok(item)
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let deleted =
            self.inner.lock().unwrap().delete(id).map_err(|e| format!("work-item delete: {e}"))?;
        if deleted {
            self.broadcast(WorkItemEvent::Deleted { id: id.to_string() });
        }
        Ok(deleted)
    }

    pub fn set_session(&self, id: &str, session_id: &str) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .set_session(id, session_id, now)
            .map_err(|e| format!("work-item set-session: {e}"))?;
        if item.is_some() {
            self.broadcast(WorkItemEvent::SessionBound {
                id: id.to_string(),
                session_id: session_id.to_string(),
            });
        }
        Ok(item)
    }

    pub fn upsert_by_external(&self, input: WorkItemInput) -> Result<WorkItem, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .upsert_by_external(id, input, now)
            .map_err(|e| format!("work-item upsert: {e}"))?;
        Ok(item)
    }

    /// Insert an item without broadcasting a per-item event. Used by the
    /// import handler so the batch `Imported` event is the only signal emitted.
    pub fn insert_silent(&self, input: WorkItemInput) -> Result<WorkItem, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        self.inner
            .lock()
            .unwrap()
            .create(id, input, now)
            .map_err(|e| format!("work-item insert: {e}"))
    }

    pub fn broadcast_imported(&self, ids: Vec<String>) {
        self.broadcast(WorkItemEvent::Imported { ids });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{ExternalRef, WorkItemInput};

    fn input(title: &str) -> WorkItemInput {
        WorkItemInput { title: title.to_string(), ..Default::default() }
    }

    #[test]
    fn create_and_list() {
        let handle = WorkItemHandle::in_memory();
        handle.create(input("Task A")).unwrap();
        handle.create(input("Task B")).unwrap();
        let items = handle.list(None).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn create_broadcasts_event() {
        let handle = WorkItemHandle::in_memory();
        let mut rx = handle.subscribe_events();

        handle.create(input("Task")).unwrap();

        let event = rx.try_recv().expect("Created event should be broadcast");
        assert!(matches!(event, WorkItemEvent::Created { .. }));
    }

    #[test]
    fn delete_broadcasts_event() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let mut rx = handle.subscribe_events();

        let deleted = handle.delete(&item.id).unwrap();
        assert!(deleted);

        let event = rx.try_recv().expect("Deleted event should be broadcast");
        assert!(matches!(event, WorkItemEvent::Deleted { .. }));
    }

    #[test]
    fn move_item_broadcasts_moved_event() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        // Subscribe after create so the create broadcast is already gone.
        let mut rx = handle.subscribe_events();

        handle.move_item(&item.id, WorkItemStatus::Doing, 1.0).unwrap();

        // The only event in the channel is the Moved event.
        let event = rx.try_recv().expect("Moved event should be broadcast");
        assert!(matches!(event, WorkItemEvent::Moved { .. }));
    }

    #[test]
    fn set_session_broadcasts_session_bound() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let mut rx = handle.subscribe_events();

        handle.set_session(&item.id, "sess-1").unwrap();

        let event = rx.try_recv().expect("SessionBound event should be broadcast");
        assert!(matches!(event, WorkItemEvent::SessionBound { .. }));
    }

    #[test]
    fn upsert_by_external_no_duplicate() {
        let handle = WorkItemHandle::in_memory();
        let ext = WorkItemInput {
            title: "First".into(),
            external_ref: Some(ExternalRef {
                provider: "test".into(),
                external_id: "x-1".into(),
                url: None,
            }),
            ..Default::default()
        };
        handle.upsert_by_external(ext.clone()).unwrap();
        let mut ext2 = ext;
        ext2.title = "Updated".into();
        handle.upsert_by_external(ext2).unwrap();

        let items = handle.list(None).unwrap();
        assert_eq!(items.len(), 1, "no duplicate on re-import");
        assert_eq!(items[0].title, "Updated");
    }
}
