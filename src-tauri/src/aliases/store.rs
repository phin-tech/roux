use roux_core::AgentAlias;

/// Filter helper for list operations. `Any` matches every project; `Exact`
/// matches one specific project (or global, when the inner value is `None`).
#[derive(Debug, Clone, Copy)]
pub enum ProjectFilter<'a> {
    Any,
    Exact(Option<&'a str>),
}

impl<'a> ProjectFilter<'a> {
    pub fn matches(&self, candidate: Option<&str>) -> bool {
        match self {
            ProjectFilter::Any => true,
            ProjectFilter::Exact(target) => candidate == *target,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BindError {
    #[error(
        "alias '{canonical}' is already bound to {current}; pass force=true to override"
    )]
    AlreadyBoundElsewhere { canonical: String, current: String },
}

/// Bind request bag. Use `BindRequest::default()` then set the fields you
/// care about; reduces multi-positional-arg call ergonomics to one struct
/// literal per call site.
#[derive(Debug, Clone, Default)]
pub struct BindRequest {
    /// Project scope. `None` means global (typically the human-user `me`).
    pub project_id: Option<String>,
    /// Optional cached session parent. Auto-claim from pane name leaves
    /// this `None` when the session linkage isn't known. Required by the
    /// explicit `roux alias claim` flow, which is rejected upstream when
    /// the caller isn't inside a session.
    pub session_id: Option<String>,
    /// Canonical addressable target. When `None`, the alias falls back
    /// to the session's primary pane at delivery time (Phase-1 compat).
    pub pane_id: Option<String>,
    /// `true` when the alias was derived from the pane's name. Released
    /// on pane rename or close.
    pub auto_claimed: bool,
    /// Override an existing bound-elsewhere binding. Manual claims with
    /// `--steal`, or auto-claim races, set this.
    pub force: bool,
}

/// In-memory alias store. Entries are keyed by `(canonical_alias, project_id)`,
/// so the same name may exist in multiple project scopes.
///
/// Persistence and event emission live in `AliasManager` (mirrors the
/// `NotificationStore` / `NotificationManager` split).
pub struct AliasStore {
    entries: Vec<AgentAlias>,
}

impl AliasStore {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn from_entries(entries: Vec<AgentAlias>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[AgentAlias] {
        &self.entries
    }

    /// Bind `canonical` (within `req.project_id` scope) to the
    /// `(session_id, pane_id)` pair from `req`.
    ///
    /// Conflict rules:
    /// - If the alias is already bound to a *different* pane, conflict
    ///   unless `req.force`.
    /// - If the alias is bound only at session-level (legacy Phase 1)
    ///   and the new session differs, conflict unless `req.force`.
    pub fn bind(
        &mut self,
        canonical: &str,
        req: BindRequest,
    ) -> Result<AgentAlias, BindError> {
        let now = now_ms();
        if let Some(entry) = self.find_mut(canonical, req.project_id.as_deref()) {
            if let Some(conflict) =
                current_binding_conflict(entry, req.session_id.as_deref(), req.pane_id.as_deref())
            {
                if !req.force {
                    return Err(BindError::AlreadyBoundElsewhere {
                        canonical: canonical.to_string(),
                        current: conflict,
                    });
                }
            }
            entry.session_id = req.session_id;
            entry.pane_id = req.pane_id;
            entry.auto_claimed = req.auto_claimed;
            entry.updated_at = now;
            return Ok(entry.clone());
        }
        let mut entry = AgentAlias::new(canonical, req.project_id);
        entry.session_id = req.session_id;
        entry.pane_id = req.pane_id;
        entry.auto_claimed = req.auto_claimed;
        self.entries.push(entry.clone());
        Ok(entry)
    }

    /// Clear `canonical`'s binding (within `project_id` scope). Entry is
    /// retained so addressed mail still has a valid target. Returns true
    /// when a binding was actually cleared.
    pub fn unbind(&mut self, canonical: &str, project_id: Option<&str>) -> bool {
        let now = now_ms();
        if let Some(entry) = self.find_mut(canonical, project_id) {
            if entry.is_bound() {
                entry.session_id = None;
                entry.pane_id = None;
                entry.auto_claimed = false;
                entry.updated_at = now;
                return true;
            }
        }
        false
    }

    /// Release every binding currently tied to `pane_id`. Used when a
    /// pane is destroyed or renamed. `only_auto_claimed=true` limits the
    /// release to bindings the system created automatically from the
    /// pane's name; manual `roux alias claim` bindings are preserved.
    /// Returns the names of the released aliases (in canonical form) so
    /// the caller can fan out events.
    pub fn unbind_for_pane(&mut self, pane_id: &str, only_auto_claimed: bool) -> Vec<String> {
        let now = now_ms();
        let mut released = Vec::new();
        for entry in self.entries.iter_mut() {
            if entry.pane_id.as_deref() != Some(pane_id) {
                continue;
            }
            if only_auto_claimed && !entry.auto_claimed {
                continue;
            }
            entry.session_id = None;
            entry.pane_id = None;
            entry.auto_claimed = false;
            entry.updated_at = now;
            released.push(entry.alias.clone());
        }
        released
    }

    /// Aliases currently bound to the given pane. Typically zero or one
    /// in practice (auto-claim binds a single name from the pane's name),
    /// but the API returns a Vec because manual claims can stack.
    pub fn find_for_pane(&self, pane_id: &str) -> Vec<&AgentAlias> {
        self.entries
            .iter()
            .filter(|a| a.pane_id.as_deref() == Some(pane_id))
            .collect()
    }

    /// Ensure `(canonical, project_id)` exists. If already present, returns
    /// it unchanged. If absent, creates it as an unbound entry. Used both
    /// to seed system aliases (`me`) and to materialize implicit alias
    /// records when posting to a never-claimed name.
    pub fn ensure(&mut self, canonical: &str, project_id: Option<String>) -> AgentAlias {
        if let Some(entry) = self.find_mut(canonical, project_id.as_deref()) {
            return entry.clone();
        }
        let entry = AgentAlias::new(canonical, project_id);
        self.entries.push(entry.clone());
        entry
    }

    pub fn get(&self, canonical: &str, project_id: Option<&str>) -> Option<&AgentAlias> {
        self.entries
            .iter()
            .find(|a| a.alias == canonical && a.project_id.as_deref() == project_id)
    }

    /// All entries matching `canonical` regardless of project scope. Used
    /// for bare-alias resolution: caller checks for ambiguity (>1 result)
    /// and demands a project qualification when needed.
    pub fn find_all_by_name(&self, canonical: &str) -> Vec<&AgentAlias> {
        self.entries.iter().filter(|a| a.alias == canonical).collect()
    }

    pub fn list(&self, project_filter: ProjectFilter<'_>, only_unbound: bool) -> Vec<AgentAlias> {
        self.entries
            .iter()
            .filter(|a| project_filter.matches(a.project_id.as_deref()))
            .filter(|a| !only_unbound || a.session_id.is_none())
            .cloned()
            .collect()
    }

    /// Aliases currently bound to the given session.
    pub fn whoami(&self, session_id: &str) -> Vec<AgentAlias> {
        self.entries
            .iter()
            .filter(|a| a.session_id.as_deref() == Some(session_id))
            .cloned()
            .collect()
    }

    fn find_mut(
        &mut self,
        canonical: &str,
        project_id: Option<&str>,
    ) -> Option<&mut AgentAlias> {
        self.entries
            .iter_mut()
            .find(|a| a.alias == canonical && a.project_id.as_deref() == project_id)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for AliasStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns a human-readable description of an existing binding when the
/// requested new binding would conflict, or `None` when there is no
/// conflict. Pane-level conflicts win over session-level (a Phase-1.5
/// pane-bound alias is more specific than a Phase-1 session-bound one).
fn current_binding_conflict(
    existing: &AgentAlias,
    new_session: Option<&str>,
    new_pane: Option<&str>,
) -> Option<String> {
    match (existing.pane_id.as_deref(), new_pane) {
        (Some(eid), Some(nid)) if eid != nid => Some(format!("pane '{eid}'")),
        (Some(eid), None) => Some(format!("pane '{eid}'")),
        _ => match (existing.session_id.as_deref(), new_session, new_pane) {
            (Some(esid), Some(ns), None) if esid != ns => Some(format!("session '{esid}'")),
            _ => None,
        },
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

    /// Test-only convenience: build a session-only `BindRequest`, mirroring
    /// the Phase-1 default of "no pane, manual bind."
    fn req(session: &str) -> BindRequest {
        BindRequest { session_id: Some(session.into()), ..Default::default() }
    }

    #[test]
    fn bind_creates_unknown_alias() {
        let mut store = AliasStore::new();
        let a = store.bind("reviewer", req("sess-1")).unwrap();
        assert_eq!(a.alias, "reviewer");
        assert_eq!(a.session_id.as_deref(), Some("sess-1"));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn bind_rebinds_when_same_session() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req("sess-1")).unwrap();
        let again = store.bind("reviewer", req("sess-1")).unwrap();
        assert_eq!(again.session_id.as_deref(), Some("sess-1"));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn bind_rejects_when_bound_elsewhere() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req("sess-1")).unwrap();
        let err = store.bind("reviewer", req("sess-2")).unwrap_err();
        assert_eq!(
            err,
            BindError::AlreadyBoundElsewhere {
                canonical: "reviewer".into(),
                current: "session 'sess-1'".into(),
            }
        );
        assert_eq!(store.get("reviewer", None).unwrap().session_id.as_deref(), Some("sess-1"));
    }

    #[test]
    fn bind_force_overrides_existing() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req("sess-1")).unwrap();
        let stolen = store
            .bind(
                "reviewer",
                BindRequest { session_id: Some("sess-2".into()), force: true, ..Default::default() },
            )
            .unwrap();
        assert_eq!(stolen.session_id.as_deref(), Some("sess-2"));
    }

