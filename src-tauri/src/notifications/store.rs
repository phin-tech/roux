use std::collections::VecDeque;

use roux_core::{Notification, NotificationRequest, NotificationSource};

/// Maximum number of notifications retained in memory.
const STORE_CAPACITY: usize = 500;

/// In-memory notification store. Ephemeral: cleared on app restart.
///
/// Ordering is by insertion, which is monotonic because `push` assigns
/// `created_at` from `SystemTime::now()` at call time. The `id` is a uuid v4;
/// ordering is not derived from the id.
pub struct NotificationStore {
    entries: VecDeque<Notification>,
    capacity: usize,
}

impl NotificationStore {
    pub fn new() -> Self {
        Self::with_capacity(STORE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(64)),
            capacity,
        }
    }

    /// Push a new notification. Returns the stored notification (with id/created_at filled in).
    pub fn push(&mut self, req: NotificationRequest) -> Notification {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let notification = Notification {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: now_ms,
            level: req.level,
            source: req.source,
            title: req.title,
            subtitle: req.subtitle,
            body: req.body,
            session_id: req.session_id,
            read: false,
            actions: req.actions,
        };

        self.entries.push_back(notification.clone());
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        notification
    }

    /// Return a snapshot of all notifications, newest first.
    pub fn list(&self) -> Vec<Notification> {
        self.entries.iter().rev().cloned().collect()
    }

    /// Return a snapshot filtered by session id. Pass `None` to match global-only notifications.
    pub fn list_for_session(&self, session_id: Option<&str>) -> Vec<Notification> {
        self.entries
            .iter()
            .rev()
            .filter(|n| n.session_id.as_deref() == session_id)
            .cloned()
            .collect()
    }

    /// Number of unread notifications. `session_filter`:
    /// - `Some(Some(id))`: only notifications with `session_id == Some(id)`
    /// - `Some(None)`: only global notifications (`session_id == None`)
    /// - `None`: all notifications
    pub fn unread_count(&self, session_filter: Option<Option<&str>>) -> usize {
        self.entries
            .iter()
            .filter(|n| !n.read)
            .filter(|n| match session_filter {
                None => true,
                Some(filter) => n.session_id.as_deref() == filter,
            })
            .count()
    }

    pub fn mark_read(&mut self, id: &str) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|n| n.id == id) {
            if !entry.read {
                entry.read = true;
                return true;
            }
        }
        false
    }

    /// Mark all notifications as read within the given scope.
    /// `session_filter` semantics match [`Self::unread_count`].
    pub fn mark_all_read(&mut self, session_filter: Option<Option<&str>>) -> usize {
        let mut count = 0;
        for entry in self.entries.iter_mut() {
            let matches = match session_filter {
                None => true,
                Some(filter) => entry.session_id.as_deref() == filter,
            };
            if matches && !entry.read {
                entry.read = true;
                count += 1;
            }
        }
        count
    }

    pub fn remove(&mut self, id: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|n| n.id == id) {
            self.entries.remove(pos);
            return true;
        }
        false
    }

    /// Remove all notifications with a source that matches the given source variant.
    /// Variant equality only — e.g. removing with `Source::Watch { watch_id: "abc" }`
    /// removes all `Source::Watch` entries, regardless of which watch_id they carry.
    /// This matches the "dismiss all from source X" UX.
    pub fn remove_by_source_variant(&mut self, source: &NotificationSource) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|n| !same_source_variant(&n.source, source));
        before - self.entries.len()
    }

    pub fn clear(&mut self, session_filter: Option<Option<&str>>) -> usize {
        let before = self.entries.len();
        match session_filter {
            None => self.entries.clear(),
            Some(filter) => {
                self.entries
                    .retain(|n| n.session_id.as_deref() != filter);
            }
        }
        before - self.entries.len()
    }

    /// Number of entries currently stored.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Get a notification by id. Used by tests today; reserved for Phase 2
    /// frontend command routing (e.g. focusing by notification id).
    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<Notification> {
        self.entries.iter().find(|n| n.id == id).cloned()
    }
}

impl Default for NotificationStore {
    fn default() -> Self {
        Self::new()
    }
}

fn same_source_variant(a: &NotificationSource, b: &NotificationSource) -> bool {
    use NotificationSource::*;
    matches!(
        (a, b),
        (Hook { .. }, Hook { .. })
            | (Watch { .. }, Watch { .. })
            | (Task { .. }, Task { .. })
            | (Cli, Cli)
            | (Osc { .. }, Osc { .. })
            | (Internal, Internal)
    )
}

