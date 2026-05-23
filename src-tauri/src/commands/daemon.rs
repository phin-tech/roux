use std::path::PathBuf;

use crate::daemon_client::DaemonStatus;
use crate::state::{required_daemon_client_ref, AppState};
use roux_runtime::process_service::{ProcessRecord, ProcessSnapshot};
use roux_sdk::{PtyRecord, PtySnapshot};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeMode {
    Daemon,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeCounts {
    pub(crate) session_count: usize,
    pub(crate) project_count: usize,
    pub(crate) watch_count: usize,
    pub(crate) process_count: usize,
    pub(crate) pty_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeStatus {
    pub(crate) mode: RuntimeMode,
    pub(crate) desktop_pid: u32,
    pub(crate) started_at_ms: u64,
    pub(crate) uptime_ms: u64,
    pub(crate) daemon: Option<DaemonStatus>,
    pub(crate) local: Option<RuntimeCounts>,
    pub(crate) status_error: Option<String>,
}

#[tauri::command]
pub(crate) fn get_daemon_status(state: tauri::State<'_, AppState>) -> Option<DaemonStatus> {
    state.daemon_client.as_ref().map(|client| client.status().clone())
}

#[tauri::command]
pub(crate) async fn get_runtime_status(
    state: tauri::State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let client = required_daemon_client_ref(&state)?;
    let (daemon, status_error) = match client.refresh_status().await {
        Ok(status) => (status, None),
        Err(err) => (client.status().clone(), Some(err)),
    };
    Ok(RuntimeStatus {
        mode: RuntimeMode::Daemon,
        desktop_pid: std::process::id(),
        started_at_ms: daemon.started_at_ms,
        uptime_ms: daemon.uptime_ms,
        daemon: Some(daemon),
        local: None,
        status_error,
    })
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
    let client = required_daemon_client_ref(&state)?;
    client
        .spawn_daemon_pty_shell(
            id,
            working_dir,
            session_id,
            pane_id,
            profile,
            None,
            Vec::new(),
            initial_size,
        )
        .await
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
    let client = required_daemon_client_ref(&state)?;
    client
        .spawn_daemon_pty_task(command, id, working_dir, session_id, pane_id, profile, initial_size)
        .await
}

#[tauri::command]
pub(crate) async fn daemon_pty_output(
    id: String,
    max_bytes: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<PtySnapshot, String> {
    let client = required_daemon_client_ref(&state)?;
    client.daemon_pty_output(id, max_bytes).await
}

#[tauri::command]
pub(crate) async fn daemon_pty_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PtyRecord>, String> {
    let client = required_daemon_client_ref(&state)?;
    client.list_daemon_ptys().await
}

#[tauri::command]
pub(crate) async fn daemon_pty_write(
    id: String,
    data: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let client = required_daemon_client_ref(&state)?;
    client.write_daemon_pty(id, data).await
}

#[tauri::command]
pub(crate) async fn daemon_pty_resize(
    id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<PtyRecord, String> {
    let client = required_daemon_client_ref(&state)?;
    client.resize_daemon_pty(id, cols, rows).await
}

#[tauri::command]
pub(crate) async fn daemon_pty_kill(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<PtyRecord, String> {
    let client = required_daemon_client_ref(&state)?;
    client.kill_daemon_pty(id).await
}
