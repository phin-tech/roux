//! Tauri commands for per-session pane state persistence. Thin wrappers around
//! [`crate::pane_state`] — all real logic lives there.

// No #[specta::specta] on these — serde_json::Value produces invalid TypeScript
// in specta's generated output. The frontend calls these via raw invoke() anyway.

use crate::state::AppState;
// `rlog!` is `#[macro_export]`-ed by `crate::logging`, so it's reachable as
// `crate::rlog!(...)` without an explicit `use`.

fn join_err(e: tauri::Error) -> String {
    format!("task join: {e}")
}

#[tauri::command]
pub(crate) async fn load_pane_state(session_id: String) -> Option<serde_json::Value> {
    // The loader walks the status directory for provider-session enrichment;
    // off-main-thread because hundreds of files at startup add up.
    let id_for_log = session_id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::pane_state::load_pane_state(&session_id)
    })
    .await;
    match result {
        Ok(value) => value,
        Err(e) => {
            // Don't let a panic in the blocking task look like "no saved
            // state" — that would silently trigger reset behavior.
            crate::rlog!("load_pane_state: blocking task failed for {id_for_log:?}: {e}");
            None
        }
    }
}

#[tauri::command]
pub(crate) async fn save_pane_state(
    session_id: String,
    data: serde_json::Value,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::pane_state::save_pane_state(&session_id, data)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub(crate) async fn save_live_pane_state(
    session_id: String,
    schema_version: u32,
    layout: serde_json::Value,
    pane_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let records =
        state.runtime.pane_handle.list_by_ids(pane_ids).await.map_err(|e| e.to_string())?;

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

    tauri::async_runtime::spawn_blocking(move || {
        crate::pane_state::save_live_pane_state(&session_id, schema_version, layout, descriptors)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub(crate) async fn delete_pane_state(session_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::pane_state::delete_pane_state(&session_id))
        .await
        .map_err(join_err)?
}
