use crate::pty::PtyInfo;
use crate::state::{required_daemon_client, AppState};

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_session_ptys(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PtyInfo>, String> {
    let client = required_daemon_client(&state)?;
    Ok(client
        .list_daemon_ptys()
        .await?
        .into_iter()
        .map(|record| record.info)
        .filter(|info| info.session_id.as_deref() == Some(session_id.as_str()))
        .collect())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_all_ptys(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PtyInfo>, String> {
    let client = required_daemon_client(&state)?;
    Ok(client.list_daemon_ptys().await?.into_iter().map(|record| record.info).collect())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn detach_pty(
    pty_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let client = required_daemon_client(&state)?;
    client.detach_daemon_pty(pty_id.clone()).await?;
    super::sessions::abort_daemon_attach_task(&state, &pty_id)?;
    Ok(())
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct AttachResult {
    pub replay_bytes: Vec<u8>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn attach_pty_to_pane(
    pty_id: String,
    pane_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<AttachResult, String> {
    let client = required_daemon_client(&state)?;
    let snapshot = client.daemon_pty_output(pty_id.clone(), Some(256 * 1024)).await?;
    let _ = client.attach_daemon_pty_to_pane(pty_id.clone(), pane_id).await?;
    let _ = client.resize_daemon_pty(pty_id, cols, rows).await?;
    let replay_bytes = snapshot.output_bytes;
    Ok(AttachResult { replay_bytes })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn mark_pty_read(
    pty_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let client = required_daemon_client(&state)?;
    let _ = client.mark_daemon_pty_read(pty_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_pty_name(
    pty_id: String,
    name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let client = required_daemon_client(&state)?;
    let _ = client.set_daemon_pty_name(pty_id, name).await?;
    Ok(())
}
