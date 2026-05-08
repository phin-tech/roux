use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use roux_core::{AgentAlias, AliasEvent};
use tauri::{AppHandle, Emitter};

use super::persistence::{self, load_from_path, save_to_path};
use super::store::{AliasStore, BindError, BindRequest, ProjectFilter};

/// Tauri event name emitted on every alias mutation.
pub const ALIAS_EVENT: &str = "alias-event";

/// Clonable handle over the alias store. Wraps `Arc<Mutex<_>>` so all clones
/// share state. Mirrors the `NotificationManager` shape.
///
/// Persistence runs synchronously on every mutation. The on-disk file is
/// small (a few entries × ~150 bytes each) so the cost is negligible and
/// the simpler "save on every change" model avoids stale-state recovery
/// after a crash.
#[derive(Clone)]
pub struct AliasManager {
    inner: Arc<Mutex<AliasStore>>,
    /// `None` = in-memory only (test mode); `Some(path)` = persisted.
    persistence_path: Option<Arc<PathBuf>>,
}

impl AliasManager {
    /// Production constructor: loads from the default config-dir path,
    /// seeds the reserved `me` alias, and writes the seeded state back.
    pub fn load() -> Self {
        Self::load_from(persistence::persistence_path())
    }

    /// Load from an explicit path. Used by tests to redirect to a temp dir.
    /// Always seeds `me` if absent (the reserved human-user mailbox alias).
    pub fn load_from(path: PathBuf) -> Self {
        let entries = load_from_path(&path);
        let had_me = entries.iter().any(|a| a.alias == "me" && a.project_id.is_none());
        let mut store = AliasStore::from_entries(entries);
        if !had_me {
            store.ensure("me", None);
        }
        let mgr = Self {
            inner: Arc::new(Mutex::new(store)),
            persistence_path: Some(Arc::new(path)),
        };
        if !had_me {
            mgr.persist();
        }
        mgr
    }

