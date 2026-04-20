use crate::pty::PtyInfo;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub(crate) fn list_session_ptys(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PtyInfo>, String> {
    let ptys = state.pty_manager.list_for_session(&session_id);
    rlog!("list_session_ptys({}): found {} PTYs", session_id, ptys.len());
    for pty in &ptys {
        rlog!("  - {} status={:?} session_id={:?}", pty.id, pty.status, pty.session_id);
    }
    Ok(ptys)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn detach_pty(
    pty_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.pty_manager.detach(&pty_id);
    Ok(())
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct AttachResult {
    pub replay_bytes: Vec<u8>,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn attach_pty_to_pane(
    pty_id: String,
    pane_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<AttachResult, String> {
    let replay_bytes = state.pty_manager.get_replay(&pty_id, 256 * 1024);
    state.pty_manager.attach_to_pane(&pty_id, &pane_id);
    let _ = state.pty_manager.resize(&pty_id, cols, rows);
    Ok(AttachResult { replay_bytes })
}

#[tauri::command]
#[specta::specta]
pub(crate) fn mark_pty_read(
    pty_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.pty_manager.mark_read(&pty_id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_pty_name(
    pty_id: String,
    name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.pty_manager.set_name(&pty_id, name.as_deref());
    Ok(())
}
