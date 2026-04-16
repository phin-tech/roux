use crate::pane_service::PaneRecord;
use crate::state::AppState;

#[tauri::command]
pub(crate) async fn upsert_pane_record(
    record: PaneRecord,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.pane_handle.upsert(record).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn remove_pane_record(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.pane_handle.remove(&id).await.map_err(|e| e.to_string())
}
