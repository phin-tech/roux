use crate::services::sessions as svc;
use crate::session::Session;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

/// Options bag for `create_session_shell`. Bundled because Specta caps command
/// signatures at 10 params, and the Claude/Codex/worktree spawn paths all
/// share the same set of optional configuration.
#[derive(Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateShellOpts {
    #[serde(default)]
    pub nono_profile: Option<String>,
    #[serde(default)]
    pub nono_allow_dirs: Option<Vec<String>>,
    /// Spawn-profile id (`claude`, `codex`, user-profile id, …). Passed to
    /// the PTY env so agents wake up under the right profile.
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub initial_size: Option<(u16, u16)>,
    /// Git starting point for a new worktree's branch (e.g. "main",
    /// "origin/main"). Ignored unless `branch` is set and new.
    #[serde(default)]
    pub base: Option<String>,
    /// Run `git fetch origin` before resolving `base`. Used for
    /// `origin/*`-style bases that may be stale locally.
    #[serde(default)]
    pub fetch_first: Option<bool>,
    /// Project to attach the new session to. When set, the PTY env vars
    /// for the project notes + `ROUX_PROJECT_CONTEXT_PATHS` are populated
    /// on the very first spawn (no reconnect required).
    #[serde(default)]
    pub project_id: Option<String>,
    /// Project session-blueprint id this session was spawned from. Stamped
    /// onto the persisted Session so the sidebar can collapse the dimmed
    /// blueprint row when the live session is up.
    #[serde(default)]
    pub blueprint_id: Option<String>,
    /// Smol-machine binding for this session. When set, the session's
    /// primary PTY (and every subsequent shell pane) runs inside the
    /// named VM via `smolvm machine exec`. Empty/missing means the
    /// session runs on the host as usual.
    #[serde(default)]
    pub smol_machine_name: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn write_to_session(
    id: String,
    data: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.pty_manager.write(&id, data.as_bytes()).map_err(|e| e.to_string())
}

