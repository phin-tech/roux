use crate::services::sessions as svc;
use crate::session::Session;
use crate::state::AppState;

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
pub(crate) fn spawn_shell(
    id: String,
    working_dir: String,
    session_id: Option<String>,
    pane_id: Option<String>,
    nono_profile: Option<String>,
    nono_allow_dirs: Option<Vec<String>>,
    initial_cols: Option<u16>,
    initial_rows: Option<u16>,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::pty::NonoConfig;
    let nono = nono_profile
        .map(|profile| NonoConfig { profile, allow_dirs: nono_allow_dirs.unwrap_or_default() });
    let initial_size = initial_cols.zip(initial_rows);
    // Secondary pane spawn path. Primary session shells already carry
    // ROUX_PROJECT_ID / ROUX_WORKTREE_PATH via services::sessions. Secondary
    // panes could resolve the same from session_handle, but SessionHandle::get
    // is async and this command is sync; leaving as None until either the
    // frontend passes them or this command is promoted to async.
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
            initial_size,
            app.clone(),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn spawn_task(
    id: String,
    command: String,
    working_dir: String,
    session_id: Option<String>,
    pane_id: Option<String>,
    initial_cols: Option<u16>,
    initial_rows: Option<u16>,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let initial_size = initial_cols.zip(initial_rows);
    // See spawn_shell above: project/worktree env is deferred for the
    // secondary-pane path until this command goes async.
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
            initial_size,
            app.clone(),
        )
        .map_err(|e| e.to_string())
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

#[tauri::command]
#[specta::specta]
pub(crate) async fn kill_session(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    svc::kill_session(&state.pty_manager, &state.session_handle, &id)
        .await
        .map_err(|e| e.to_string())
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
    state
        .session_handle
        .set_name_override(&session_id, name_override)
        .await
        .map_err(|e| e.to_string())
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
    nono_profile: Option<String>,
    nono_allow_dirs: Option<Vec<String>>,
    initial_cols: Option<u16>,
    initial_rows: Option<u16>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    use crate::pty::NonoConfig;
    // Clone before await — the MutexGuard is not Send.
    let settings = state.settings.lock().unwrap().clone();
    let nono = nono_profile
        .map(|profile| NonoConfig { profile, allow_dirs: nono_allow_dirs.unwrap_or_default() });
    let initial_size = initial_cols.zip(initial_rows);

    let target = if let Some(ref wt_path) = worktree_path {
        svc::SessionTarget::ExistingWorktree { path: wt_path }
    } else if let Some(ref br) = branch {
        svc::SessionTarget::NewWorktree { branch: br }
    } else {
        svc::SessionTarget::Repo
    };

    svc::create_session_shell(
        &state.pty_manager,
        &state.session_handle,
        &settings,
        &repo_path,
        &name,
        target,
        nono.as_ref(),
        initial_size,
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
    initial_cols: Option<u16>,
    initial_rows: Option<u16>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    use crate::pty::NonoConfig;
    let nono = nono_profile
        .map(|profile| NonoConfig { profile, allow_dirs: nono_allow_dirs.unwrap_or_default() });
    let initial_size = initial_cols.zip(initial_rows);
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    svc::reconnect_session_shell(
        &state.pty_manager,
        &state.session_handle,
        &state.project_handle,
        &settings,
        &id,
        nono.as_ref(),
        initial_size,
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, String> {
    state.session_handle.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn refresh_session_git_status(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    svc::refresh_git_status(&state.session_handle, &id).await.map_err(|e| e.to_string())
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