    #[test]
    fn bind_after_unbind_succeeds_without_force() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req("sess-1")).unwrap();
        assert!(store.unbind("reviewer", None));
        let rebind = store.bind("reviewer", req("sess-2")).unwrap();
        assert_eq!(rebind.session_id.as_deref(), Some("sess-2"));
    }

    #[test]
    fn unbind_clears_session_but_keeps_entry() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req("sess-1")).unwrap();
        assert!(store.unbind("reviewer", None));
        let entry = store.get("reviewer", None).unwrap();
        assert!(entry.session_id.is_none());
        assert!(entry.pane_id.is_none());
        assert_eq!(store.len(), 1, "entry must persist for queued mail");
    }

    #[test]
    fn unbind_returns_false_when_already_unbound() {
        let mut store = AliasStore::new();
        store.ensure("reviewer", None);
        assert!(!store.unbind("reviewer", None));
    }

    #[test]
    fn unbind_returns_false_for_missing_alias() {
        let mut store = AliasStore::new();
        assert!(!store.unbind("ghost", None));
    }

    #[test]
    fn ensure_creates_unbound_when_missing() {
        let mut store = AliasStore::new();
        let a = store.ensure("reviewer", None);
        assert_eq!(a.alias, "reviewer");
        assert!(a.session_id.is_none());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn ensure_is_idempotent() {
        let mut store = AliasStore::new();
        let first = store.ensure("reviewer", None);
        let second = store.ensure("reviewer", None);
        assert_eq!(first.created_at, second.created_at);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn project_scoped_aliases_are_independent() {
        let mut store = AliasStore::new();
        store
            .bind(
                "reviewer",
                BindRequest {
                    project_id: Some("proj-a".into()),
                    session_id: Some("sess-a".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .bind(
                "reviewer",
                BindRequest {
                    project_id: Some("proj-b".into()),
                    session_id: Some("sess-b".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store.bind("reviewer", req("sess-global")).unwrap();
        assert_eq!(store.len(), 3);

        assert_eq!(
            store.get("reviewer", Some("proj-a")).unwrap().session_id.as_deref(),
            Some("sess-a")
        );
        assert_eq!(
            store.get("reviewer", Some("proj-b")).unwrap().session_id.as_deref(),
            Some("sess-b")
        );
        assert_eq!(
            store.get("reviewer", None).unwrap().session_id.as_deref(),
            Some("sess-global")
        );
    }

    #[test]
    fn find_all_by_name_returns_every_scope() {
        let mut store = AliasStore::new();
        for (proj, sess) in [(Some("proj-a"), "s1"), (Some("proj-b"), "s2")] {
            store
                .bind(
                    "reviewer",
                    BindRequest {
                        project_id: proj.map(String::from),
                        session_id: Some(sess.into()),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        store.bind("other", req("s3")).unwrap();
        let matches = store.find_all_by_name("reviewer");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn list_filters_by_project_scope() {
        let mut store = AliasStore::new();
        store.bind("a", req("s")).unwrap();
        store
            .bind(
                "b",
                BindRequest {
                    project_id: Some("p1".into()),
                    session_id: Some("s".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .bind(
                "c",
                BindRequest {
                    project_id: Some("p2".into()),
                    session_id: Some("s".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(store.list(ProjectFilter::Any, false).len(), 3);
        assert_eq!(store.list(ProjectFilter::Exact(None), false).len(), 1);
        assert_eq!(store.list(ProjectFilter::Exact(Some("p1")), false).len(), 1);
        assert_eq!(store.list(ProjectFilter::Exact(Some("missing")), false).len(), 0);
    }

    #[test]
    fn list_only_unbound_filters_correctly() {
        let mut store = AliasStore::new();
        store.bind("bound", req("sess")).unwrap();
        store.ensure("unbound", None);
        let only_unbound = store.list(ProjectFilter::Any, true);
        assert_eq!(only_unbound.len(), 1);
        assert_eq!(only_unbound[0].alias, "unbound");
    }

    #[test]
    fn whoami_lists_aliases_for_session() {
        let mut store = AliasStore::new();
        store.bind("alpha", req("sess-1")).unwrap();
        store.bind("beta", req("sess-1")).unwrap();
        store.bind("gamma", req("sess-2")).unwrap();
        store.ensure("orphan", None);

        let names: Vec<_> = store.whoami("sess-1").iter().map(|a| a.alias.clone()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
        assert_eq!(store.whoami("sess-2").len(), 1);
        assert_eq!(store.whoami("missing").len(), 0);
    }

    #[test]
    fn from_entries_round_trip() {
        let mut store = AliasStore::new();
        store.bind("alpha", req("sess-1")).unwrap();
        store
            .bind(
                "beta",
                BindRequest {
                    project_id: Some("proj".into()),
                    session_id: Some("sess-2".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let snapshot: Vec<_> = store.entries().to_vec();

        let restored = AliasStore::from_entries(snapshot);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.get("alpha", None).unwrap().session_id.as_deref(), Some("sess-1"));
        assert_eq!(
            restored.get("beta", Some("proj")).unwrap().session_id.as_deref(),
            Some("sess-2")
        );
    }

    // ── Phase 1.5: pane-level binding ─────────────────────────────────────

    fn req_pane(session: &str, pane: &str) -> BindRequest {
        BindRequest {
            session_id: Some(session.into()),
            pane_id: Some(pane.into()),
            ..Default::default()
        }
    }

    #[test]
    fn bind_with_pane_records_pane_id() {
        let mut store = AliasStore::new();
        let a = store.bind("reviewer", req_pane("sess-1", "pane-A")).unwrap();
        assert_eq!(a.pane_id.as_deref(), Some("pane-A"));
        assert_eq!(a.session_id.as_deref(), Some("sess-1"));
        assert!(!a.auto_claimed);
    }

    #[test]
    fn bind_pane_conflicts_with_other_pane() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req_pane("sess-1", "pane-A")).unwrap();
        let err = store.bind("reviewer", req_pane("sess-1", "pane-B")).unwrap_err();
        match err {
            BindError::AlreadyBoundElsewhere { current, .. } => {
                assert!(current.contains("pane 'pane-A'"));
            }
        }
    }

    #[test]
    fn bind_force_overrides_pane() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req_pane("sess-1", "pane-A")).unwrap();
        let stolen = store
            .bind(
                "reviewer",
                BindRequest {
                    session_id: Some("sess-1".into()),
                    pane_id: Some("pane-B".into()),
                    force: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(stolen.pane_id.as_deref(), Some("pane-B"));
    }

    #[test]
    fn auto_claimed_flag_round_trips() {
        let mut store = AliasStore::new();
        let a = store
            .bind(
                "reviewer",
                BindRequest {
                    session_id: Some("sess-1".into()),
                    pane_id: Some("pane-A".into()),
                    auto_claimed: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(a.auto_claimed);
    }

    #[test]
    fn unbind_for_pane_releases_only_matching_pane() {
        let mut store = AliasStore::new();
        store
            .bind(
                "reviewer",
                BindRequest {
                    session_id: Some("sess-1".into()),
                    pane_id: Some("pane-A".into()),
                    auto_claimed: true,
                    ..Default::default()
                },
            )
            .unwrap();
        store.bind("other", req_pane("sess-1", "pane-B")).unwrap();
        let released = store.unbind_for_pane("pane-A", false);
        assert_eq!(released, vec!["reviewer".to_string()]);
        assert!(store.get("reviewer", None).unwrap().pane_id.is_none());
        assert_eq!(store.get("other", None).unwrap().pane_id.as_deref(), Some("pane-B"));
    }

    #[test]
    fn unbind_for_pane_skips_manual_when_only_auto() {
        let mut store = AliasStore::new();
        store
            .bind(
                "manual",
                BindRequest {
                    session_id: Some("sess-1".into()),
                    pane_id: Some("pane-A".into()),
                    auto_claimed: false,
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .bind(
                "auto",
                BindRequest {
                    session_id: Some("sess-1".into()),
                    pane_id: Some("pane-A".into()),
                    auto_claimed: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let released = store.unbind_for_pane("pane-A", true);
        assert_eq!(released, vec!["auto".to_string()]);
        // manual binding survived
        assert_eq!(
            store.get("manual", None).unwrap().pane_id.as_deref(),
            Some("pane-A")
        );
    }

    #[test]
    fn find_for_pane_returns_only_that_panes_aliases() {
        let mut store = AliasStore::new();
        store.bind("reviewer", req_pane("s", "pane-A")).unwrap();
        store.bind("builder", req_pane("s", "pane-B")).unwrap();
        let mine: Vec<_> = store.find_for_pane("pane-A").iter().map(|a| a.alias.clone()).collect();
        assert_eq!(mine, vec!["reviewer"]);
    }
}
