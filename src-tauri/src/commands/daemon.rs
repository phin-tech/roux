use std::path::PathBuf;

use crate::daemon_client::DaemonStatus;
use crate::state::AppState;
use roux_runtime::process_service::{ProcessRecord, ProcessSnapshot};

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