/// Quick helper to build a minimal test request. Kept here so both unit tests
/// and integration tests can use it.
#[cfg(test)]
pub(crate) fn test_request(
    level: roux_core::NotificationLevel,
    source: NotificationSource,
    title: &str,
    session_id: Option<&str>,
) -> NotificationRequest {
    NotificationRequest {
        level,
        source,
        title: title.to_string(),
        subtitle: None,
        body: None,
        session_id: session_id.map(String::from),
        actions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::NotificationLevel as L;
    use roux_core::NotificationSource as S;

    #[test]
    fn push_assigns_id_and_created_at() {
        let mut store = NotificationStore::new();
        let n = store.push(test_request(L::Info, S::Cli, "hello", None));
        assert!(!n.id.is_empty());
        assert!(n.created_at > 0);
        assert!(!n.read);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn push_caps_at_capacity() {
        let mut store = NotificationStore::with_capacity(3);
        for i in 0..5 {
            store.push(test_request(L::Info, S::Cli, &format!("n{i}"), None));
        }
        assert_eq!(store.len(), 3);
        // Newest first in list(); oldest two were dropped, so titles should be n4, n3, n2.
        let list = store.list();
        assert_eq!(list[0].title, "n4");
        assert_eq!(list[1].title, "n3");
        assert_eq!(list[2].title, "n2");
    }

    #[test]
    fn list_is_newest_first() {
        let mut store = NotificationStore::new();
        store.push(test_request(L::Info, S::Cli, "first", None));
        store.push(test_request(L::Info, S::Cli, "second", None));
        let list = store.list();
        assert_eq!(list[0].title, "second");
        assert_eq!(list[1].title, "first");
    }

    #[test]
    fn unread_count_filters_by_session() {
        let mut store = NotificationStore::new();
        store.push(test_request(L::Info, S::Cli, "global", None));
        store.push(test_request(L::Info, S::Cli, "sess-a-1", Some("a")));
        store.push(test_request(L::Info, S::Cli, "sess-a-2", Some("a")));
        store.push(test_request(L::Info, S::Cli, "sess-b-1", Some("b")));

        assert_eq!(store.unread_count(None), 4);
        assert_eq!(store.unread_count(Some(None)), 1);
        assert_eq!(store.unread_count(Some(Some("a"))), 2);
        assert_eq!(store.unread_count(Some(Some("b"))), 1);
        assert_eq!(store.unread_count(Some(Some("missing"))), 0);
    }

    #[test]
    fn mark_read_is_idempotent_and_reports_change() {
        let mut store = NotificationStore::new();
        let n = store.push(test_request(L::Info, S::Cli, "x", None));
        assert!(store.mark_read(&n.id));
        assert!(!store.mark_read(&n.id));
        assert_eq!(store.unread_count(None), 0);
    }

    #[test]
    fn mark_all_read_respects_scope() {
        let mut store = NotificationStore::new();
        store.push(test_request(L::Info, S::Cli, "global", None));
        store.push(test_request(L::Info, S::Cli, "a1", Some("a")));
        store.push(test_request(L::Info, S::Cli, "a2", Some("a")));
        store.push(test_request(L::Info, S::Cli, "b1", Some("b")));

        let marked = store.mark_all_read(Some(Some("a")));
        assert_eq!(marked, 2);
        assert_eq!(store.unread_count(Some(Some("a"))), 0);
        assert_eq!(store.unread_count(Some(Some("b"))), 1);
        assert_eq!(store.unread_count(Some(None)), 1);
    }

    #[test]
    fn remove_by_source_variant_removes_matching_variant_only() {
        let mut store = NotificationStore::new();
        store.push(test_request(L::Info, S::Cli, "cli-1", None));
        store.push(test_request(L::Info, S::Cli, "cli-2", None));
        store.push(test_request(
            L::Info,
            S::Watch { watch_id: "w1".into() },
            "watch-1",
            None,
        ));
        store.push(test_request(
            L::Info,
            S::Watch { watch_id: "w2".into() },
            "watch-2",
            None,
        ));

        let removed = store.remove_by_source_variant(&S::Watch {
            watch_id: "irrelevant".into(),
        });
        assert_eq!(removed, 2);
        assert_eq!(store.len(), 2);
        let remaining: Vec<_> = store.list().iter().map(|n| n.title.clone()).collect();
        assert_eq!(remaining, vec!["cli-2", "cli-1"]);
    }

    #[test]
    fn clear_respects_scope() {
        let mut store = NotificationStore::new();
        store.push(test_request(L::Info, S::Cli, "global", None));
        store.push(test_request(L::Info, S::Cli, "a1", Some("a")));
        store.push(test_request(L::Info, S::Cli, "b1", Some("b")));
        let removed = store.clear(Some(Some("a")));
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 2);

        let removed = store.clear(None);
        assert_eq!(removed, 2);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn list_for_session_filters_correctly() {
        let mut store = NotificationStore::new();
        store.push(test_request(L::Info, S::Cli, "global", None));
        store.push(test_request(L::Info, S::Cli, "a1", Some("a")));
        store.push(test_request(L::Info, S::Cli, "a2", Some("a")));

        let a = store.list_for_session(Some("a"));
        assert_eq!(a.len(), 2);
        let g = store.list_for_session(None);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].title, "global");
    }
}
