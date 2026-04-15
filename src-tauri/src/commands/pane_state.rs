//! Tauri commands for per-session pane state persistence. Thin wrappers around
//! [`crate::pane_state`] — all real logic lives there.

// No #[specta::specta] on these — serde_json::Value produces invalid TypeScript
// in specta's generated output. The frontend calls these via raw invoke() anyway.

use crate::state::AppState;

#[tauri::command]
pub(crate) fn load_pane_state(session_id: String) -> Option<serde_json::Value> {
    crate::pane_state::load_pane_state(&session_id)
}

#[tauri::command]
pub(crate) fn save_pane_state(session_id: String, data: serde_json::Value) -> Result<(), String> {
    crate::pane_state::save_pane_state(&session_id, data)
}

#[tauri::command]
pub(crate) async fn save_live_pane_state(
    session_id: String,
    schema_version: u32,
    layout: serde_json::Value,
    pane_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let records = state
        .pane_handle
        .list_by_ids(pane_ids)
        .await
        .map_err(|e| e.to_string())?;

    let descriptors = records
        .into_iter()
        .map(|record| {
            let working_dir = if record.pane_type == "shell" {
                state.pty_manager.get_cwd(&record.pty_id)
            } else {
                None
            };
            record.descriptor_with_working_dir(working_dir)
        })
        .collect();

    crate::pane_state::save_live_pane_state(&session_id, schema_version, layout, descriptors)
}

#[tauri::command]
pub(crate) fn delete_pane_state(session_id: String) -> Result<(), String> {
    crate::pane_state::delete_pane_state(&session_id)
}
