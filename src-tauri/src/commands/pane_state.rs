//! Tauri commands for per-session pane state persistence. Thin wrappers around
//! [`crate::pane_state`] — all real logic lives there.

#[tauri::command]
#[specta::specta]
pub(crate) fn load_pane_state(session_id: String) -> Option<serde_json::Value> {
    crate::pane_state::load_pane_state(&session_id)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn save_pane_state(
    session_id: String,
    data: serde_json::Value,
) -> Result<(), String> {
    crate::pane_state::save_pane_state(&session_id, data)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn delete_pane_state(session_id: String) -> Result<(), String> {
    crate::pane_state::delete_pane_state(&session_id)
}
