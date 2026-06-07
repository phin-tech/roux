use crate::state::AppState;
use roux_core::{
    Attachment, AttachmentDocument, AttachmentInput, AttachmentTargetKind, WorkItem,
    WorkItemDecision, WorkItemDecisionOption, WorkItemInput, WorkItemPlanResult,
    WorkItemReviewAcceptResult, WorkItemReviewRequestChangesResult, WorkItemReviewRequestResult,
    WorkItemRun, WorkItemRunEvent, WorkItemStageRunResult, WorkItemStartResult, WorkItemStatus,
};

#[derive(Debug, Default, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkItemStartCommandOptions {
    profile: Option<String>,
    repo_path: Option<String>,
    name: Option<String>,
    worktree_path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    fetch_first: Option<bool>,
    force_start: Option<bool>,
    fix_ci: Option<bool>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_list(
    project_id: Option<String>,
    include_archived: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkItem>, String> {
    let include_archived = include_archived.unwrap_or(false);
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-list")) {
        return client.work_item_list(project_id, include_archived).await.map_err(String::from);
    }
    state.runtime.work_item_handle.list_with_archived(project_id.as_deref(), include_archived)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_create(
    input: WorkItemInput,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-create")) {
        return client.work_item_create(input).await.map_err(String::from);
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
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-update")) {
        return client.work_item_update(id, input).await.map_err(String::from);
    }
    state
        .runtime
        .work_item_handle
        .update(&id, input)?
        .ok_or_else(|| "work item not found".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_move(
    id: String,
    status: WorkItemStatus,
    sort_order: f64,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-move")) {
        return client.work_item_move(id, status, sort_order).await.map_err(String::from);
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
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-delete")) {
        return client.work_item_delete(id).await.map_err(String::from);
    }
    if state.runtime.work_item_handle.delete(&id)? {
        Ok(id)
    } else {
        Err("work item not found".to_string())
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_archive(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-archive")) {
        return client.work_item_archive(id).await.map_err(String::from);
    }
    let item = state
        .runtime
        .work_item_handle
        .get(&id)?
        .ok_or_else(|| "work item not found".to_string())?;
    if state.runtime.work_item_handle.has_active_run(&id)? {
        return Err("active work item run".to_string());
    }
    if let Some(session_id) = item.session_id.as_deref() {
        archive_linked_session(&state, session_id).await?;
    }
    state.runtime.work_item_handle.archive(&id)?.ok_or_else(|| "work item not found".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_restore(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-restore")) {
        return client.work_item_restore(id).await.map_err(String::from);
    }
    state.runtime.work_item_handle.restore(&id)?.ok_or_else(|| "work item not found".to_string())
}

async fn archive_linked_session(state: &AppState, session_id: &str) -> Result<(), String> {
    let ptys = state.runtime.pty_handle.list().await.map_err(|err| err.to_string())?;
    for pty in ptys {
        if pty.info.session_id.as_deref() == Some(session_id) {
            let _ = state.runtime.pty_handle.remove(&pty.id).await;
        }
    }
    state.runtime.session_handle.archive(session_id).await.map_err(|err| err.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_attach_session(
    id: String,
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-attach-session"))
    {
        return client.work_item_attach_session(id, session_id).await.map_err(String::from);
    }
    Err("Attaching a work item session requires a running daemon.".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_detach_session(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItem, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-detach-session"))
    {
        return client.work_item_detach_session(id).await.map_err(String::from);
    }
    Err("Detaching a work item session requires a running daemon.".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_start(
    id: String,
    options: Option<WorkItemStartCommandOptions>,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemStartResult, String> {
    let options = options.unwrap_or_default();
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-start")) {
        validate_fix_ci_start_capability(
            options.fix_ci,
            client.supports("work-item-start-fix-ci"),
        )?;
        return client
            .work_item_start(
                id,
                options.profile,
                options.repo_path,
                options.name,
                options.worktree_path,
                options.branch,
                options.base,
                options.fetch_first,
                options.force_start,
                options.fix_ci,
            )
            .await
            .map_err(String::from);
    }
    Err("Starting a work item requires a running daemon.".to_string())
}

fn validate_fix_ci_start_capability(
    fix_ci: Option<bool>,
    supports_fix_ci: bool,
) -> Result<(), String> {
    if fix_ci.unwrap_or(false) && !supports_fix_ci {
        return Err(
            "Fix CI work item starts require daemon capability work-item-start-fix-ci. Restart the Roux daemon."
                .to_string(),
        );
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_plan(
    id: String,
    profile: Option<String>,
    repo_path: Option<String>,
    name: Option<String>,
    worktree_path: Option<String>,
    replace_active: bool,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemPlanResult, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-plan")) {
        return client
            .work_item_plan(id, profile, repo_path, name, worktree_path, replace_active)
            .await
            .map_err(String::from);
    }
    Err("Planning a work item requires a running daemon.".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_run_stage(
    id: String,
    stage_id: Option<String>,
    outcome: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemStageRunResult, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-run-stage"))
    {
        return client.work_item_run_stage(id, stage_id, outcome).await.map_err(String::from);
    }
    Err("Running a workflow stage requires a running daemon.".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_review_accept(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemReviewAcceptResult, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-review-accept"))
    {
        return client.work_item_review_accept(id).await.map_err(String::from);
    }
    Err("Accepting work item review requires a running daemon.".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_review_request(
    run_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemReviewRequestResult, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-review-request"))
    {
        return client.work_item_review_request(run_id).await.map_err(String::from);
    }
    Err("Requesting work item review requires a running daemon.".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_item_review_request_changes(
    id: String,
    note: String,
    status: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemReviewRequestChangesResult, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-review-request-changes"))
    {
        return client
            .work_item_review_request_changes(id, note, status)
            .await
            .map_err(String::from);
    }
    Err("Requesting work item review changes requires a running daemon.".to_string())
}

#[tauri::command]
pub(crate) async fn work_item_runs_list(
    work_item_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkItemRun>, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-runs-list"))
    {
        return client.work_item_runs_list(work_item_id).await.map_err(String::from);
    }
    state.runtime.work_item_handle.list_runs(work_item_id.as_deref())
}

#[tauri::command]
pub(crate) async fn work_item_run_events(
    run_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkItemRunEvent>, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-run-events"))
    {
        return client.work_item_run_events(run_id).await.map_err(String::from);
    }
    state.runtime.work_item_handle.list_run_events(&run_id)
}

#[tauri::command]
pub(crate) async fn work_item_run_stop(
    run_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemRun, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("work-item-run-stop")) {
        return client.work_item_run_stop(run_id).await.map_err(String::from);
    }
    Err("Stopping a work item run requires a running daemon.".to_string())
}

#[tauri::command]
pub(crate) async fn work_item_decision_create(
    run_id: String,
    question: String,
    options: Vec<WorkItemDecisionOption>,
    default_value: Option<String>,
    timeout_at: Option<u64>,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemDecision, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-decision-create"))
    {
        return client
            .work_item_decision_create(run_id, question, options, default_value, timeout_at)
            .await
            .map_err(String::from);
    }
    state.runtime.work_item_handle.create_decision(
        &run_id,
        &question,
        options,
        default_value.as_deref(),
        timeout_at,
    )
}

#[tauri::command]
pub(crate) async fn work_item_decisions_list(
    work_item_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkItemDecision>, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-decisions-list"))
    {
        return client.work_item_decisions_list(work_item_id).await.map_err(String::from);
    }
    state.runtime.work_item_handle.list_pending_decisions(work_item_id.as_deref())
}

#[tauri::command]
pub(crate) async fn work_item_decision_resolve(
    id: String,
    value: String,
    resolved_by: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<WorkItemDecision, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|c| c.supports("work-item-decision-resolve"))
    {
        return client
            .work_item_decision_resolve(id, value, resolved_by)
            .await
            .map_err(String::from);
    }
    state
        .runtime
        .work_item_handle
        .resolve_decision(&id, &value, resolved_by.as_deref())?
        .ok_or_else(|| "decision not found".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn document_attach(
    input: AttachmentInput,
    state: tauri::State<'_, AppState>,
) -> Result<Attachment, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("document-attach")) {
        return client.document_attach(input).await.map_err(String::from);
    }
    validate_document_target(&state, &input.target_kind, &input.target_id).await?;
    state.runtime.work_item_handle.create_attachment(input)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn document_list(
    target_kind: Option<AttachmentTargetKind>,
    target_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Attachment>, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("document-list")) {
        return client.document_list(target_kind, target_id).await.map_err(String::from);
    }
    state.runtime.work_item_handle.list_attachments(target_kind, target_id.as_deref())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn document_get(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<AttachmentDocument, String> {
    if let Some(client) = state.daemon_client.clone().filter(|c| c.supports("document-get")) {
        return client.document_get(id).await.map_err(String::from);
    }
    state
        .runtime
        .work_item_handle
        .get_attachment_document(&id)?
        .ok_or_else(|| "document not found".to_string())
}

async fn validate_document_target(
    state: &AppState,
    target_kind: &AttachmentTargetKind,
    target_id: &str,
) -> Result<(), String> {
    match target_kind {
        AttachmentTargetKind::Session => {
            if state
                .runtime
                .session_handle
                .get(target_id)
                .await
                .map_err(|_| "session service unavailable".to_string())?
                .is_some()
            {
                Ok(())
            } else {
                Err("session not found".to_string())
            }
        }
        AttachmentTargetKind::WorkItem => {
            if state.runtime.work_item_handle.get(target_id)?.is_some() {
                Ok(())
            } else {
                Err("work item not found".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_ci_start_requires_granular_daemon_capability() {
        let err = validate_fix_ci_start_capability(Some(true), false).unwrap_err();

        assert!(err.contains("work-item-start-fix-ci"));
        assert!(err.contains("Restart the Roux daemon"));
    }

    #[test]
    fn normal_start_does_not_require_fix_ci_capability() {
        assert!(validate_fix_ci_start_capability(None, false).is_ok());
        assert!(validate_fix_ci_start_capability(Some(false), false).is_ok());
        assert!(validate_fix_ci_start_capability(Some(true), true).is_ok());
    }
}
