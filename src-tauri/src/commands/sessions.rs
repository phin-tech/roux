use crate::services::sessions as svc;
use crate::session::Session;
use crate::state::{
    required_daemon_client, required_daemon_client_ref, AppState, DaemonPtyAttachTask,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Manager;

static NEXT_DAEMON_ATTACH_TASK_TOKEN: AtomicU64 = AtomicU64::new(1);

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

pub(crate) fn abort_daemon_attach_task(state: &AppState, id: &str) -> Result<(), String> {
    let previous = state.daemon_pty_attach_tasks.lock().map_err(|err| err.to_string())?.remove(id);
    if let Some(previous) = previous {
        previous.handle.abort();
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn write_to_session(
    id: String,
    data: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let client = required_daemon_client(&state)?;
    client.write_daemon_pty(id, data).await
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
pub(crate) async fn resize_session(
    id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let client = required_daemon_client(&state)?;
    let _ = client.resize_daemon_pty(id, cols, rows).await?;
    Ok(())
}

// No #[specta::specta] — Channel<Response> doesn't implement specta::Type
#[tauri::command]
pub(crate) async fn attach_pty_output(
    id: String,
    on_event: tauri::ipc::Channel<tauri::ipc::Response>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let client = required_daemon_client(&state)?;
    let _ = client.daemon_pty_output(id.clone(), Some(0)).await?;
    let token = NEXT_DAEMON_ATTACH_TASK_TOKEN.fetch_add(1, Ordering::Relaxed);
    let bridge = client.spawn_daemon_pty_output_bridge(id.clone(), on_event, app.clone());
    let cleanup_id = id.clone();
    let cleanup_app = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let _ = bridge.await;
        if let Some(state) = cleanup_app.try_state::<AppState>() {
            if let Ok(mut tasks) = state.daemon_pty_attach_tasks.lock() {
                let should_remove =
                    tasks.get(&cleanup_id).map(|task| task.token == token).unwrap_or(false);
                if should_remove {
                    tasks.remove(&cleanup_id);
                }
            }
        }
    });
    let previous = state
        .daemon_pty_attach_tasks
        .lock()
        .map_err(|err| err.to_string())?
        .insert(id, DaemonPtyAttachTask { token, handle });
    if let Some(previous) = previous {
        previous.handle.abort();
    }
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
    _app: tauri::AppHandle,
) -> Result<(), String> {
    let daemon_nono_profile = nono_profile.clone();
    let daemon_nono_allow_dirs = nono_allow_dirs.clone().unwrap_or_default();

    let client = required_daemon_client(&state)?;
    client
        .spawn_daemon_pty_shell(
            Some(id),
            Some(working_dir),
            session_id,
            pane_id,
            profile,
            daemon_nono_profile,
            daemon_nono_allow_dirs,
            initial_size,
        )
        .await?;
    Ok(())
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
    _app: tauri::AppHandle,
) -> Result<(), String> {
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

    let client = required_daemon_client(&state)?;
    client
        .spawn_daemon_pty_task(
            command.clone(),
            Some(id.clone()),
            Some(working_dir.clone()),
            session_id.clone(),
            pane_id,
            profile,
            initial_size,
        )
        .await?;
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
pub(crate) async fn archive_session_with_hooks(state: &AppState, id: &str) -> Result<(), String> {
    let client = required_daemon_client_ref(state)?;
    let session = client.get_session(id.to_string()).await?;
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
    let _ = client.archive_session(id.to_string()).await?;
    // Stop session-scoped recurring watches (e.g. PR pollers) so they
    // don't outlive the archived session and keep firing forever.
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("watch-remove-for-session"))
    {
        if let Err(err) = client.remove_watches_for_session(id.to_string()).await {
            rlog!("archive_session: daemon watch cleanup failed for {id}: {err}");
        }
    }
    state.watch_manager.remove_watches_for_session(id).await;
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
    let client = required_daemon_client_ref(&state)?;
    let _ = client.restore_session(id).await?;
    Ok(())
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
    let client = required_daemon_client_ref(&state)?;
    client.delete_session(id.clone()).await?;
    if let Err(e) = crate::pane_state::delete_pane_state(&id) {
        rlog!("delete_session_permanently: failed to delete pane state for {id}: {e}");
    }
    // Tear down any session-scoped watches that may still be polling
    // (no-op if the session was already archived and watches were
    // cleaned up at archive time).
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("watch-remove-for-session"))
    {
        if let Err(err) = client.remove_watches_for_session(id.clone()).await {
            rlog!("delete_session_permanently: daemon watch cleanup failed for {id}: {err}");
        }
    }
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
    let client = required_daemon_client_ref(&state)?;
    client.session_worktree_exists(id).await
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
pub(crate) async fn kill_pty(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let client = required_daemon_client(&state)?;
    let _ = client.kill_daemon_pty(id.clone()).await?;
    abort_daemon_attach_task(&state, &id)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_name_override(
    session_id: String,
    name_override: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let client = required_daemon_client_ref(&state)?;
    client.set_session_name_override(session_id, name_override).await
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
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });
    let client = required_daemon_client_ref(&state)?;
    client.set_session_pinned_pr_url(session_id, normalized).await
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
    let client = required_daemon_client_ref(&state)?;
    client.set_session_smol_machine(session_id, machine_name).await
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
    let client = required_daemon_client_ref(&state)?;
    client.refresh_session_branch(session_id).await
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
    _app: tauri::AppHandle,
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

    let smol_machine_name =
        opts.smol_machine_name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());

    let client = required_daemon_client(&state)?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let branch_for_notes = match &target {
        svc::SessionTarget::Repo => {
            svc::get_current_branch(&repo_path).unwrap_or_else(|| "main".to_string())
        }
        svc::SessionTarget::ExistingWorktree { path } => {
            svc::get_current_branch(path).unwrap_or_else(|| "main".to_string())
        }
        svc::SessionTarget::NewWorktree { branch, .. } => branch.to_string(),
    };
    let project_record = match opts.project_id.as_deref() {
        Some(pid) => client
            .list_projects()
            .await
            .ok()
            .and_then(|projects| projects.into_iter().find(|project| project.id == pid)),
        None => None,
    };
    let notes = svc::build_notes_env_for_new_session(
        &settings,
        &session_id,
        &branch_for_notes,
        &repo_path,
        project_record.as_ref(),
    );
    let (daemon_worktree_path, daemon_branch, daemon_base, daemon_fetch_first) = match &target {
        svc::SessionTarget::Repo => (None, None, None, false),
        svc::SessionTarget::ExistingWorktree { path } => {
            (Some(path.to_string()), None, None, false)
        }
        svc::SessionTarget::NewWorktree { branch, start_point, fetch_first } => {
            (None, Some(branch.to_string()), start_point.map(str::to_string), *fetch_first)
        }
    };

    client
        .create_session_shell(crate::daemon_client::DaemonCreateSessionShellRequest {
            id: session_id,
            repo_path,
            name,
            worktree_path: daemon_worktree_path,
            branch: daemon_branch,
            base: daemon_base,
            fetch_first: daemon_fetch_first,
            profile: opts.profile,
            nono_profile: nono.as_ref().map(|config| config.profile.clone()),
            nono_allow_dirs: nono
                .as_ref()
                .map(|config| config.allow_dirs.clone())
                .unwrap_or_default(),
            initial_size,
            project_id: opts.project_id,
            blueprint_id: opts.blueprint_id,
            smol_machine_name,
            notes: Some(notes),
        })
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
    _app: tauri::AppHandle,
) -> Result<Session, String> {
    use crate::pty::NonoConfig;
    let nono = nono_profile
        .map(|profile| NonoConfig { profile, allow_dirs: nono_allow_dirs.unwrap_or_default() });
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let client = required_daemon_client(&state)?;
    let session = client.get_session(id.clone()).await?;
    let project_record = match session.project_id.as_deref() {
        Some(pid) => client
            .list_projects()
            .await
            .ok()
            .and_then(|projects| projects.into_iter().find(|project| project.id == pid)),
        None => None,
    };
    let notes = svc::build_notes_env_for_new_session(
        &settings,
        &session.id,
        &session.branch,
        &session.repo_root,
        project_record.as_ref(),
    );
    client
        .reconnect_session_shell(crate::daemon_client::DaemonReconnectSessionShellRequest {
            id,
            profile,
            nono_profile: nono.as_ref().map(|config| config.profile.clone()),
            nono_allow_dirs: nono
                .as_ref()
                .map(|config| config.allow_dirs.clone())
                .unwrap_or_default(),
            initial_size,
            notes: Some(notes),
        })
        .await
}

/// Active sessions only — archived rows are excluded. The history view
/// uses `list_archived_sessions` for those.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, String> {
    let client = required_daemon_client_ref(&state)?;
    client.list_sessions().await.map(|all| all.into_iter().filter(|s| !s.archived).collect())
}

/// Archived sessions, sorted newest-first by `ended_at` so the history
/// pane renders in the order the user closed them.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_archived_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, String> {
    let client = required_daemon_client_ref(&state)?;
    let mut archived: Vec<Session> =
        client.list_sessions().await?.into_iter().filter(|s| s.archived).collect();
    archived.sort_by_key(|s| std::cmp::Reverse(s.ended_at.unwrap_or(0)));
    Ok(archived)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn refresh_session_git_status(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let client = required_daemon_client_ref(&state)?;
    let _ = client.refresh_session_branch(id.clone()).await?;
    let session = client.get_session(id).await?;
    Ok(session.is_git_repo)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_is_git_repo(path: String) -> bool {
    svc::is_git_repo(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn list_git_repos_in_roots(roots: Vec<String>, exclude_worktrees: bool) -> Vec<String> {
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
