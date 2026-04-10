use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use roux_core::{Notification, NotificationEvent, NotificationRequest, NotificationSource};

use super::store::NotificationStore;

/// Tauri event name used for all notification store mutations.
pub const NOTIFICATION_EVENT: &str = "notification-event";

/// Clonable handle over the notification store. Wraps an `Arc<Mutex<_>>`
/// internally so all clones share state. Meant to live inside `AppState`.
#[derive(Clone)]
pub struct NotificationManager {
    inner: Arc<Mutex<NotificationStore>>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(NotificationStore::new())),
        }
    }

    /// Push a new notification and emit an `Added` event to the frontend.
    /// Returns the stored notification (with id + created_at filled in).
    pub fn push(&self, req: NotificationRequest, app: Option<&AppHandle>) -> Notification {
        let notification = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.push(req)
        };
        if let Some(app) = app {
            let _ = app.emit(
                NOTIFICATION_EVENT,
                &NotificationEvent::Added {
                    notification: notification.clone(),
                },
            );
        }
        notification
    }

    pub fn list(&self) -> Vec<Notification> {
        let store = self.inner.lock().expect("notification store poisoned");
        store.list()
    }

    pub fn list_for_session(&self, session_id: Option<&str>) -> Vec<Notification> {
        let store = self.inner.lock().expect("notification store poisoned");
        store.list_for_session(session_id)
    }

    pub fn unread_count(&self, session_filter: Option<Option<&str>>) -> usize {
        let store = self.inner.lock().expect("notification store poisoned");
        store.unread_count(session_filter)
    }

    pub fn mark_read(&self, id: &str, app: Option<&AppHandle>) -> bool {
        let changed = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.mark_read(id)
        };
        if changed {
            if let Some(app) = app {
                let _ = app.emit(
                    NOTIFICATION_EVENT,
                    &NotificationEvent::Read { id: id.to_string() },
                );
            }
        }
        changed
    }

    pub fn mark_all_read(
        &self,
        session_filter: Option<Option<&str>>,
        app: Option<&AppHandle>,
    ) -> usize {
        let marked = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.mark_all_read(session_filter)
        };
        if marked > 0 {
            if let Some(app) = app {
                let _ = app.emit(
                    NOTIFICATION_EVENT,
                    &NotificationEvent::ReadAll {
                        session_id: session_filter.and_then(|f| f.map(String::from)),
                    },
                );
            }
        }
        marked
    }

    pub fn remove(&self, id: &str, app: Option<&AppHandle>) -> bool {
        let removed = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.remove(id)
        };
        if removed {
            if let Some(app) = app {
                let _ = app.emit(
                    NOTIFICATION_EVENT,
                    &NotificationEvent::Removed { id: id.to_string() },
                );
            }
        }
        removed
    }

    pub fn remove_by_source_variant(
        &self,
        source: &NotificationSource,
        app: Option<&AppHandle>,
    ) -> usize {
        let removed = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.remove_by_source_variant(source)
        };
        if removed > 0 {
            if let Some(app) = app {
                // Emit Cleared for now — frontend can re-query if it needs
                // fine-grained per-id events. Phase 2 may expand this.
                let _ = app.emit(
                    NOTIFICATION_EVENT,
                    &NotificationEvent::Cleared { session_id: None },
                );
            }
        }
        removed
    }

    pub fn clear(&self, session_filter: Option<Option<&str>>, app: Option<&AppHandle>) -> usize {
        let removed = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.clear(session_filter)
        };
        if removed > 0 {
            if let Some(app) = app {
                let _ = app.emit(
                    NOTIFICATION_EVENT,
                    &NotificationEvent::Cleared {
                        session_id: session_filter.and_then(|f| f.map(String::from)),
                    },
                );
            }
        }
        removed
    }

    /// Test helper.
    #[cfg(test)]
    pub fn get(&self, id: &str) -> Option<Notification> {
        let store = self.inner.lock().expect("notification store poisoned");
        store.get(id)
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{NotificationLevel, NotificationRequest, NotificationSource};

    fn req(title: &str, session: Option<&str>) -> NotificationRequest {
        NotificationRequest {
            level: NotificationLevel::Info,
            source: NotificationSource::Cli,
            title: title.to_string(),
            subtitle: None,
            body: None,
            session_id: session.map(String::from),
            actions: Vec::new(),
        }
    }

    #[test]
    fn manager_push_and_list() {
        let mgr = NotificationManager::new();
        let n = mgr.push(req("hello", None), None);
        assert_eq!(mgr.list().len(), 1);
        assert!(mgr.get(&n.id).is_some());
    }

    #[test]
    fn manager_mark_read_reflects_in_unread() {
        let mgr = NotificationManager::new();
        let n = mgr.push(req("hello", Some("s1")), None);
        assert_eq!(mgr.unread_count(Some(Some("s1"))), 1);
        assert!(mgr.mark_read(&n.id, None));
        assert_eq!(mgr.unread_count(Some(Some("s1"))), 0);
        assert!(!mgr.mark_read(&n.id, None));
    }

    #[test]
    fn manager_remove() {
        let mgr = NotificationManager::new();
        let n = mgr.push(req("hello", None), None);
        assert!(mgr.remove(&n.id, None));
        assert!(mgr.get(&n.id).is_none());
    }
}