    /// In-memory variant. No load, no persist on mutations. For tests that
    /// don't care about disk state.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        let mut store = AliasStore::new();
        store.ensure("me", None);
        Self { inner: Arc::new(Mutex::new(store)), persistence_path: None }
    }

    pub fn bind(
        &self,
        canonical: &str,
        req: BindRequest,
        app: Option<&AppHandle>,
    ) -> Result<AgentAlias, BindError> {
        let alias = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            store.bind(canonical, req)?
        };
        self.persist();
        if let Some(app) = app {
            let _ = app.emit(ALIAS_EVENT, &AliasEvent::Set { alias: alias.clone() });
        }
        Ok(alias)
    }

    /// Release every binding tied to `pane_id`. Used on pane close /
    /// rename. `only_auto_claimed=true` preserves manual `roux alias claim`
    /// bindings. Returns `(canonical, project_id)` pairs so the caller
    /// can emit per-scope `Unset` events for the UI.
    pub fn unbind_for_pane(
        &self,
        pane_id: &str,
        only_auto_claimed: bool,
        app: Option<&AppHandle>,
    ) -> Vec<(String, Option<String>)> {
        let released = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            store.unbind_for_pane(pane_id, only_auto_claimed)
        };
        if !released.is_empty() {
            self.persist();
            if let Some(app) = app {
                for (canonical, project_id) in &released {
                    let _ = app.emit(
                        ALIAS_EVENT,
                        &AliasEvent::Unset {
                            canonical: canonical.clone(),
                            project_id: project_id.clone(),
                        },
                    );
                }
            }
        }
        released
    }

    pub fn find_for_pane(&self, pane_id: &str) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.find_for_pane(pane_id).into_iter().cloned().collect()
    }

    /// Try to auto-claim an alias for `pane_id` from `pane_name`.
    ///
    /// Behavior:
    /// - If `pane_name` is `None` or empty → release any auto-claim
    ///   currently held by this pane and return `None`.
    /// - If `pane_name` doesn't pass `validate_user_alias_name` (capitals,
    ///   spaces, reserved) → release auto-claims, return `None`.
    /// - If the canonical alias is already bound to a *different* pane →
    ///   release auto-claims for THIS pane, return `None` (no steal).
    /// - Otherwise: release any prior auto-claim, then bind under the new
    ///   name with `auto_claimed=true`.
    ///
    /// Idempotent: calling repeatedly with the same name is a no-op (after
    /// the first claim succeeds). Pane rename = call with the new name and
    /// the helper handles the old-name release.
    pub fn try_auto_claim_from_pane_name(
        &self,
        pane_id: &str,
        pane_name: Option<&str>,
        project_id: Option<String>,
        app: Option<&AppHandle>,
    ) -> Option<AgentAlias> {
        let trimmed = pane_name.map(str::trim).filter(|s| !s.is_empty());

        // Validate first; if the name doesn't fit the alias format, we
        // still need to release any prior auto-claim (e.g. user renamed
        // a pane from "reviewer" to "Reviewer (v2)").
        let canonical = match trimmed.and_then(|n| roux_core::validate_user_alias_name(n).ok()) {
            Some(c) => c,
            None => {
                self.unbind_for_pane(pane_id, true, app);
                return None;
            }
        };

        // If THIS pane already holds this exact alias, no-op.
        let already_held = {
            let store = self.inner.lock().expect("alias store poisoned");
            store
                .get(&canonical, project_id.as_deref())
                .map(|a| a.pane_id.as_deref() == Some(pane_id))
                .unwrap_or(false)
        };
        if already_held {
            return self.get(&canonical, project_id.as_deref());
        }

        // If the alias is bound elsewhere (different pane), don't steal —
        // but DO release any prior auto-claim this pane held under a
        // different name.
        let bound_elsewhere = {
            let store = self.inner.lock().expect("alias store poisoned");
            match store.get(&canonical, project_id.as_deref()) {
                Some(a) => match a.pane_id.as_deref() {
                    Some(other) => other != pane_id,
                    None => false,
                },
                None => false,
            }
        };
        if bound_elsewhere {
            self.unbind_for_pane(pane_id, true, app);
            return None;
        }

        // Safe to claim. Release prior auto-claims first so the rename
        // case (oldname→newname) doesn't leave a stale binding.
        self.unbind_for_pane(pane_id, true, app);

        match self.bind(
            &canonical,
            BindRequest {
                project_id,
                session_id: None,
                pane_id: Some(pane_id.to_string()),
                auto_claimed: true,
                force: false,
            },
            app,
        ) {
            Ok(alias) => Some(alias),
            Err(_) => None,
        }
    }

    pub fn unbind(
        &self,
        canonical: &str,
        project_id: Option<&str>,
        app: Option<&AppHandle>,
    ) -> bool {
        let changed = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            store.unbind(canonical, project_id)
        };
        if changed {
            self.persist();
            if let Some(app) = app {
                let _ = app.emit(
                    ALIAS_EVENT,
                    &AliasEvent::Unset {
                        canonical: canonical.to_string(),
                        project_id: project_id.map(String::from),
                    },
                );
            }
        }
        changed
    }

    /// Idempotent ensure — creates an unbound entry if missing, returns
    /// the existing entry otherwise. Persists and fires an `AliasEvent::Set`
    /// only when a new entry was actually inserted, so the frontend store
    /// picks up implicit aliases (e.g. `mailbox-post` to a not-yet-claimed
    /// name) without needing a re-hydrate.
    pub fn ensure(
        &self,
        canonical: &str,
        project_id: Option<String>,
        app: Option<&AppHandle>,
    ) -> AgentAlias {
        let (alias, was_new) = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            let before_len = store.entries().len();
            let alias = store.ensure(canonical, project_id);
            let was_new = store.entries().len() > before_len;
            (alias, was_new)
        };
        if was_new {
            self.persist();
            if let Some(app) = app {
                let _ = app.emit(ALIAS_EVENT, &AliasEvent::Set { alias: alias.clone() });
            }
        }
        alias
    }

    pub fn get(&self, canonical: &str, project_id: Option<&str>) -> Option<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.get(canonical, project_id).cloned()
    }

    pub fn find_all_by_name(&self, canonical: &str) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.find_all_by_name(canonical).into_iter().cloned().collect()
    }

    pub fn list(&self, project_filter: ProjectFilter<'_>, only_unbound: bool) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.list(project_filter, only_unbound)
    }

    pub fn whoami(&self, session_id: &str) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.whoami(session_id)
    }

    fn persist(&self) {
        let Some(path) = self.persistence_path.as_ref() else {
            return;
        };
        let store = self.inner.lock().expect("alias store poisoned");
        if let Err(e) = save_to_path(store.entries(), path.as_path()) {
            eprintln!("[roux] alias persistence failed at {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_seeds_me_alias_when_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let mgr = AliasManager::load_from(path.clone());
        assert!(mgr.get("me", None).is_some(), "me alias must be seeded");
        // And persisted back so the seed survives next load.
        assert!(path.exists(), "load_from must write the seeded state");
    }

    #[test]
    fn load_preserves_existing_me_alias() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        // First load creates `me`.
        let mgr = AliasManager::load_from(path.clone());
        // Bind some other alias and verify it persists.
        mgr.bind("reviewer", BindRequest { session_id: Some("sess-1".into()), ..Default::default() }, None).unwrap();
        // Reload from same path.
        let mgr2 = AliasManager::load_from(path);
        assert!(mgr2.get("me", None).is_some());
        assert_eq!(mgr2.get("reviewer", None).unwrap().session_id.as_deref(), Some("sess-1"));
    }

    #[test]
    fn bind_persists_to_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let mgr = AliasManager::load_from(path.clone());
        mgr.bind("reviewer", BindRequest { session_id: Some("sess-1".into()), ..Default::default() }, None).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let aliases = envelope["data"].as_array().unwrap();
        assert!(aliases.iter().any(|a| a["alias"] == "reviewer"));
    }

    #[test]
    fn unbind_persists_when_changed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let mgr = AliasManager::load_from(path.clone());
        mgr.bind("reviewer", BindRequest { session_id: Some("sess-1".into()), ..Default::default() }, None).unwrap();
        assert!(mgr.unbind("reviewer", None, None));

        let mgr2 = AliasManager::load_from(path);
        assert!(mgr2.get("reviewer", None).unwrap().session_id.is_none());
    }

    #[test]
    fn unbind_returns_false_when_nothing_changes() {
        let mgr = AliasManager::in_memory();
        // `me` is unbound by default; unbinding it again should report no change.
        assert!(!mgr.unbind("me", None, None));
    }

    #[test]
    fn bind_force_overrides_existing_binding() {
        let mgr = AliasManager::in_memory();
        mgr.bind(
            "reviewer",
            BindRequest { session_id: Some("sess-1".into()), ..Default::default() },
            None,
        )
        .unwrap();
        let err = mgr
            .bind(
                "reviewer",
                BindRequest { session_id: Some("sess-2".into()), ..Default::default() },
                None,
            )
            .unwrap_err();
        assert!(matches!(err, BindError::AlreadyBoundElsewhere { .. }));
        let stolen = mgr
            .bind(
                "reviewer",
                BindRequest { session_id: Some("sess-2".into()), force: true, ..Default::default() },
                None,
            )
            .unwrap();
        assert_eq!(stolen.session_id.as_deref(), Some("sess-2"));
    }

    fn rs(session: &str) -> BindRequest {
        BindRequest { session_id: Some(session.into()), ..Default::default() }
    }

    #[test]
    fn whoami_finds_aliases_held_by_session() {
        let mgr = AliasManager::in_memory();
        mgr.bind("alpha", rs("sess-1"), None).unwrap();
        mgr.bind("beta", rs("sess-1"), None).unwrap();
        mgr.bind("gamma", rs("sess-2"), None).unwrap();

        let mine = mgr.whoami("sess-1");
        let mut names: Vec<_> = mine.into_iter().map(|a| a.alias).collect();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn list_filters_correctly() {
        let mgr = AliasManager::in_memory();
        mgr.bind("a", rs("s"), None).unwrap();
        mgr.bind(
            "b",
            BindRequest {
                project_id: Some("p1".into()),
                session_id: Some("s".into()),
                ..Default::default()
            },
            None,
        )
        .unwrap();

        let all = mgr.list(ProjectFilter::Any, false);
        // me + a + b
        assert_eq!(all.len(), 3);

        let global_only = mgr.list(ProjectFilter::Exact(None), false);
        // me + a
        assert_eq!(global_only.len(), 2);

        let p1_only = mgr.list(ProjectFilter::Exact(Some("p1")), false);
        assert_eq!(p1_only.len(), 1);
        assert_eq!(p1_only[0].alias, "b");
    }

    #[test]
    fn ensure_is_idempotent_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let mgr = AliasManager::load_from(path.clone());
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        // Sleep a tick so mtime would change if a write happened.
        std::thread::sleep(std::time::Duration::from_millis(10));
        mgr.ensure("me", None, None); // already exists
        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after, "ensure on existing entry must not rewrite the file");
    }

    #[test]
    fn ensure_writes_when_new_entry_added() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let mgr = AliasManager::load_from(path.clone());
        mgr.ensure("freshly-created", None, None);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("freshly-created"));
    }

    // ── Phase 1.5: pane-name auto-claim ─────────────────────────────────

    #[test]
    fn auto_claim_binds_when_pane_name_is_valid_alias() {
        let mgr = AliasManager::in_memory();
        let alias = mgr
            .try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None)
            .expect("should claim");
        assert_eq!(alias.alias, "reviewer");
        assert_eq!(alias.pane_id.as_deref(), Some("pane-A"));
        assert!(alias.auto_claimed);
    }

    #[test]
    fn auto_claim_skips_when_name_has_invalid_chars() {
        let mgr = AliasManager::in_memory();
        // Capitals, spaces, parens — all rejected by validate_user_alias_name.
        assert!(mgr
            .try_auto_claim_from_pane_name("pane-A", Some("Reviewer (v2)"), None, None)
            .is_none());
        assert!(mgr.find_for_pane("pane-A").is_empty());
    }

    #[test]
    fn auto_claim_skips_reserved_name() {
        let mgr = AliasManager::in_memory();
        assert!(mgr.try_auto_claim_from_pane_name("pane-A", Some("me"), None, None).is_none());
    }

    #[test]
    fn auto_claim_idempotent_for_same_pane_and_name() {
        let mgr = AliasManager::in_memory();
        let first = mgr
            .try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None)
            .expect("first claim");
        let second = mgr
            .try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None)
            .expect("re-claim");
        assert_eq!(first.created_at, second.created_at);
    }

    #[test]
    fn auto_claim_does_not_steal_from_other_pane() {
        let mgr = AliasManager::in_memory();
        mgr.try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None);
        // A second pane named "reviewer" can't auto-claim — pane-A holds it.
        assert!(mgr.try_auto_claim_from_pane_name("pane-B", Some("reviewer"), None, None).is_none());
        // pane-A still holds it.
        let held = mgr.find_for_pane("pane-A");
        assert_eq!(held[0].alias, "reviewer");
    }

    #[test]
    fn auto_claim_releases_prior_auto_claim_on_rename() {
        let mgr = AliasManager::in_memory();
        // Pane starts as "reviewer".
        mgr.try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None);
        // Renamed to "builder".
        let after = mgr
            .try_auto_claim_from_pane_name("pane-A", Some("builder"), None, None)
            .expect("rename should re-claim");
        assert_eq!(after.alias, "builder");
        // The old "reviewer" alias is now unbound (pane_id cleared).
        let held = mgr.find_for_pane("pane-A");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].alias, "builder");
    }

    #[test]
    fn auto_claim_invalid_rename_releases_old_binding() {
        let mgr = AliasManager::in_memory();
        mgr.try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None);
        // Rename to something that can't be canonicalized to a valid
        // alias (space + parens).
        let result =
            mgr.try_auto_claim_from_pane_name("pane-A", Some("Reviewer (v2)"), None, None);
        assert!(result.is_none(), "invalid name should not claim");
        // The pane has no auto-claimed binding anymore.
        assert!(mgr.find_for_pane("pane-A").is_empty());
    }

    #[test]
    fn auto_claim_preserves_manual_claim_on_pane() {
        let mgr = AliasManager::in_memory();
        // Manual claim binds with auto_claimed=false.
        mgr.bind(
            "manual-name",
            BindRequest {
                session_id: Some("sess-1".into()),
                pane_id: Some("pane-A".into()),
                auto_claimed: false,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        // Now pane gets renamed to a valid alias name.
        let auto = mgr
            .try_auto_claim_from_pane_name("pane-A", Some("auto-name"), None, None)
            .expect("auto-claim still works alongside manual");
        assert!(auto.auto_claimed);
        // Both bindings exist.
        let held = mgr.find_for_pane("pane-A");
        assert_eq!(held.len(), 2);
    }

    #[test]
    fn auto_claim_with_empty_name_releases_existing_auto_claim() {
        let mgr = AliasManager::in_memory();
        mgr.try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None);
        let result =
            mgr.try_auto_claim_from_pane_name("pane-A", Some(""), None, None);
        assert!(result.is_none());
        assert!(mgr.find_for_pane("pane-A").is_empty());
    }

    #[test]
    fn unbind_for_pane_releases_only_auto_when_only_auto_claimed_true() {
        let mgr = AliasManager::in_memory();
        mgr.bind(
            "manual",
            BindRequest {
                session_id: Some("sess-1".into()),
                pane_id: Some("pane-A".into()),
                auto_claimed: false,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        mgr.try_auto_claim_from_pane_name("pane-A", Some("auto"), None, None);

        let released = mgr.unbind_for_pane("pane-A", true, None);
        assert_eq!(released, vec![("auto".to_string(), None)]);
        // Manual binding is still in place.
        let held = mgr.find_for_pane("pane-A");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].alias, "manual");
    }
}
