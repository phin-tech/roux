use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use roux_core::{Notification, NotificationEvent, NotificationRequest, NotificationSource};

use super::policy::{should_fan_out_to_os, PolicyInput};
use super::store::NotificationStore;
use crate::state::AppState;

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
        Self { inner: Arc::new(Mutex::new(NotificationStore::new())) }
    }

    /// Push a new notification and emit an `Added` event to the frontend.
    /// Also fans out to an OS notification when the policy says so. Returns
    /// the stored notification (with id + created_at filled in).
    pub fn push(&self, req: NotificationRequest, app: Option<&AppHandle>) -> Notification {
        // Dedup fast path: if the request carries a dedup_key and an unread
        // entry with that key already exists, update it in place instead of
        // creating a new notification. Keeps permission-prompt floods from
        // stacking up dozens of identical cards.
        if let Some(key) = req.dedup_key.clone() {
            let updated = {
                let mut store = self.inner.lock().expect("notification store poisoned");
                store.update_by_dedup_key(&key, &req)
            };
            if let Some(notification) = updated {
                if let Some(app) = app {
                    let _ = app.emit(
                        NOTIFICATION_EVENT,
                        &NotificationEvent::Updated { notification: notification.clone() },
                    );
                    self.maybe_fan_out_to_os(app, &notification);
                }
                return notification;
            }
        }

        let notification = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.push(req)
        };
        if let Some(app) = app {
            let _ = app.emit(
                NOTIFICATION_EVENT,
                &NotificationEvent::Added { notification: notification.clone() },
            );
            self.maybe_fan_out_to_os(app, &notification);
        }
        notification
    }

    fn maybe_fan_out_to_os(&self, app: &AppHandle, notification: &Notification) {
        // Read current settings + focus state. If either the kill switch is
        // off or the window is focused, the policy returns false and we bail.
        let notifications_enabled = app
            .try_state::<AppState>()
            .map(|s| s.settings.lock().map(|g| g.notifications_enabled).unwrap_or(true))
            .unwrap_or(true);

        // Iterate all webview windows — tauri.conf.json doesn't pin a label,
        // so the window label is not reliably "main". We report "focused" if
        // any of the app's windows currently has focus.
        let window_focused =
            app.webview_windows().values().any(|w| w.is_focused().unwrap_or(false));

        let should = should_fan_out_to_os(PolicyInput {
            level: notification.level,
            source: &notification.source,
            window_focused,
            notifications_enabled,
        });

        crate::rlog!(
            "notifications.policy: level={:?} source={:?} focused={} enabled={} => fire={}",
            notification.level,
            notification.source,
            window_focused,
            notifications_enabled,
            should
        );

        if !should {
            return;
        }

        let title = build_os_title(notification);
        let body = notification.body.clone().unwrap_or_default();
        match app.notification().builder().title(&title).body(&body).show() {
            Ok(()) => crate::rlog!("notifications.os: fired title={:?}", title),
            Err(e) => crate::rlog!("notifications.os: FAILED title={:?} err={}", title, e),
        }
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
                let _ =
                    app.emit(NOTIFICATION_EVENT, &NotificationEvent::Read { id: id.to_string() });
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
                let _ = app
                    .emit(NOTIFICATION_EVENT, &NotificationEvent::Removed { id: id.to_string() });
            }
        }
        removed
    }

    /// Remove the unread notification carrying `dedup_key` and emit a
    /// `Removed` event. Returns `true` when an entry was removed. No-op
    /// when the key is absent or only matches read entries — see
    /// `NotificationStore::remove_by_dedup_key` for the "handle to a live
    /// notification" rationale.
    pub fn remove_by_dedup_key(&self, key: &str, app: Option<&AppHandle>) -> bool {
        let removed_id = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.remove_by_dedup_key(key)
        };
        if let Some(id) = removed_id {
            if let Some(app) = app {
                let _ =
                    app.emit(NOTIFICATION_EVENT, &NotificationEvent::Removed { id: id.clone() });
            }
            return true;
        }
        false
    }

    pub fn remove_by_source_variant(
        &self,
        source: &NotificationSource,
        app: Option<&AppHandle>,
    ) -> usize {
        let removed_ids = {
            let mut store = self.inner.lock().expect("notification store poisoned");
            store.remove_by_source_variant(source)
        };
        if let Some(app) = app {
            for id in &removed_ids {
                let _ =
                    app.emit(NOTIFICATION_EVENT, &NotificationEvent::Removed { id: id.clone() });
            }
        }
        removed_ids.len()
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

/// Build the OS-notification title. For attention/error we prepend a small
/// glyph so the alert is scannable at a glance in the notification center.
fn build_os_title(notification: &Notification) -> String {
    use roux_core::NotificationLevel as L;
    let prefix = match notification.level {
        L::Attention => "⚠ ",
        L::Error => "✖ ",
        L::Warning => "! ",
        L::Success => "✓ ",
        L::Info => "",
    };
    if let Some(ref sub) = notification.subtitle {
        format!("{}{} — {}", prefix, notification.title, sub)
    } else {
        format!("{}{}", prefix, notification.title)
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
            dedup_key: None,
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

    #[test]
    fn manager_push_with_dedup_key_updates_in_place() {
        let mgr = NotificationManager::new();
        let mut first_req = req("first", None);
        first_req.dedup_key = Some("key-1".into());
        let first = mgr.push(first_req, None);

        let mut second_req = req("second", None);
        second_req.dedup_key = Some("key-1".into());
        let second = mgr.push(second_req, None);

        // Same id, refreshed title, store length unchanged.
        assert_eq!(first.id, second.id);
        assert_eq!(second.title, "second");
        assert_eq!(mgr.list().len(), 1);
    }

    #[test]
    fn manager_remove_by_dedup_key_removes_and_emits() {
        let mgr = NotificationManager::new();
        let mut r = req("a", None);
        r.dedup_key = Some("attention:pane:p-1".into());
        let n = mgr.push(r, None);
        assert!(mgr.remove_by_dedup_key("attention:pane:p-1", None));
        assert!(mgr.get(&n.id).is_none(), "entry should be gone");
    }

    #[test]
    fn manager_remove_by_dedup_key_returns_false_without_match() {
        let mgr = NotificationManager::new();
        assert!(!mgr.remove_by_dedup_key("nonexistent", None));
    }

    #[test]
    fn manager_push_with_different_dedup_keys_appends() {
        let mgr = NotificationManager::new();
        let mut a = req("a", None);
        a.dedup_key = Some("ka".into());
        let mut b = req("b", None);
        b.dedup_key = Some("kb".into());
        mgr.push(a, None);
        mgr.push(b, None);
        assert_eq!(mgr.list().len(), 2);
    }
}
