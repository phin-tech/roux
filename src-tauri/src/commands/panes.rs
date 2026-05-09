use crate::pane_service::PaneRecord;
use crate::session_service::SessionHandle;
use crate::state::AppState;

/// Resolve the project scope of the PTY at `pty_id` by walking pty →
/// session → project_id. Returns `None` (the global scope) when any
/// link is missing — e.g. the PTY hasn't registered yet or its session
/// has no project. This is best-effort: a later upsert (rename,
/// replacePty) re-resolves with whatever state exists at that moment.
///
/// `pty_session_lookup` maps a pty_id → its session_id. In production
/// the caller passes a closure over `PtyManager::get_info_direct`;
/// tests pass a closure backed by a simple map so they don't need a
/// live PTY manager.
async fn pty_project_scope(
    session_handle: &SessionHandle,
    pty_session_lookup: impl Fn(&str) -> Option<String>,
    pty_id: &str,
) -> Option<String> {
    let session_id = pty_session_lookup(pty_id)?;
    let session = session_handle.get(&session_id).await.ok().flatten()?;
    session.project_id
}

#[tauri::command]
pub(crate) async fn upsert_pane_record(
    record: PaneRecord,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let pane_id = record.id.clone();
    let pane_name = record.name.clone();
    // Capture pty_id BEFORE the upsert moves the record so the resolver
    // doesn't need a second round-trip through the pane service to
    // recover what we already had.
    let pty_id = record.pty_id.clone();
    state.pane_handle.upsert(record).await.map_err(|e| e.to_string())?;

    // Try to auto-claim an alias from the pane's name. No-op if the
    // name doesn't match the alias format (capitals, spaces, reserved)
    // or if the canonical alias is already held by another pane.
    //
    // Resolve the pane's project scope from the live PTY/session graph
    // so two panes named "reviewer" in different projects can both
    // auto-claim. Falls back to the global scope when the lookup chain
    // breaks (e.g. PTY isn't registered yet at first upsert) — a later
    // upsert (rename, replacePty) re-runs the resolver and corrects the
    // scope.
    let pty_manager = state.pty_manager.clone();
    let project_id = pty_project_scope(
        &state.session_handle,
        move |pty| pty_manager.get_info_direct(pty).and_then(|info| info.session_id),
        &pty_id,
    )
    .await;

    state.alias_manager.try_auto_claim_from_pane_name(
        &pane_id,
        pane_name.as_deref(),
        project_id,
        Some(&app),
    );
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_pane_record(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.pane_handle.remove(&id).await.map_err(|e| e.to_string())?;

    // Release auto-claimed aliases held by this pane. Manual `roux alias
    // claim` bindings persist — queued mail addressed to them survives
    // for the next session that claims them.
    state.alias_manager.unbind_for_pane(&id, true, Some(&app));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_service;
    use roux_core::Session;

    fn session_with_project(id: &str, project_id: Option<&str>) -> Session {
        Session {
            id: id.into(),
            name: format!("Session {id}"),
            repo_root: "/tmp/repo".into(),
            worktree_path: "/tmp/repo".into(),
            branch: "main".into(),
            is_worktree: false,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: project_id.map(str::to_string),
            is_git_repo: false,
            name_override: None,
            primary_pty_id: None,
            archived: false,
            ended_at: None,
            blueprint_id: None,
            pinned_pr_url: None,
            smol_machine_name: None,
        }
    }

    /// Helper: build a closure over a static pty→session table so the
    /// tests don't need a live PtyManager.
    fn pty_lookup(table: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |pty_id| {
            table
                .iter()
                .find(|(p, _)| *p == pty_id)
                .map(|(_, s)| s.to_string())
        }
    }

    #[tokio::test]
    async fn pty_project_scope_resolves_via_pty_to_session_project() {
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = session_service::spawn_with_path(
            vec![session_with_project("sess-1", Some("proj-A"))],
            dir.path().join("sessions.json"),
        );

        let project =
            pty_project_scope(&sessions, pty_lookup(&[("pty-X", "sess-1")]), "pty-X").await;
        assert_eq!(project.as_deref(), Some("proj-A"));
    }

    #[tokio::test]
    async fn pty_project_scope_returns_none_when_session_has_no_project() {
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = session_service::spawn_with_path(
            vec![session_with_project("sess-1", None)],
            dir.path().join("sessions.json"),
        );

        let project =
            pty_project_scope(&sessions, pty_lookup(&[("pty-X", "sess-1")]), "pty-X").await;
        assert!(project.is_none());
    }

    #[tokio::test]
    async fn pty_project_scope_returns_none_when_pty_lookup_fails() {
        // PTY hasn't registered yet at the moment upsert fires. Resolver
        // gracefully returns None; auto-claim lands in the global scope
        // and a later upsert can re-resolve.
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));

        let project = pty_project_scope(&sessions, pty_lookup(&[]), "pty-missing").await;
        assert!(project.is_none());
    }

    #[tokio::test]
    async fn pty_project_scope_returns_none_when_session_missing() {
        // PTY says session-X but session-X isn't in the session store.
        // Could happen during a tear-down race. Resolver returns None
        // rather than panicking.
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));

        let project = pty_project_scope(
            &sessions,
            pty_lookup(&[("pty-X", "missing-sess")]),
            "pty-X",
        )
        .await;
        assert!(project.is_none());
    }

    /// Cross-project auto-claim integration: two panes named `reviewer`,
    /// each in a different project, both succeed. Pre-fix, the second
    /// would silently fail to claim because the first held the alias
    /// in the global scope.
    #[tokio::test]
    async fn cross_project_auto_claim_does_not_collide() {
        use roux_lib::aliases::AliasManager;

        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = session_service::spawn_with_path(
            vec![
                session_with_project("sess-A", Some("proj-A")),
                session_with_project("sess-B", Some("proj-B")),
            ],
            dir.path().join("sessions.json"),
        );

        let lookup = pty_lookup(&[("pty-A", "sess-A"), ("pty-B", "sess-B")]);
        let project_a = pty_project_scope(&sessions, &lookup, "pty-A").await;
        let project_b = pty_project_scope(&sessions, &lookup, "pty-B").await;
        assert_eq!(project_a.as_deref(), Some("proj-A"));
        assert_eq!(project_b.as_deref(), Some("proj-B"));

        let alias_dir = tempfile::tempdir().unwrap();
        let alias_mgr = AliasManager::load_from(alias_dir.path().join("aliases.json"));
        let claim_a = alias_mgr
            .try_auto_claim_from_pane_name("pane-A", Some("reviewer"), project_a, None)
            .expect("pane-A should claim");
        let claim_b = alias_mgr
            .try_auto_claim_from_pane_name("pane-B", Some("reviewer"), project_b, None)
            .expect("pane-B should also claim — different project scope");

        assert_eq!(claim_a.project_id.as_deref(), Some("proj-A"));
        assert_eq!(claim_a.pane_id.as_deref(), Some("pane-A"));
        assert_eq!(claim_b.project_id.as_deref(), Some("proj-B"));
        assert_eq!(claim_b.pane_id.as_deref(), Some("pane-B"));
    }

    /// Late-binding transition: the first upsert fires before the PTY
    /// is registered (so the resolver returns `None` and auto-claim lands
    /// in the global scope). A subsequent upsert (e.g. on rename or
    /// replacePty) finds the now-registered PTY and re-resolves to the
    /// project scope. The old global-scope binding for this pane must be
    /// released so we don't leave a stale entry behind.
    #[tokio::test]
    async fn auto_claim_transitions_from_global_to_project_scope() {
        use roux_lib::aliases::AliasManager;

        let alias_dir = tempfile::tempdir().unwrap();
        let alias_mgr = AliasManager::load_from(alias_dir.path().join("aliases.json"));

        // First call: PTY not registered yet, project resolves to None.
        alias_mgr
            .try_auto_claim_from_pane_name("pane-A", Some("reviewer"), None, None)
            .expect("first claim succeeds in global scope");
        assert_eq!(
            alias_mgr.get("reviewer", None).and_then(|a| a.pane_id),
            Some("pane-A".to_string()),
        );

        // Second call: PTY now registered, project resolves to proj-A.
        alias_mgr
            .try_auto_claim_from_pane_name(
                "pane-A",
                Some("reviewer"),
                Some("proj-A".into()),
                None,
            )
            .expect("re-claim under project scope succeeds");

        // The new project-scoped binding is held by pane-A.
        assert_eq!(
            alias_mgr.get("reviewer", Some("proj-A")).and_then(|a| a.pane_id),
            Some("pane-A".to_string()),
        );
        // The old global-scope entry was released (pane_id cleared) so
        // it doesn't shadow the project-scoped one.
        assert!(
            alias_mgr.get("reviewer", None).and_then(|a| a.pane_id).is_none(),
            "old global-scope binding must be released",
        );
    }

    /// Same-project collision still respects the original "first wins"
    /// rule. The fix scopes by project — it doesn't loosen the conflict
    /// model within a scope.
    #[tokio::test]
    async fn same_project_auto_claim_still_collides() {
        use roux_lib::aliases::AliasManager;

        let alias_dir = tempfile::tempdir().unwrap();
        let alias_mgr = AliasManager::load_from(alias_dir.path().join("aliases.json"));
        alias_mgr
            .try_auto_claim_from_pane_name(
                "pane-A",
                Some("reviewer"),
                Some("proj-A".into()),
                None,
            )
            .expect("pane-A claims first");
        let second = alias_mgr.try_auto_claim_from_pane_name(
            "pane-B",
            Some("reviewer"),
            Some("proj-A".into()),
            None,
        );
        assert!(second.is_none(), "second pane in same project must not steal");
    }
}