/// Frontend reply for a socket-initiated round-trip (e.g. panes list / create).
/// `request_id` was sent in the matching `roux-command` event; the frontend
/// answers by calling this command with the serialized data.
// No #[specta::specta] — serde_json::Value produces invalid TypeScript.
#[tauri::command]
pub(crate) fn submit_roux_reply(
    request_id: String,
    data: serde_json::Value,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let sender = {
        let mut map = state.pending_replies.lock().map_err(|e| e.to_string())?;
        map.remove(&request_id)
    };
    match sender {
        Some(tx) => tx.send(data).map_err(|_| {
            // Receiver dropped — the socket handler already timed out and
            // cleaned its end. The frontend reply arrived too late to matter.
            format!("reply for request_id {} arrived after timeout", request_id)
        }),
        None => Err(format!("no pending reply for request_id {}", request_id)),
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn resize_session(
    id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.pty_manager.resize(&id, cols, rows).map_err(|e| e.to_string())
}

// No #[specta::specta] — Channel<Response> doesn't implement specta::Type
#[tauri::command]
pub(crate) fn attach_pty_output(
    id: String,
    on_event: tauri::ipc::Channel<tauri::ipc::Response>,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.pty_manager.attach_output_channel(&id, on_event);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn spawn_shell(
    id: String,
    working_dir: String,
    session_id: Option<String>,
    pane_id: Option<String>,
    nono_profile: Option<String>,
    nono_allow_dirs: Option<Vec<String>>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::pty::{NonoConfig, SmolvmExec};
    let nono = nono_profile
        .map(|profile| NonoConfig { profile, allow_dirs: nono_allow_dirs.unwrap_or_default() });

    // Look up the session record so secondary panes inherit its
    // smol-machine binding. Promoted to async (was sync) so the lookup
    // can happen here rather than forcing the frontend to thread
    // smol_machine_name through every spawn call site. Lookup failures
    // (DB read error, IO error) propagate so we never silently bypass
    // VM isolation by treating an error as "no binding". `Ok(None)` —
    // the session genuinely doesn't exist yet, e.g. CLI bridge spawning
    // ahead of session creation — falls through with no binding.
    let session_record = match session_id.as_deref() {
        Some(sid) => state.runtime.session_handle.get(sid).await.map_err(|e| e.to_string())?,
        None => None,
    };
    let smol_name = session_record.as_ref().and_then(|s| s.smol_machine_name.clone());
    let smolvm = match smol_name {
        Some(name) if !name.trim().is_empty() => {
            let install =
                crate::services::smolvm::resolve_smolvm_binary().ok_or_else(|| {
                    format!(
                        "session is bound to smol machine '{name}', but smolvm is not installed; \
                         unbind via the panel or install smolvm to continue"
                    )
                })?;
            Some(SmolvmExec {
                binary: install.path,
                machine_name: name.trim().to_string(),
                guest_shell: "/bin/sh".to_string(),
            })
        }
        _ => None,
    };

    // Secondary pane spawn path. Primary session shells already carry
    // ROUX_PROJECT_ID / ROUX_WORKTREE_PATH via services::sessions. Secondary
    // panes could resolve the same from `session_record` above; left as
    // None until callers actually need them downstream.
    state
        .pty_manager
        .spawn_shell(
            &id,
            &working_dir,
            session_id.as_deref(),
            pane_id.as_deref(),
            None,
            None,
            None, // notes env snapshot — session-creation path only
            nono.as_ref(),
            smolvm.as_ref(),
            initial_size,
            crate::pty::PtyRole::Secondary,
            profile.as_deref(),
            app.clone(),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn spawn_task(
    id: String,
    command: String,
    working_dir: String,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::pty::SmolvmExec;

    // See spawn_shell above: project/worktree env is deferred for the
    // secondary-pane path until this command goes async.
    let context = crate::automation_hooks::HookContext {
        repo_path: Some(working_dir.clone()),
        worktree_path: Some(working_dir.clone()),
        task_id: Some(id.clone()),
        session_id: session_id.clone(),
        scope: session_id.as_ref().map(|_| "session".to_string()),
        cwd: Some(working_dir.clone()),
        ..crate::automation_hooks::HookContext::new(crate::automation_hooks::HookEvent::PreTaskRun)
    };
    state
        .automation_hooks
        .run_blocking(crate::automation_hooks::HookEvent::PreTaskRun, context)
        .await
        .map_err(|e| e.to_string())?;

    // Inherit the session's smol-machine binding so a `roux run`-style
    // task lands inside the same VM as the rest of the session. Mirrors
    // the spawn_shell flow above (via build_smolvm_exec_for_session
    // in services::sessions). See `commands/sessions.rs::spawn_shell`
    // and `socket.rs::handle_run` for the matching wraps.
    let session_record = match session_id.as_deref() {
        Some(sid) => state.runtime.session_handle.get(sid).await.map_err(|e| e.to_string())?,
        None => None,
    };
    let smol_name = session_record.as_ref().and_then(|s| s.smol_machine_name.clone());
    let smolvm = match smol_name {
        Some(name) if !name.trim().is_empty() => {
            let install =
                crate::services::smolvm::resolve_smolvm_binary().ok_or_else(|| {
                    format!(
                        "session is bound to smol machine '{name}', but smolvm is not installed; \
                         unbind via the panel or install smolvm to continue"
                    )
                })?;
            Some(SmolvmExec {
                binary: install.path,
                machine_name: name.trim().to_string(),
                guest_shell: "/bin/sh".to_string(),
            })
        }
        _ => None,
    };

    state
        .pty_manager
        .spawn_task(
            &id,
            &command,
            &working_dir,
            session_id.as_deref(),
            pane_id.as_deref(),
            None,
            None,
            None, // notes env snapshot — session-creation path only
            smolvm.as_ref(),
            initial_size,
            crate::pty::PtyRole::Secondary,
            profile.as_deref(),
            app.clone(),
        )
        .map_err(|e| e.to_string())?;
    let scope = session_id.as_ref().map(|_| "session".to_string());
    let context = crate::automation_hooks::HookContext {
        repo_path: Some(working_dir.clone()),
        worktree_path: Some(working_dir.clone()),
        task_id: Some(id),
        session_id,
        scope,
        cwd: Some(working_dir),
        ..crate::automation_hooks::HookContext::new(crate::automation_hooks::HookEvent::PostTaskRun)
    };
    state
        .automation_hooks
        .spawn_background(crate::automation_hooks::HookEvent::PostTaskRun, context);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn get_pty_generation(id: String, state: tauri::State<AppState>) -> Option<u64> {
    state.pty_manager.get_generation(&id)
}

/// Live cwd of a PTY-backed process, resolved from the OS (no shell hooks).
/// Used at pane-state save time so that reconnecting a session restores the
/// directory the shell is actually in (after `cd`s), not just the directory
/// it was spawned in.
#[tauri::command]
#[specta::specta]
pub(crate) fn get_pty_cwd(id: String, state: tauri::State<AppState>) -> Option<String> {
    state.pty_manager.get_cwd(&id)
}

/// Archive a session (soft-delete) and run the surrounding hook +
/// watch-cleanup work. Shared by the Tauri command and the CLI/socket
/// handler so both code paths get identical lifecycle behavior.
pub(crate) async fn archive_session_with_hooks(
    state: &AppState,
    id: &str,
) -> Result<(), String> {
    let session = state.runtime.session_handle.get(id).await.map_err(|e| e.to_string())?;
    if let Some(session) = session.as_ref() {
        let context = crate::automation_hooks::HookContext {
            repo_path: Some(session.repo_root.clone()),
            worktree_path: Some(session.worktree_path.clone()),
            branch: Some(session.branch.clone()),
            session_id: Some(session.id.clone()),
            project_id: session.project_id.clone(),
            scope: Some("session".into()),
            cwd: Some(session.worktree_path.clone()),
            ..crate::automation_hooks::HookContext::new(
                crate::automation_hooks::HookEvent::PreSessionClose,
            )
        };
        state
            .automation_hooks
            .run_blocking(crate::automation_hooks::HookEvent::PreSessionClose, context)
            .await
            .map_err(|e| e.to_string())?;
    }
    svc::kill_session(&state.pty_manager, &state.runtime.session_handle, id)
        .await
        .map_err(|e| e.to_string())?;
    // Stop session-scoped recurring watches (e.g. PR pollers) so they
    // don't outlive the archived session and keep firing forever.
    state.watch_manager.remove_watches_for_session(id).await;
    if let Some(session) = session {
        let context = crate::automation_hooks::HookContext {
            repo_path: Some(session.repo_root.clone()),
            worktree_path: Some(session.worktree_path.clone()),
            branch: Some(session.branch.clone()),
            session_id: Some(session.id.clone()),
            project_id: session.project_id.clone(),
            scope: Some("session".into()),
            cwd: Some(session.worktree_path.clone()),
            ..crate::automation_hooks::HookContext::new(
                crate::automation_hooks::HookEvent::PostSessionClose,
            )
        };
        state
            .automation_hooks
            .spawn_background(crate::automation_hooks::HookEvent::PostSessionClose, context);
    }
    Ok(())
}

/// Archive a session (soft-delete). The frontend command name is retained
/// for backward-compat, but the record is kept on disk and shown in the
/// sessions-history pane until the user permanently deletes it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn kill_session(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    archive_session_with_hooks(&state, &id).await
}

/// Bring an archived session back to the active list.
#[tauri::command]
#[specta::specta]
pub(crate) async fn restore_session(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    svc::restore_session(&state.runtime.session_handle, &id).await.map_err(|e| e.to_string())
}

/// Permanently delete a session record. Irreversible. Does not touch the
/// worktree — worktree handling is explicit via the History pane's
/// Clean worktree action.
#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_session_permanently(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    svc::delete_session_permanently(&state.pty_manager, &state.runtime.session_handle, &id)
        .await
        .map_err(|e| e.to_string())?;
    // Tear down any session-scoped watches that may still be polling
    // (no-op if the session was already archived and watches were
    // cleaned up at archive time).
    state.watch_manager.remove_watches_for_session(&id).await;
    Ok(())
}

/// Check whether an archived session's worktree path still exists on disk.
/// The frontend uses this to disable the Restore button when the worktree
/// has been removed since archival.
#[tauri::command]
#[specta::specta]
pub(crate) async fn session_worktree_exists(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let session = state.runtime.session_handle.get(&id).await.map_err(|e| e.to_string())?;
    Ok(session.map(|s| std::path::Path::new(&s.worktree_path).exists()).unwrap_or(false))
}

/// Kill only the PTY for `id`, leaving session state, pane-state files, and
/// the session record untouched. Used by `disposePane` on the frontend so
/// closing a pane never accidentally destroys its session — even when the
/// pane's `ptyId === sessionId` (the session-owned PTY spawned by
/// `create_session` / `create_session_shell`).
///
/// Prior to this command, `disposePane` called `kill_session`, which tore
/// down `session_handle` and `pane_state` as a side effect. That was fine
/// for non-primary shells whose ptyId was a random UUID (not in
/// `session_handle`) but catastrophic for primary panes, where
/// `ptyId == sessionId` matched a real session record and deleted it.
#[tauri::command]
#[specta::specta]
pub(crate) fn kill_pty(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.kill(&id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_name_override(
    session_id: String,
    name_override: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Some(client) = &state.daemon_client {
        return client.set_session_name_override(session_id, name_override).await;
    }
    state
        .runtime.session_handle
        .set_name_override(&session_id, name_override)
        .await
        .map_err(|e| e.to_string())
}

/// Pin (or clear) a PR for a session. The status bar uses this when set
/// instead of the branch-based discovery, so cross-repo PRs and renamed
/// branches still surface in the chip.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_pinned_pr_url(
    session_id: String,
    url: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let normalized = url.and_then(|u| {
        let t = u.trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    });
    state
        .runtime.session_handle
        .set_pinned_pr_url(&session_id, normalized)
        .await
        .map_err(|e| e.to_string())
}

/// Bind (or clear) a smol machine for a session. When set, every PTY
/// spawned for this session runs via `smolvm machine exec --name <n> ...`
/// inside the named VM instead of on the host. Pass `None` (or empty) to
/// unbind. The empty-string normalization happens in
/// `SessionHandle::set_smol_machine_name`'s service handler so the wire
/// shape stays simple.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_smol_machine(
    session_id: String,
    machine_name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .runtime.session_handle
        .set_smol_machine_name(&session_id, machine_name)
        .await
        .map_err(|e| e.to_string())
}

/// Re-read the session's worktree branch via `git rev-parse` and update the
/// stored value if it changed. Returns the current branch (whether or not it
/// changed). The frontend calls this on a low-frequency tick so PR discovery
/// re-runs after the user `git checkout`s inside a Roux pane.
#[tauri::command]
#[specta::specta]
pub(crate) async fn refresh_session_branch(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    let session = state
        .runtime.session_handle
        .get(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(session) = session else { return Ok(None) };
    if !session.is_git_repo {
        return Ok(Some(session.branch));
    }
    // `git branch --show-current` returns empty on detached HEAD; in that
    // case keep whatever the session already has rather than overwriting
    // with "" — the chip would otherwise vanish during a transient rebase.
    let Some(current) = svc::get_current_branch(&session.worktree_path) else {
        return Ok(Some(session.branch));
    };
    if current.is_empty() {
        return Ok(Some(session.branch));
    }
    let _ = state
        .runtime.session_handle
        .set_branch(&session_id, current.clone())
        .await
        .map_err(|e| e.to_string())?;
    Ok(Some(current))
}

/// Spawns a plain shell in the session's
/// primary PTY instead of the claude binary. The frontend attaches the
/// selected spawn profile and types setup / startup commands after the
/// shell is ready. Used for every non-claude profile in the new-session
/// picker (Codex, Plain shell, user profiles, inline Custom…).
#[tauri::command]
#[specta::specta]
pub(crate) async fn create_session_shell(
    repo_path: String,
    name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    opts: Option<CreateShellOpts>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    use crate::pty::NonoConfig;
    let opts = opts.unwrap_or_default();
    // Clone before await — the MutexGuard is not Send.
    let settings = state.settings.lock().unwrap().clone();
    let nono = opts.nono_profile.map(|profile| NonoConfig {
        profile,
        allow_dirs: opts.nono_allow_dirs.unwrap_or_default(),
    });
    let initial_size = opts.initial_size;

    let target = if let Some(ref wt_path) = worktree_path {
        svc::SessionTarget::ExistingWorktree { path: wt_path }
    } else if let Some(ref br) = branch {
        svc::SessionTarget::NewWorktree {
            branch: br,
            start_point: opts.base.as_deref(),
            fetch_first: opts.fetch_first.unwrap_or(false),
        }
    } else {
        svc::SessionTarget::Repo
    };

    let smol_machine_name = opts
        .smol_machine_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());

    svc::create_session_shell(
        &state.pty_manager,
        &state.runtime.session_handle,
        &state.runtime.project_handle,
        &settings,
        &repo_path,
        &name,
        target,
        nono.as_ref(),
        opts.profile.as_deref(),
        initial_size,
        opts.project_id.as_deref(),
        opts.blueprint_id.as_deref(),
        smol_machine_name.as_deref(),
        Some(&state.automation_hooks),
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Respawns a plain shell in the session's primary PTY. The frontend
/// replays the pane's spawn profile commands into the fresh shell after
/// this call returns, so agents come back up the same way they were
/// originally launched via `create_session_shell`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn reconnect_session_shell(
    id: String,
    nono_profile: Option<String>,
    nono_allow_dirs: Option<Vec<String>>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    use crate::pty::NonoConfig;
    let nono = nono_profile
        .map(|profile| NonoConfig { profile, allow_dirs: nono_allow_dirs.unwrap_or_default() });
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    svc::reconnect_session_shell(
        &state.pty_manager,
        &state.runtime.session_handle,
        &state.runtime.project_handle,
        &settings,
        &id,
        nono.as_ref(),
        profile.as_deref(),
        initial_size,
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Active sessions only — archived rows are excluded. The history view
/// uses `list_archived_sessions` for those.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, String> {
    if let Some(client) = &state.daemon_client {
        return client
            .list_sessions()
            .await
            .map(|all| all.into_iter().filter(|s| !s.archived).collect());
    }
    state
        .runtime.session_handle
        .list()
        .await
        .map(|all| all.into_iter().filter(|s| !s.archived).collect())
        .map_err(|e| e.to_string())
}

/// Archived sessions, sorted newest-first by `ended_at` so the history
/// pane renders in the order the user closed them.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_archived_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, String> {
    if let Some(client) = &state.daemon_client {
        let mut archived: Vec<Session> = client
            .list_sessions()
            .await?
            .into_iter()
            .filter(|s| s.archived)
            .collect();
        archived.sort_by_key(|s| std::cmp::Reverse(s.ended_at.unwrap_or(0)));
        return Ok(archived);
    }
    state
        .runtime.session_handle
        .list()
        .await
        .map(|all| {
            let mut archived: Vec<Session> = all.into_iter().filter(|s| s.archived).collect();
            archived.sort_by_key(|s| std::cmp::Reverse(s.ended_at.unwrap_or(0)));
            archived
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn refresh_session_git_status(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    svc::refresh_git_status(&state.runtime.session_handle, &id).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_is_git_repo(path: String) -> bool {
    svc::is_git_repo(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn list_git_repos_in_roots(
    roots: Vec<String>,
    exclude_worktrees: bool,
) -> Vec<String> {
    svc::list_git_repos_in_roots(&roots, exclude_worktrees)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn list_claude_sessions(cwd: String) -> Result<Vec<svc::ClaudeSession>, String> {
    svc::list_claude_sessions(&cwd).map_err(|e| e.to_string())
}

/// Return the built-in spawn profile registry, assembled from each provider
/// module plus the catch-all "Plain shell". Called once at frontend startup
/// to populate the built-in segment of the pane-picker registry. Safe to
/// call again any time — the result is derived from current settings and
/// has no side effects.
#[tauri::command]
#[specta::specta]
pub(crate) fn get_builtin_profiles(state: tauri::State<AppState>) -> Vec<roux_core::SpawnProfile> {
    let settings = state.settings.lock().unwrap().clone();
    crate::providers::builtin_profiles(&settings)
}
