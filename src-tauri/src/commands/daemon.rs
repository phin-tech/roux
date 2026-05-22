use std::path::PathBuf;

use crate::daemon_client::DaemonStatus;
use crate::state::AppState;
use roux_runtime::process_service::{ProcessRecord, ProcessSnapshot};
use roux_runtime::pty_service::{PtyRecord, PtySnapshot, PtySpawnRequest};

#[tauri::command]
pub(crate) fn get_daemon_status(state: tauri::State<'_, AppState>) -> Option<DaemonStatus> {
    state.daemon_client.as_ref().map(|client| client.status().clone())
}

#[tauri::command]
pub(crate) async fn daemon_process_start(
    command: String,
    working_dir: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<ProcessRecord, String> {
    if let Some(client) = &state.daemon_client {
        return client.start_daemon_process(command, working_dir).await;
    }

    state
        .runtime
        .process_handle
        .start(command, working_dir.map(PathBuf::from))
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn daemon_process_output(
    id: String,
    max_bytes: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<ProcessSnapshot, String> {
    if let Some(client) = &state.daemon_client {
        return client.daemon_process_output(id, max_bytes).await;
    }

    state
        .runtime
        .process_handle
        .snapshot(
            &id,
            max_bytes.unwrap_or(roux_runtime::process_service::PROCESS_OUTPUT_DEFAULT_POLL_BYTES),
        )
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "daemon process not found".to_string())
}

#[tauri::command]
pub(crate) async fn daemon_process_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProcessRecord>, String> {
    if let Some(client) = &state.daemon_client {
        return client.list_daemon_processes().await;
    }

    state.runtime.process_handle.list().await.map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn daemon_process_kill(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ProcessRecord, String> {
    if let Some(client) = &state.daemon_client {
        return client.kill_daemon_process(id).await;
    }

    state
        .runtime
        .process_handle
        .kill(&id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "daemon process not found".to_string())
}

#[tauri::command]
pub(crate) async fn daemon_pty_spawn_shell(
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
    state: tauri::State<'_, AppState>,
) -> Result<PtyRecord, String> {
    if let Some(client) = &state.daemon_client {
        return client
            .spawn_daemon_pty_shell(id, working_dir, session_id, pane_id, profile, initial_size)
            .await;
    }

    state
        .runtime
        .pty_handle
        .spawn_shell(pty_spawn_request(id, working_dir, session_id, pane_id, profile, initial_size))
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn daemon_pty_spawn_task(
    command: String,
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
    state: tauri::State<'_, AppState>,
) -> Result<PtyRecord, String> {
    if let Some(client) = &state.daemon_client {
        return client
            .spawn_daemon_pty_task(
                command,
                id,
                working_dir,
                session_id,
                pane_id,
                profile,
                initial_size,
            )
            .await;
    }

    state
        .runtime
        .pty_handle
        .spawn_task(
            command,
            pty_spawn_request(id, working_dir, session_id, pane_id, profile, initial_size),
        )
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn daemon_pty_output(
    id: String,
    max_bytes: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<PtySnapshot, String> {
    if let Some(client) = &state.daemon_client {
        return client.daemon_pty_output(id, max_bytes).await;
    }

    state
        .runtime
        .pty_handle
        .snapshot(
            &id,
            max_bytes.unwrap_or(roux_runtime::pty_service::PTY_OUTPUT_DEFAULT_POLL_BYTES),
        )
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "daemon pty not found".to_string())
}

#[tauri::command]
pub(crate) async fn daemon_pty_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PtyRecord>, String> {
    if let Some(client) = &state.daemon_client {
        return client.list_daemon_ptys().await;
    }

    state.runtime.pty_handle.list().await.map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn daemon_pty_write(
    id: String,
    data: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Some(client) = &state.daemon_client {
        return client.write_daemon_pty(id, data).await;
    }

    state.runtime.pty_handle.write(&id, data.into_bytes()).await.map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn daemon_pty_resize(
    id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<PtyRecord, String> {
    if let Some(client) = &state.daemon_client {
        return client.resize_daemon_pty(id, cols, rows).await;
    }

    state
        .runtime
        .pty_handle
        .resize(&id, cols, rows)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "daemon pty not found".to_string())
}

#[tauri::command]
pub(crate) async fn daemon_pty_kill(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<PtyRecord, String> {
    if let Some(client) = &state.daemon_client {
        return client.kill_daemon_pty(id).await;
    }

    state
        .runtime
        .pty_handle
        .kill(&id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "daemon pty not found".to_string())
}

fn pty_spawn_request(
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> PtySpawnRequest {
    PtySpawnRequest {
        id,
        working_dir: working_dir.map(PathBuf::from),
        session_id,
        pane_id,
        profile,
        initial_size,
        ..PtySpawnRequest::default()
    }
}
