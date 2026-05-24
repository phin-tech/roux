use crate::state::AppState;
use roux_core::{WorkItem, WorkItemInput, WorkItemStatus};

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_list(
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkItem>, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-list"))
    {
        return client.work_item_list(project_id).await;
    }
    state.runtime.work_item_handle.list(project_id.as_deref())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_create(
    input: WorkItemInput,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-create"))
    {
        return client.work_item_create(input).await;
    }
    state.runtime.work_item_handle.create(input)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_update(
    id: String,
    input: WorkItemInput,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-update"))
    {
        return client.work_item_update(id, input).await;
    }
    state.runtime.work_item_handle.update(&id, input)?.ok_or_else(|| "work item not found".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_move(
    id: String,
    status: WorkItemStatus,
    sort_order: f64,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-move"))
    {
        return client.work_item_move(id, status, sort_order).await;
    }
    state
        .runtime
        .work_item_handle
        .move_item(&id, status, sort_order)?
        .ok_or_else(|| "work item not found".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_delete(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-delete"))
    {
        return client.work_item_delete(id).await;
    }
    if state.runtime.work_item_handle.delete(&id)? {
        Ok(id)
    } else {
        Err("work item not found".to_string())
    }
}
