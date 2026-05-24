pub(crate) use roux_runtime::automation_hooks::{
    context_from_run_request, hook_list_to_value, hook_run_to_value, request_from_socket_args,
    worktree_provider_hooks, AutomationHookManager, HookContext, HookEvent, HookListItem,
    HookLogEntry, HookPreviewItem, HookRunRequest, HookRunSummary,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_list_automation_hooks(
    repo_path: Option<String>,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<HookListItem>, String> {
    if let Some(client) = state.daemon_client.clone().filter(|client| client.supports("hook-show"))
    {
        return client.list_automation_hooks(repo_path).await.map_err(Into::into);
    }
    state.automation_hooks.list_hooks(repo_path.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_preview_automation_hooks(
    request: HookRunRequest,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<HookPreviewItem>, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("hook-preview"))
    {
        return client.preview_automation_hooks(request).await.map_err(Into::into);
    }
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let wt_available = crate::services::setup::resolve_wt_binary().is_some();
    let (event, context) =
        context_from_run_request(request, Some(settings.worktree_provider), wt_available)
            .map_err(|e| e.to_string())?;
    state.automation_hooks.preview(event, &context).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_run_automation_hook(
    request: HookRunRequest,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<HookRunSummary, String> {
    if let Some(client) = state.daemon_client.clone().filter(|client| client.supports("hook-run")) {
        return client.run_automation_hook(request).await.map_err(Into::into);
    }
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let wt_available = crate::services::setup::resolve_wt_binary().is_some();
    let (event, context) =
        context_from_run_request(request, Some(settings.worktree_provider), wt_available)
            .map_err(|e| e.to_string())?;
    let ran = if event.is_blocking() {
        state.automation_hooks.run_blocking(event, context).await.map_err(|e| e.to_string())?
    } else {
        state.automation_hooks.run_background(event, context).await.map_err(|e| e.to_string())?
    };
    Ok(HookRunSummary { event: event.as_str().into(), ran })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_approve_automation_hook(
    approval_id: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("hook-approve"))
    {
        return client.approve_automation_hook(approval_id).await.map_err(Into::into);
    }
    state.automation_hooks.approve(&approval_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_clear_automation_hook_approvals(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("hook-clear-approvals"))
    {
        return client.clear_automation_hook_approvals().await.map_err(Into::into);
    }
    state.automation_hooks.clear_approvals().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_list_automation_hook_logs(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<HookLogEntry>, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("hook-log-list"))
    {
        return client.list_automation_hook_logs().await.map_err(Into::into);
    }
    Ok(state.automation_hooks.list_logs())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_read_automation_hook_log(
    path: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<String, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("hook-log-read"))
    {
        return client.read_automation_hook_log(path).await.map_err(Into::into);
    }
    state.automation_hooks.read_log(&path).map_err(|e| e.to_string())
}
