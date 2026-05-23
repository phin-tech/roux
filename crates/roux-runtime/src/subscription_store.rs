use roux_core::{topic_matches, BusSubscription};

use crate::alias_store::ProjectFilter;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AddError {
    #[error(
        "subscription already exists for alias '{alias}' pattern '{pattern}' in this project scope"
    )]
    Duplicate { alias: String, pattern: String },
}

/// In-memory subscription store. Entries are keyed implicitly by `id`
/// (uuid). Duplicate-prevention triple is `(alias, pattern, project_id)` —
/// adding the same triple twice is rejected so a chatty agent doesn't
/// bloat the table.
///
/// Persistence and event emission live in `SubscriptionManager` (mirrors
/// the `AliasStore` / `AliasManager` split).
pub struct SubscriptionStore {
    entries: Vec<BusSubscription>,
}

impl SubscriptionStore {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn from_entries(entries: Vec<BusSubscription>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[BusSubscription] {
        &self.entries
    }

    /// Add `subscription`. Rejects if `(alias, pattern, project_id)` is
    /// already present.
    pub fn add(&mut self, subscription: BusSubscription) -> Result<BusSubscription, AddError> {
        if self.entries.iter().any(|s| {
            s.alias == subscription.alias
                && s.pattern == subscription.pattern
                && s.project_id == subscription.project_id
        }) {
            return Err(AddError::Duplicate {
                alias: subscription.alias,
                pattern: subscription.pattern,
            });
        }
        self.entries.push(subscription.clone());
        Ok(subscription)
    }

    /// Remove by id. Returns `true` when something was actually removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|s| s.id != id);
        self.entries.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&BusSubscription> {
        self.entries.iter().find(|s| s.id == id)
    }

    /// Subscriptions belonging to `alias` within the given project filter.
    pub fn for_alias(
        &self,
        alias: &str,
        project_filter: ProjectFilter<'_>,
    ) -> Vec<BusSubscription> {
        self.entries
            .iter()
            .filter(|s| s.alias == alias)
            .filter(|s| project_filter.matches(s.project_id.as_deref()))
            .cloned()
            .collect()
    }

    /// All subscriptions matching the given project filter.
    pub fn list(&self, project_filter: ProjectFilter<'_>) -> Vec<BusSubscription> {
        self.entries
            .iter()
            .filter(|s| project_filter.matches(s.project_id.as_deref()))
            .cloned()
            .collect()
    }

    /// Subscriptions whose pattern matches `topic` AND whose project
    /// scope is compatible with `event_project_id`. A subscription with
    /// `project_id = None` is global and matches any event scope; a
    /// scoped subscription only matches events in the same scope.
    pub fn matching_topic(
        &self,
        topic: &str,
        event_project_id: Option<&str>,
    ) -> Vec<BusSubscription> {
        self.entries
            .iter()
            .filter(|s| project_scope_matches(s.project_id.as_deref(), event_project_id))
            .filter(|s| topic_matches(&s.pattern, topic))
            .cloned()
            .collect()
    }

    /// Patterns subscribed to by `alias` in scopes compatible with
    /// `event_project_id`. Hot path for `EventStore::list_for_recipient`
    /// and `recipient_owns` — keep cheap (string clones only).
    pub fn patterns_for_alias(&self, alias: &str, event_project_id: Option<&str>) -> Vec<String> {
        self.entries
            .iter()
            .filter(|s| s.alias == alias)
            .filter(|s| project_scope_matches(s.project_id.as_deref(), event_project_id))
            .map(|s| s.pattern.clone())
            .collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for SubscriptionStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true when a subscription's project scope is compatible with
/// an event's project scope. A `None` (global) subscription matches any
/// event scope; a scoped subscription only matches events in the same
/// scope.
fn project_scope_matches(sub_scope: Option<&str>, event_scope: Option<&str>) -> bool {
    match sub_scope {
        None => true,
        Some(s) => event_scope == Some(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(id: &str, alias: &str, pattern: &str, project_id: Option<&str>) -> BusSubscription {
        BusSubscription {
            id: id.into(),
            alias: alias.into(),
            pattern: pattern.into(),
            project_id: project_id.map(str::to_string),
            created_at: 0,
        }
    }

    #[test]
    fn add_then_get_round_trips() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "auditor", "*.completed", None)).unwrap();
        let got = store.get("s1").unwrap();
        assert_eq!(got.alias, "auditor");
        assert_eq!(got.pattern, "*.completed");
    }

    #[test]
    fn add_rejects_duplicate_triple() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "auditor", "*.completed", None)).unwrap();
        let err = store.add(sub("s2", "auditor", "*.completed", None)).unwrap_err();
        assert!(matches!(err, AddError::Duplicate { .. }));
    }

    #[test]
    fn add_allows_same_pattern_in_different_project_scopes() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "auditor", "*.completed", None)).unwrap();
        store.add(sub("s2", "auditor", "*.completed", Some("p1"))).unwrap();
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn remove_returns_false_for_missing_id() {
        let mut store = SubscriptionStore::new();
        assert!(!store.remove("missing"));
    }

    #[test]
    fn remove_returns_true_and_drops_entry() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "a", "*", None)).unwrap();
        assert!(store.remove("s1"));
        assert!(store.get("s1").is_none());
    }

    #[test]
    fn for_alias_filters_by_alias_and_project() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "auditor", "*.completed", None)).unwrap();
        store.add(sub("s2", "auditor", "*.failed", Some("p1"))).unwrap();
        store.add(sub("s3", "builder", "*", None)).unwrap();

        let auditor_any = store.for_alias("auditor", ProjectFilter::Any);
        assert_eq!(auditor_any.len(), 2);

        let auditor_global = store.for_alias("auditor", ProjectFilter::Exact(None));
        assert_eq!(auditor_global.len(), 1);
        assert_eq!(auditor_global[0].pattern, "*.completed");

        let builder = store.for_alias("builder", ProjectFilter::Any);
        assert_eq!(builder.len(), 1);
    }

    #[test]
    fn matching_topic_returns_only_glob_matches() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "a", "repo-a.*", None)).unwrap();
        store.add(sub("s2", "b", "**.completed", None)).unwrap();
        store.add(sub("s3", "c", "exact.match", None)).unwrap();

        let ms = store.matching_topic("repo-a.build.completed", None);
        let aliases: Vec<_> = ms.iter().map(|s| s.alias.as_str()).collect();
        // `repo-a.*` does NOT match three-segment topic; `**.completed` does.
        assert_eq!(aliases, vec!["b"]);
    }

    #[test]
    fn matching_topic_respects_project_scope() {
        let mut store = SubscriptionStore::new();
        // Global sub matches any event scope.
        store.add(sub("s1", "a", "*", None)).unwrap();
        // Scoped sub only matches events in the same project.
        store.add(sub("s2", "b", "*", Some("p1"))).unwrap();
        store.add(sub("s3", "c", "*", Some("p2"))).unwrap();

        // Event in project p1: sub s1 (global) and s2 (p1) match; s3 (p2) doesn't.
        let ms = store.matching_topic("foo", Some("p1"));
        let aliases: Vec<_> = ms.iter().map(|s| s.alias.as_str()).collect();
        assert_eq!(aliases, vec!["a", "b"]);

        // Global event (no project): only the global sub matches.
        let ms = store.matching_topic("foo", None);
        let aliases: Vec<_> = ms.iter().map(|s| s.alias.as_str()).collect();
        assert_eq!(aliases, vec!["a"]);
    }

    #[test]
    fn patterns_for_alias_returns_relevant_patterns_only() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "auditor", "repo-a.*", None)).unwrap();
        store.add(sub("s2", "auditor", "**.failed", Some("p1"))).unwrap();
        store.add(sub("s3", "builder", "*", None)).unwrap();

        // Event in p1 sees both auditor subs (global s1 + scoped s2).
        let mut got = store.patterns_for_alias("auditor", Some("p1"));
        got.sort();
        assert_eq!(got, vec!["**.failed".to_string(), "repo-a.*".to_string()]);

        // Global event only sees the global sub.
        let got = store.patterns_for_alias("auditor", None);
        assert_eq!(got, vec!["repo-a.*".to_string()]);
    }

    #[test]
    fn list_filters_by_project_scope() {
        let mut store = SubscriptionStore::new();
        store.add(sub("s1", "a", "*", None)).unwrap();
        store.add(sub("s2", "b", "*", Some("p1"))).unwrap();

        assert_eq!(store.list(ProjectFilter::Any).len(), 2);
        assert_eq!(store.list(ProjectFilter::Exact(None)).len(), 1);
        assert_eq!(store.list(ProjectFilter::Exact(Some("p1"))).len(), 1);
        assert_eq!(store.list(ProjectFilter::Exact(Some("p2"))).len(), 0);
    }
}
