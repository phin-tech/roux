use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use roux_core::{validate_topic_pattern, BusSubscription};

use crate::alias_store::ProjectFilter;
use crate::subscription_persistence::{load_from_path, persistence_path, save_to_path};
use crate::subscription_store::{AddError, SubscriptionStore};

#[derive(Debug, thiserror::Error)]
pub enum SubscribeError {
    #[error("invalid topic pattern: {0}")]
    InvalidPattern(String),
    #[error("invalid alias: {0}")]
    InvalidAlias(String),
    #[error(transparent)]
    Store(#[from] AddError),
    /// Disk save failed. The in-memory mutation has been rolled back
    /// before this error is surfaced — caller need not retry the undo.
    #[error("persistence failed: {0}")]
    Persist(String),
}

#[derive(Debug, thiserror::Error)]
pub enum UnsubscribeError {
    /// Disk save failed; the deletion has been rolled back.
    #[error("persistence failed: {0}")]
    Persist(String),
}

/// Clonable handle over the subscription store. Persistence runs
/// synchronously on every mutation — the file is small (typically a few
/// dozen rows × ~150 bytes) so cost is negligible and the simpler
/// "save on every change" model avoids stale-state recovery after crash.
#[derive(Clone)]
pub struct SubscriptionManager {
    inner: Arc<Mutex<SubscriptionStore>>,
    persistence_path: Option<Arc<PathBuf>>,
}

impl SubscriptionManager {
    pub fn load() -> Self {
        Self::load_from(persistence_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let entries = load_from_path(&path);
        Self {
            inner: Arc::new(Mutex::new(SubscriptionStore::from_entries(entries))),
            persistence_path: Some(Arc::new(path)),
        }
    }

    /// In-memory variant. No load, no persist on mutations. For tests.
    pub fn in_memory() -> Self {
        Self { inner: Arc::new(Mutex::new(SubscriptionStore::new())), persistence_path: None }
    }

    /// Add a subscription. Validates the pattern (and alias format)
    /// before insertion. Returns the persisted subscription with its id
    /// stamped.
    pub fn subscribe(
        &self,
        alias: &str,
        pattern: &str,
        project_id: Option<String>,
    ) -> Result<BusSubscription, SubscribeError> {
        let canonical_alias = roux_core::validate_alias_name(alias)
            .map_err(|e| SubscribeError::InvalidAlias(e.to_string()))?;
        let validated_pattern = validate_topic_pattern(pattern)
            .map_err(|e| SubscribeError::InvalidPattern(e.to_string()))?;

        let subscription = BusSubscription {
            id: uuid::Uuid::new_v4().to_string(),
            alias: canonical_alias,
            pattern: validated_pattern,
            project_id,
            created_at: now_ms(),
        };

        let inserted = {
            let mut store = self.inner.lock().expect("subscription store poisoned");
            store.add(subscription)?
        };
        // Persist BEFORE emitting — if the disk write fails, roll the
        // in-memory mutation back and surface the error so the caller
        // can retry. Otherwise the UI/CLI would observe a "Created"
        // event that disappears on next restart.
        if let Err(e) = self.persist() {
            let mut store = self.inner.lock().expect("subscription store poisoned");
            store.remove(&inserted.id);
            return Err(SubscribeError::Persist(e.to_string()));
        }
        Ok(inserted)
    }

    /// Remove by id. Returns `Ok(true)` when something was deleted,
    /// `Ok(false)` when the id was unknown, and an error when the
    /// disk save failed (in which case the in-memory deletion has
    /// been rolled back).
    pub fn unsubscribe(&self, id: &str) -> Result<bool, UnsubscribeError> {
        let removed_entry = {
            let mut store = self.inner.lock().expect("subscription store poisoned");
            let entry = store.get(id).cloned();
            if entry.is_some() {
                store.remove(id);
            }
            entry
        };
        let Some(removed) = removed_entry else {
            return Ok(false);
        };
        if let Err(e) = self.persist() {
            // Roll back: re-insert the entry. We use `add` here because
            // the duplicate-triple guard won't fire (we just removed the
            // sole row with this triple).
            let mut store = self.inner.lock().expect("subscription store poisoned");
            let _ = store.add(removed);
            return Err(UnsubscribeError::Persist(e.to_string()));
        }
        Ok(true)
    }

    pub fn get(&self, id: &str) -> Option<BusSubscription> {
        let store = self.inner.lock().expect("subscription store poisoned");
        store.get(id).cloned()
    }

    pub fn for_alias(
        &self,
        alias: &str,
        project_filter: ProjectFilter<'_>,
    ) -> Vec<BusSubscription> {
        let store = self.inner.lock().expect("subscription store poisoned");
        store.for_alias(alias, project_filter)
    }

    pub fn list(&self, project_filter: ProjectFilter<'_>) -> Vec<BusSubscription> {
        let store = self.inner.lock().expect("subscription store poisoned");
        store.list(project_filter)
    }

    /// Subscriptions whose pattern matches `topic` AND whose project
    /// scope is compatible with `event_project_id`. Hot path on every
    /// `bus publish`.
    pub fn matching_topic(
        &self,
        topic: &str,
        event_project_id: Option<&str>,
    ) -> Vec<BusSubscription> {
        let store = self.inner.lock().expect("subscription store poisoned");
        store.matching_topic(topic, event_project_id)
    }

    /// Patterns subscribed to by `alias` in scopes compatible with
    /// `event_project_id`. Used by the mailbox layer to extend
    /// `list_for_recipient` and ack ownership for topic events.
    pub fn patterns_for_alias(&self, alias: &str, event_project_id: Option<&str>) -> Vec<String> {
        let store = self.inner.lock().expect("subscription store poisoned");
        store.patterns_for_alias(alias, event_project_id)
    }

    /// Persist the current in-memory state. `Ok(())` when there's no
    /// configured path (test mode) so callers can chain unconditionally.
    fn persist(&self) -> std::io::Result<()> {
        let Some(path) = self.persistence_path.as_ref() else {
            return Ok(());
        };
        let store = self.inner.lock().expect("subscription store poisoned");
        save_to_path(store.entries(), path.as_path())
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
    use tempfile::tempdir;

    #[test]
    fn subscribe_persists_to_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let mgr = SubscriptionManager::load_from(path.clone());
        let s = mgr.subscribe("auditor", "*.completed", None).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(&s.id));
        assert!(raw.contains("auditor"));
    }

