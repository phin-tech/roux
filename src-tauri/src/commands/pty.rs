use crate::pty::PtyInfo;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_session_ptys(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PtyInfo>, String> {
    let mut ptys = Vec::new();
    if let Some(client) = state.daemon_client.clone() {
        ptys.extend(
            client
                .list_daemon_ptys()
                .await?
                .into_iter()
                .map(|record| record.info)
                .filter(|info| info.session_id.as_deref() == Some(session_id.as_str())),
        );
    }
    ptys.extend(state.pty_manager.list_for_session(&session_id));
    Ok(ptys)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_all_ptys(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PtyInfo>, String> {
    let mut ptys = Vec::new();
    if let Some(client) = state.daemon_client.clone() {
        ptys.extend(client.list_daemon_ptys().await?.into_iter().map(|record| record.info));
    }
    ptys.extend(state.pty_manager.list_all());
    Ok(ptys)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn detach_pty(
    pty_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Some(client) = state.daemon_client.clone() {
        match client.detach_daemon_pty(pty_id.clone()).await {
            Ok(_) => return Ok(()),
            Err(err) if is_daemon_pty_not_found(&err) => {}
            Err(err) => return Err(err),
        }
    }
    state.pty_manager.detach(&pty_id);
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
    if let Some(client) = state.daemon_client.clone() {
        match client.daemon_pty_output(pty_id.clone(), Some(256 * 1024)).await {
            Ok(snapshot) => {
                let _ = client.attach_daemon_pty_to_pane(pty_id.clone(), pane_id).await?;
                let _ = client.resize_daemon_pty(pty_id, cols, rows).await?;
                return Ok(AttachResult { replay_bytes: snapshot.output_bytes });
            }
            Err(err) if is_daemon_pty_not_found(&err) => {}
            Err(err) => return Err(err),
        }
    }

    let replay_bytes = state.pty_manager.get_replay(&pty_id, 256 * 1024);
    state.pty_manager.attach_to_pane(&pty_id, &pane_id);
    let _ = state.pty_manager.resize(&pty_id, cols, rows);
    Ok(AttachResult { replay_bytes })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn mark_pty_read(
    pty_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Some(client) = state.daemon_client.clone() {
        match client.mark_daemon_pty_read(pty_id.clone()).await {
            Ok(_) => return Ok(()),
            Err(err) if is_daemon_pty_not_found(&err) => {}
            Err(err) => return Err(err),
        }
    }
    state.pty_manager.mark_read(&pty_id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_pty_name(
    pty_id: String,
    name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Some(client) = state.daemon_client.clone() {
        match client.set_daemon_pty_name(pty_id.clone(), name.clone()).await {
            Ok(_) => return Ok(()),
            Err(err) if is_daemon_pty_not_found(&err) => {}
            Err(err) => return Err(err),
        }
    }
    state.pty_manager.set_name(&pty_id, name.as_deref());
    Ok(())
}

fn is_daemon_pty_not_found(err: &str) -> bool {
    err.contains("daemon pty not found")
}
