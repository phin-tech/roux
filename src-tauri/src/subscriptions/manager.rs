use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use roux_core::{validate_topic_pattern, BusSubscription, BusSubscriptionEvent};
use tauri::{AppHandle, Emitter};

use crate::aliases::ProjectFilter;

use super::persistence::{self, load_from_path, save_to_path};
use super::store::{AddError, SubscriptionStore};

/// Tauri event name emitted on every subscription mutation.
pub const SUBSCRIPTION_EVENT: &str = "subscription-event";

#[derive(Debug, thiserror::Error)]
pub enum SubscribeError {
    #[error("invalid topic pattern: {0}")]
    InvalidPattern(String),
    #[error("invalid alias: {0}")]
    InvalidAlias(String),
    #[error(transparent)]
    Store(#[from] AddError),
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
        Self::load_from(persistence::persistence_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let entries = load_from_path(&path);
        Self {
            inner: Arc::new(Mutex::new(SubscriptionStore::from_entries(entries))),
            persistence_path: Some(Arc::new(path)),
        }
    }

    /// In-memory variant. No load, no persist on mutations. For tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SubscriptionStore::new())),
            persistence_path: None,
        }
    }

    /// Add a subscription. Validates the pattern (and alias format)
    /// before insertion. Returns the persisted subscription with its id
    /// stamped.
    pub fn subscribe(
        &self,
        alias: &str,
        pattern: &str,
        project_id: Option<String>,
        app: Option<&AppHandle>,
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
        self.persist();
        if let Some(app) = app {
            let _ = app.emit(
                SUBSCRIPTION_EVENT,
                &BusSubscriptionEvent::Created { subscription: inserted.clone() },
            );
        }
        Ok(inserted)
    }

    /// Remove by id. Returns `true` when something was deleted.
    pub fn unsubscribe(&self, id: &str, app: Option<&AppHandle>) -> bool {
        let removed = {
            let mut store = self.inner.lock().expect("subscription store poisoned");
            store.remove(id)
        };
        if removed {
            self.persist();
            if let Some(app) = app {
                let _ = app
                    .emit(SUBSCRIPTION_EVENT, &BusSubscriptionEvent::Removed { id: id.to_string() });
            }
        }
        removed
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
    pub fn patterns_for_alias(
        &self,
        alias: &str,
        event_project_id: Option<&str>,
    ) -> Vec<String> {
        let store = self.inner.lock().expect("subscription store poisoned");
        store.patterns_for_alias(alias, event_project_id)
    }

    fn persist(&self) {
        let Some(path) = self.persistence_path.as_ref() else {
            return;
        };
        let store = self.inner.lock().expect("subscription store poisoned");
        if let Err(e) = save_to_path(store.entries(), path.as_path()) {
            eprintln!(
                "[roux] subscription persistence failed at {}: {e}",
                path.display(),
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
    use tempfile::tempdir;

    #[test]
    fn subscribe_persists_to_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let mgr = SubscriptionManager::load_from(path.clone());
        let s = mgr.subscribe("auditor", "*.completed", None, None).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(&s.id));
        assert!(raw.contains("auditor"));
    }

    #[test]
    fn subscribe_rejects_invalid_pattern() {
        let mgr = SubscriptionManager::in_memory();
        let err = mgr.subscribe("auditor", "Bad Pattern!", None, None).unwrap_err();
        assert!(matches!(err, SubscribeError::InvalidPattern(_)));
    }

    #[test]
    fn subscribe_rejects_invalid_alias() {
        let mgr = SubscriptionManager::in_memory();
        let err = mgr.subscribe("Bad Alias!", "*", None, None).unwrap_err();
        assert!(matches!(err, SubscribeError::InvalidAlias(_)));
    }

    #[test]
    fn subscribe_canonicalizes_alias_to_lowercase() {
        let mgr = SubscriptionManager::in_memory();
        let s = mgr.subscribe("Auditor", "*", None, None).unwrap();
        assert_eq!(s.alias, "auditor");
    }

    #[test]
    fn subscribe_rejects_duplicate_triple() {
        let mgr = SubscriptionManager::in_memory();
        mgr.subscribe("auditor", "*.completed", None, None).unwrap();
        let err = mgr.subscribe("auditor", "*.completed", None, None).unwrap_err();
        assert!(matches!(err, SubscribeError::Store(AddError::Duplicate { .. })));
    }

    #[test]
    fn unsubscribe_returns_false_for_missing_id() {
        let mgr = SubscriptionManager::in_memory();
        assert!(!mgr.unsubscribe("ghost", None));
    }

    #[test]
    fn unsubscribe_removes_and_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let mgr = SubscriptionManager::load_from(path.clone());
        let s = mgr.subscribe("auditor", "*", None, None).unwrap();
        assert!(mgr.unsubscribe(&s.id, None));

        let mgr2 = SubscriptionManager::load_from(path);
        assert!(mgr2.get(&s.id).is_none());
    }

    #[test]
    fn matching_topic_returns_glob_matches() {
        let mgr = SubscriptionManager::in_memory();
        mgr.subscribe("a", "repo-a.*", None, None).unwrap();
        mgr.subscribe("b", "**.completed", None, None).unwrap();
        let ms = mgr.matching_topic("repo-a.build.completed", None);
        let aliases: Vec<_> = ms.iter().map(|s| s.alias.clone()).collect();
        assert_eq!(aliases, vec!["b".to_string()]);
    }

    #[test]
    fn for_alias_round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let mgr = SubscriptionManager::load_from(path.clone());
        mgr.subscribe("auditor", "*.completed", None, None).unwrap();
        mgr.subscribe("auditor", "**.failed", Some("p1".into()), None).unwrap();

        let mgr2 = SubscriptionManager::load_from(path);
        let mine = mgr2.for_alias("auditor", ProjectFilter::Any);
        assert_eq!(mine.len(), 2);
    }
}
