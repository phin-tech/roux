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
        Self { entries: VecDeque::with_capacity(capacity.min(64)), capacity }
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
            dedup_key: req.dedup_key,
        };

        self.entries.push_back(notification.clone());
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        notification
    }

    /// Update an existing unread notification with the given dedup key in-place.
    /// Returns the updated notification, or `None` if no matching unread entry exists.
    /// The caller is responsible for emitting the `Updated` event.
    pub fn update_by_dedup_key(
        &mut self,
        key: &str,
        req: &NotificationRequest,
    ) -> Option<Notification> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let entry = self
            .entries
            .iter_mut()
            .rev()
            .find(|n| !n.read && n.dedup_key.as_deref() == Some(key))?;

        entry.level = req.level;
        entry.source = req.source.clone();
        entry.title = req.title.clone();
        entry.subtitle = req.subtitle.clone();
        entry.body = req.body.clone();
        entry.session_id = req.session_id.clone();
        entry.actions = req.actions.clone();
        entry.created_at = now_ms;
        Some(entry.clone())
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

    /// Remove the most recent unread notification carrying the given dedup
    /// key. Returns the removed notification's id so the caller can emit
    /// the `Removed` event. Read entries are skipped: matches
    /// `update_by_dedup_key` semantics — a dedup key is a handle to a
    /// live notification, not to user-acknowledged history.
    pub fn remove_by_dedup_key(&mut self, key: &str) -> Option<String> {
        let pos = self.entries.iter().rposition(|n| {
            !n.read && n.dedup_key.as_deref() == Some(key)
        })?;
        self.entries.remove(pos).map(|n| n.id)
    }

    /// Remove all notifications with a source that matches the given source variant.
    /// Variant equality only — e.g. removing with `Source::Watch { watch_id: "abc" }`
    /// removes all `Source::Watch` entries, regardless of which watch_id they carry.
    /// This matches the "dismiss all from source X" UX. Returns the ids of the
    /// removed notifications so the caller can emit per-id `Removed` events.
    pub fn remove_by_source_variant(&mut self, source: &NotificationSource) -> Vec<String> {
        let mut removed = Vec::new();
        self.entries.retain(|n| {
            if same_source_variant(&n.source, source) {
                removed.push(n.id.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    pub fn clear(&mut self, session_filter: Option<Option<&str>>) -> usize {
        let before = self.entries.len();
        match session_filter {
            None => self.entries.clear(),
            Some(filter) => {
                self.entries.retain(|n| n.session_id.as_deref() != filter);
            }
        }
        before - self.entries.len()
    }

    /// Number of entries currently stored.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Get a notification by id. Currently used only by tests — kept because
    /// any future per-id command routing (e.g. focus-by-notification) would
    /// want a direct lookup rather than scanning `list()`.
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
        dedup_key: None,
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

    fn req_with_key(title: &str, key: &str) -> NotificationRequest {
        let mut r = test_request(L::Info, S::Cli, title, None);
        r.dedup_key = Some(key.to_string());
        r
    }

    #[test]
    fn update_by_dedup_key_refreshes_existing_unread() {
        let mut store = NotificationStore::new();
        let first = store.push(req_with_key("first", "abc"));
        let next_req = req_with_key("second", "abc");
        let updated = store.update_by_dedup_key("abc", &next_req).expect("match");
        assert_eq!(store.len(), 1, "should update in place, not append");
        assert_eq!(updated.id, first.id, "id preserved");
        assert_eq!(updated.title, "second", "title refreshed");
        assert!(updated.created_at >= first.created_at);
    }

    #[test]
    fn update_by_dedup_key_does_not_match_read_entries() {
        let mut store = NotificationStore::new();
        let first = store.push(req_with_key("first", "abc"));
        store.mark_read(&first.id);
        let next_req = req_with_key("second", "abc");
        assert!(store.update_by_dedup_key("abc", &next_req).is_none());
        // Caller falls back to push; simulate that here to lock behavior.
        store.push(next_req);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn update_by_dedup_key_returns_none_without_match() {
        let mut store = NotificationStore::new();
        store.push(req_with_key("first", "abc"));
        assert!(store.update_by_dedup_key("different", &req_with_key("x", "abc")).is_none());
        assert_eq!(store.len(), 1);
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
        store.push(test_request(L::Info, S::Watch { watch_id: "w1".into() }, "watch-1", None));
        store.push(test_request(L::Info, S::Watch { watch_id: "w2".into() }, "watch-2", None));

        let removed = store.remove_by_source_variant(&S::Watch { watch_id: "irrelevant".into() });
        assert_eq!(removed.len(), 2);
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
    fn remove_by_dedup_key_removes_matching_unread_entry() {
        let mut store = NotificationStore::new();
        let a = store.push(req_with_key("a", "key-a"));
        let b = store.push(req_with_key("b", "key-b"));

        let removed = store.remove_by_dedup_key("key-a");
        assert_eq!(removed, Some(a.id.clone()));
        assert_eq!(store.len(), 1);
        assert_eq!(store.list()[0].id, b.id, "unrelated entry must survive");
    }

    #[test]
    fn remove_by_dedup_key_returns_none_without_match() {
        let mut store = NotificationStore::new();
        store.push(req_with_key("a", "key-a"));
        assert_eq!(store.remove_by_dedup_key("nope"), None);
        assert_eq!(store.len(), 1);
    }

    /// Matches `update_by_dedup_key` semantics: dedup keys only apply to
    /// live (unread) notifications. If the user already dismissed the
    /// attention notification themselves, an `Exit(Attention)` emitted
    /// seconds later must not silently retract history.
    #[test]
    fn remove_by_dedup_key_skips_read_entries() {
        let mut store = NotificationStore::new();
        let a = store.push(req_with_key("a", "key-a"));
        store.mark_read(&a.id);
        assert_eq!(store.remove_by_dedup_key("key-a"), None);
        assert_eq!(store.len(), 1);
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
