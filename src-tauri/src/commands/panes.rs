use crate::pane_service::PaneRecord;
use crate::state::AppState;

#[tauri::command]
pub(crate) async fn upsert_pane_record(
    record: PaneRecord,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let pane_id = record.id.clone();
    let pane_name = record.name.clone();
    state.pane_handle.upsert(record).await.map_err(|e| e.to_string())?;

    // Try to auto-claim an alias from the pane's name. No-op if the
    // name doesn't match the alias format (capitals, spaces, reserved)
    // or if the canonical alias is already held by another pane.
    // Project scope is `None` for the MVP — pane→session→project lookup
    // is a follow-up.
    state.alias_manager.try_auto_claim_from_pane_name(
        &pane_id,
        pane_name.as_deref(),
        None,
        Some(&app),
    );
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_pane_record(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.pane_handle.remove(&id).await.map_err(|e| e.to_string())?;

    // Release auto-claimed aliases held by this pane. Manual `roux alias
    // claim` bindings persist — queued mail addressed to them survives
    // for the next session that claims them.
    state.alias_manager.unbind_for_pane(&id, true, Some(&app));
    Ok(())
}