    #[test]
    fn subscribe_rejects_invalid_pattern() {
        let mgr = SubscriptionManager::in_memory();
        let err = mgr.subscribe("auditor", "Bad Pattern!", None).unwrap_err();
        assert!(matches!(err, SubscribeError::InvalidPattern(_)));
    }

    #[test]
    fn subscribe_rejects_invalid_alias() {
        let mgr = SubscriptionManager::in_memory();
        let err = mgr.subscribe("Bad Alias!", "*", None).unwrap_err();
        assert!(matches!(err, SubscribeError::InvalidAlias(_)));
    }

    #[test]
    fn subscribe_canonicalizes_alias_to_lowercase() {
        let mgr = SubscriptionManager::in_memory();
        let s = mgr.subscribe("Auditor", "*", None).unwrap();
        assert_eq!(s.alias, "auditor");
    }

    #[test]
    fn subscribe_rejects_duplicate_triple() {
        let mgr = SubscriptionManager::in_memory();
        mgr.subscribe("auditor", "*.completed", None).unwrap();
        let err = mgr.subscribe("auditor", "*.completed", None).unwrap_err();
        assert!(matches!(err, SubscribeError::Store(AddError::Duplicate { .. })));
    }

    #[test]
    fn unsubscribe_returns_false_for_missing_id() {
        let mgr = SubscriptionManager::in_memory();
        assert!(!mgr.unsubscribe("ghost").unwrap());
    }

    #[test]
    fn unsubscribe_removes_and_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let mgr = SubscriptionManager::load_from(path.clone());
        let s = mgr.subscribe("auditor", "*", None).unwrap();
        assert!(mgr.unsubscribe(&s.id).unwrap());

        let mgr2 = SubscriptionManager::load_from(path);
        assert!(mgr2.get(&s.id).is_none());
    }

    #[test]
    fn matching_topic_returns_glob_matches() {
        let mgr = SubscriptionManager::in_memory();
        mgr.subscribe("a", "repo-a.*", None).unwrap();
        mgr.subscribe("b", "**.completed", None).unwrap();
        let ms = mgr.matching_topic("repo-a.build.completed", None);
        let aliases: Vec<_> = ms.iter().map(|s| s.alias.clone()).collect();
        assert_eq!(aliases, vec!["b".to_string()]);
    }

    #[test]
    fn for_alias_round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let mgr = SubscriptionManager::load_from(path.clone());
        mgr.subscribe("auditor", "*.completed", None).unwrap();
        mgr.subscribe("auditor", "**.failed", Some("p1".into())).unwrap();

        let mgr2 = SubscriptionManager::load_from(path);
        let mine = mgr2.for_alias("auditor", ProjectFilter::Any);
        assert_eq!(mine.len(), 2);
    }
}
