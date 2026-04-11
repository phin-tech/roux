use crate::session::Session;
use crate::services::sessions as svc;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub(crate) fn write_to_session(id: String, data: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.write(&id, data.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn resize_session(id: String, cols: u16, rows: u16, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.resize(&id, cols, rows).map_err(|e| e.to_string())
}

// No #[specta::specta] — Channel<Response> doesn't implement specta::Type
#[tauri::command]
pub(crate) fn attach_pty_output(id: String, on_event: tauri::ipc::Channel<tauri::ipc::Response>, state: tauri::State<AppState>) -> Result<(), String> {
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
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state
        .pty_manager
        .spawn_shell(&id, &working_dir, session_id.as_deref(), pane_id.as_deref(), app.clone())
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
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state
        .pty_manager
        .spawn_task(&id, &command, &working_dir, session_id.as_deref(), pane_id.as_deref(), app.clone())
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
pub(crate) async fn kill_session(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
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
pub(crate) async fn create_session(
    repo_path: String,
    name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    extra_flags: Option<Vec<String>>,
    nono_profile: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let flags = extra_flags.unwrap_or_default();

    let target = if let Some(ref wt_path) = worktree_path {
        svc::SessionTarget::ExistingWorktree { path: wt_path }
    } else if let Some(ref br) = branch {
        svc::SessionTarget::NewWorktree { branch: br }
    } else {
        svc::SessionTarget::Repo
    };

    svc::create_session(
        &state.pty_manager,
        &state.session_handle,
        &settings,
        &repo_path,
        &name,
        target,
        &flags,
        nono_profile.as_deref(),
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Parallel to `create_session`, but spawns a plain shell in the session's
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
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    // Clone before await — the MutexGuard is not Send.
    let settings = state.settings.lock().unwrap().clone();

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
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn reconnect_session(
    id: String,
    extra_flags: Option<Vec<String>>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let flags = extra_flags.unwrap_or_default();
    svc::reconnect_session(
        &state.pty_manager,
        &state.session_handle,
        &settings,
        &id,
        &flags,
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_sessions(state: tauri::State<'_, AppState>) -> Result<Vec<Session>, String> {
    state.session_handle.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn refresh_session_git_status(id: String, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    svc::refresh_git_status(&state.session_handle, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_is_git_repo(path: String) -> bool {
    svc::is_git_repo(&path)
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
