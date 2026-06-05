use serde::Serialize;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

use roux_core::WorkItemInputPresence;
use roux_runtime::host::RuntimeHost;
use roux_runtime::pty_service::{PtyOutputEvent, PtySpawnRequest, PTY_OUTPUT_DEFAULT_POLL_BYTES};

use super::identity::{request_authorized, DaemonIdentity};
use super::protocol::{Request, Response};
use super::{
    bool_arg, handle_session_create_shell, kill_session_ptys, load_daemon_settings,
    optional_nullable_string_arg, optional_string_arg, parse_pty_env_request,
};

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum WorkItemEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: Box<roux_core::WorkItemEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

fn default_work_item_sort_order() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as f64)
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Work item handlers
// ---------------------------------------------------------------------------

pub(super) async fn handle_work_item_list(req: Request, host: &RuntimeHost) -> Response {
    let project_id = optional_string_arg(&req.args, &["projectId", "project_id"]);
    match host.work_item_handle.list(project_id.as_deref()) {
        Ok(items) => match serde_json::to_value(&items) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work items: {err}")),
        },
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_create(req: Request, host: &RuntimeHost) -> Response {
    let Some(title) = optional_string_arg(&req.args, &["title"]) else {
        return Response::err("title required");
    };
    let input = roux_core::WorkItemInput {
        title,
        body: optional_string_arg(&req.args, &["body"]),
        status: req
            .args
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(roux_core::WorkItemStatus::from_str_opt),
        repo_path: optional_string_arg(&req.args, &["repoPath", "repo_path"]),
        agent_profile: optional_string_arg(&req.args, &["agentProfile", "agent_profile"]),
        base_branch: optional_string_arg(&req.args, &["baseBranch", "base_branch", "base"]),
        worktree_path: optional_string_arg(&req.args, &["worktreePath", "worktree_path"]),
        branch: optional_string_arg(&req.args, &["branch", "worktreeBranch", "worktree_branch"]),
        fetch_first: bool_arg(&req.args, &["fetchFirst", "fetch_first"]),
        start_error: optional_nullable_string_arg(&req.args, &["startError", "start_error"]),
        project_id: optional_string_arg(&req.args, &["projectId", "project_id"]),
        parent_id: optional_string_arg(&req.args, &["parentId", "parent_id"]),
        external_ref: None,
        sort_order: req
            .args
            .get("sortOrder")
            .or_else(|| req.args.get("sort_order"))
            .and_then(|v| v.as_f64()),
        field_presence: work_item_input_presence(&req.args),
    };
    match host.work_item_handle.create(input) {
        Ok(item) => match serde_json::to_value(&item) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item: {err}")),
        },
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_update(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id"]) else {
        return Response::err("id required");
    };
    let Some(title) = optional_string_arg(&req.args, &["title"]) else {
        return Response::err("title required");
    };
    let input = roux_core::WorkItemInput {
        title,
        body: optional_string_arg(&req.args, &["body"]),
        status: req
            .args
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(roux_core::WorkItemStatus::from_str_opt),
        repo_path: optional_string_arg(&req.args, &["repoPath", "repo_path"]),
        agent_profile: optional_string_arg(&req.args, &["agentProfile", "agent_profile"]),
        base_branch: optional_string_arg(&req.args, &["baseBranch", "base_branch", "base"]),
        worktree_path: optional_string_arg(&req.args, &["worktreePath", "worktree_path"]),
        branch: optional_string_arg(&req.args, &["branch", "worktreeBranch", "worktree_branch"]),
        fetch_first: bool_arg(&req.args, &["fetchFirst", "fetch_first"]),
        start_error: optional_nullable_string_arg(&req.args, &["startError", "start_error"]),
        project_id: optional_string_arg(&req.args, &["projectId", "project_id"]),
        parent_id: optional_string_arg(&req.args, &["parentId", "parent_id"]),
        external_ref: None,
        sort_order: req
            .args
            .get("sortOrder")
            .or_else(|| req.args.get("sort_order"))
            .and_then(|v| v.as_f64()),
        field_presence: work_item_input_presence(&req.args),
    };
    match host.work_item_handle.update(&id, input) {
        Ok(Some(item)) => match serde_json::to_value(&item) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item: {err}")),
        },
        Ok(None) => Response::err("work item not found"),
        Err(err) => Response::err(err),
    }
}

fn work_item_input_presence(args: &serde_json::Value) -> WorkItemInputPresence {
    WorkItemInputPresence {
        body: has_arg(args, &["body"]),
        repo_path: has_arg(args, &["repoPath", "repo_path"]),
        agent_profile: has_arg(args, &["agentProfile", "agent_profile"]),
        base_branch: has_arg(args, &["baseBranch", "base_branch", "base"]),
        worktree_path: has_arg(args, &["worktreePath", "worktree_path"]),
        branch: has_arg(args, &["branch", "worktreeBranch", "worktree_branch"]),
        fetch_first: has_arg(args, &["fetchFirst", "fetch_first"]),
        start_error: has_arg(args, &["startError", "start_error"]),
        project_id: has_arg(args, &["projectId", "project_id"]),
        parent_id: has_arg(args, &["parentId", "parent_id"]),
    }
}

fn has_arg(args: &serde_json::Value, names: &[&str]) -> bool {
    names.iter().any(|name| args.get(*name).is_some())
}

pub(super) async fn handle_work_item_move(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id"]) else {
        return Response::err("id required");
    };
    let Some(status_str) = optional_string_arg(&req.args, &["status"]) else {
        return Response::err("status required");
    };
    let Some(status) = roux_core::WorkItemStatus::from_str_opt(&status_str) else {
        return Response::err(format!("unknown status: {status_str}"));
    };
    let sort_order = req
        .args
        .get("sortOrder")
        .or_else(|| req.args.get("sort_order"))
        .and_then(|v| v.as_f64())
        .unwrap_or_else(default_work_item_sort_order);
    match host.work_item_handle.move_item(&id, status, sort_order) {
        Ok(Some(item)) => match serde_json::to_value(&item) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item: {err}")),
        },
        Ok(None) => Response::err("work item not found"),
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_delete(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id"]) else {
        return Response::err("id required");
    };
    match host.work_item_handle.delete(&id) {
        Ok(true) => Response::success(serde_json::json!({ "id": id })),
        Ok(false) => Response::err("work item not found"),
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_attach_session(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id"]) else {
        return Response::err("id required");
    };
    let Some(session_id) = optional_string_arg(&req.args, &["sessionId", "session_id"]) else {
        return Response::err("sessionId required");
    };

    let item = match host.work_item_handle.get(&id) {
        Ok(Some(item)) => item,
        Ok(None) => return Response::err("work item not found"),
        Err(err) => return Response::err(err),
    };
    let session = match host.session_handle.get(&session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    };
    let bound_work_item_id = match host.work_item_handle.list(None) {
        Ok(items) => items
            .into_iter()
            .find(|candidate| candidate.session_id.as_deref() == Some(session_id.as_str()))
            .map(|candidate| candidate.id),
        Err(err) => return Response::err(err),
    };

    let decision =
        match roux_core::decide_work_item_session_attach(roux_core::WorkItemSessionAttachInput {
            work_item_id: item.id.clone(),
            work_item_project_id: item.project_id.clone(),
            session_id: session.id.clone(),
            session_project_id: session.project_id.clone(),
            session_bound_work_item_id: bound_work_item_id,
        }) {
            Ok(decision) => decision,
            Err(err) => return Response::err(err.to_string()),
        };

    let mut item = match host.work_item_handle.attach_session(&item.id, &session_id) {
        Ok(Some(item)) => item,
        Ok(None) => return Response::err("work item not found"),
        Err(err) => return Response::err(err),
    };

    if let Some(project_id) = decision.session_project_id_update {
        if let Err(err) = host.session_handle.set_project(&session_id, Some(project_id)).await {
            return Response::err(err.to_string());
        }
    }
    if let Some(project_id) = decision.work_item_project_id_update {
        let input = work_item_project_update_input(&item, project_id);
        item = match host.work_item_handle.update(&id, input) {
            Ok(Some(item)) => item,
            Ok(None) => return Response::err("work item not found"),
            Err(err) => return Response::err(err),
        };
    }

    match serde_json::to_value(&item) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize work item: {err}")),
    }
}

pub(super) async fn handle_work_item_detach_session(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id"]) else {
        return Response::err("id required");
    };
    match host.work_item_handle.detach_session(&id) {
        Ok(Some(item)) => match serde_json::to_value(&item) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item: {err}")),
        },
        Ok(None) => Response::err("work item not found"),
        Err(err) => Response::err(err),
    }
}

fn work_item_project_update_input(
    item: &roux_core::WorkItem,
    project_id: String,
) -> roux_core::WorkItemInput {
    roux_core::WorkItemInput {
        title: item.title.clone(),
        body: item.body.clone(),
        status: Some(item.status.clone()),
        repo_path: item.repo_path.clone(),
        agent_profile: item.agent_profile.clone(),
        base_branch: item.base_branch.clone(),
        worktree_path: item.worktree_path.clone(),
        branch: item.branch.clone(),
        fetch_first: item.fetch_first,
        start_error: item.start_error.clone(),
        project_id: Some(project_id),
        parent_id: item.parent_id.clone(),
        external_ref: None,
        sort_order: Some(item.sort_order),
        field_presence: WorkItemInputPresence { project_id: true, ..Default::default() },
    }
}

pub(super) async fn handle_document_attach(req: Request, host: &RuntimeHost) -> Response {
    let Some(target_kind) = optional_string_arg(&req.args, &["targetKind", "target_kind"]) else {
        return Response::err("targetKind required");
    };
    let Some(target_kind) = roux_core::AttachmentTargetKind::from_str_opt(&target_kind) else {
        return Response::err(format!("unknown targetKind: {target_kind}"));
    };
    let Some(target_id) = optional_string_arg(&req.args, &["targetId", "target_id"]) else {
        return Response::err("targetId required");
    };
    let Some(content) = optional_string_arg(&req.args, &["content"]) else {
        return Response::err("content required");
    };
    let content_kind = match optional_string_arg(&req.args, &["contentKind", "content_kind"]) {
        Some(kind) => match roux_core::AttachmentContentKind::from_str_opt(&kind) {
            Some(kind) => kind,
            None => return Response::err(format!("unknown contentKind: {kind}")),
        },
        None => roux_core::AttachmentContentKind::Text,
    };

    if let Err(err) = validate_attachment_target(host, &target_kind, &target_id).await {
        return Response::err(err);
    }

    let input = roux_core::AttachmentInput {
        target_kind,
        target_id,
        title: optional_string_arg(&req.args, &["title"]),
        content_kind,
        content,
        mime_type: optional_string_arg(&req.args, &["mimeType", "mime_type"]),
        source_path: optional_string_arg(&req.args, &["sourcePath", "source_path"]),
    };
    match host.work_item_handle.create_attachment(input) {
        Ok(attachment) => match serde_json::to_value(&attachment) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize document attachment: {err}")),
        },
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_document_list(req: Request, host: &RuntimeHost) -> Response {
    let target_kind = match optional_string_arg(&req.args, &["targetKind", "target_kind"]) {
        Some(kind) => match roux_core::AttachmentTargetKind::from_str_opt(&kind) {
            Some(kind) => Some(kind),
            None => return Response::err(format!("unknown targetKind: {kind}")),
        },
        None => None,
    };
    let target_id = optional_string_arg(&req.args, &["targetId", "target_id"]);
    if target_id.is_some() && target_kind.is_none() {
        return Response::err("targetKind required when targetId is provided");
    }
    match host.work_item_handle.list_attachments(target_kind, target_id.as_deref()) {
        Ok(attachments) => match serde_json::to_value(&attachments) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize document attachments: {err}")),
        },
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_document_get(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id", "documentId", "document_id"]) else {
        return Response::err("id required");
    };
    match host.work_item_handle.get_attachment_document(&id) {
        Ok(Some(document)) => match serde_json::to_value(&document) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize document: {err}")),
        },
        Ok(None) => Response::err("document not found"),
        Err(err) => Response::err(err),
    }
}

async fn validate_attachment_target(
    host: &RuntimeHost,
    target_kind: &roux_core::AttachmentTargetKind,
    target_id: &str,
) -> Result<(), String> {
    match target_kind {
        roux_core::AttachmentTargetKind::Session => {
            if host
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
        roux_core::AttachmentTargetKind::WorkItem => {
            if host.work_item_handle.get(target_id)?.is_some() {
                Ok(())
            } else {
                Err("work item not found".to_string())
            }
        }
    }
}

pub(super) async fn handle_work_item_start(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    match start_work_item_run(req, host, identity).await {
        Ok(result) => match serde_json::to_value(result) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item start: {err}")),
        },
        Err(resp) => resp,
    }
}

pub(super) async fn handle_work_item_plan(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    match plan_work_item_run(req, host, identity).await {
        Ok(result) => match serde_json::to_value(result) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item plan: {err}")),
        },
        Err(resp) => resp,
    }
}

pub(super) async fn handle_work_item_review_accept(req: Request, host: &RuntimeHost) -> Response {
    let Some(item_id) = optional_string_arg(&req.args, &["id", "workItemId", "work_item_id"])
    else {
        return Response::err("id required");
    };
    let payload = serde_json::json!({
        "reason": "reviewAccepted",
        "acceptedBy": optional_string_arg(&req.args, &["acceptedBy", "accepted_by"])
            .unwrap_or_else(|| "user".to_string()),
    });
    match host.work_item_handle.accept_review(&item_id, payload) {
        Ok(Some((item, run))) => {
            match serde_json::to_value(roux_core::WorkItemReviewAcceptResult { item, run }) {
                Ok(value) => Response::success(value),
                Err(err) => {
                    Response::err(format!("failed to serialize work item review accept: {err}"))
                }
            }
        }
        Ok(None) => Response::err("work item has no review run to accept"),
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_review_request(req: Request, host: &RuntimeHost) -> Response {
    let Some(run_id) = optional_string_arg(&req.args, &["id", "runId", "run_id"]) else {
        return Response::err("runId required");
    };
    let payload = serde_json::json!({
        "reason": "reviewRequested",
        "requestedBy": optional_string_arg(&req.args, &["requestedBy", "requested_by"])
            .unwrap_or_else(|| "agent".to_string()),
    });
    let result_payload = review_request_result_payload(&req.args);
    match host.work_item_handle.request_review(&run_id, payload, result_payload) {
        Ok(Some((item, run))) => {
            match serde_json::to_value(roux_core::WorkItemReviewRequestResult { item, run }) {
                Ok(value) => Response::success(value),
                Err(err) => {
                    Response::err(format!("failed to serialize work item review request: {err}"))
                }
            }
        }
        Ok(None) => Response::err("work item implementation run cannot request review"),
        Err(err) => Response::err(err),
    }
}

fn review_request_result_payload(args: &serde_json::Value) -> Option<serde_json::Value> {
    let mut payload = serde_json::Map::new();
    if let Some(summary) = optional_string_arg(args, &["summary", "agentSummary", "agent_summary"])
    {
        if !summary.trim().is_empty() {
            payload.insert("summary".into(), serde_json::Value::String(summary));
        }
    }
    let tests = optional_string_list_arg(args, &["tests", "test"]);
    if !tests.is_empty() {
        payload.insert(
            "tests".into(),
            serde_json::Value::Array(tests.into_iter().map(serde_json::Value::String).collect()),
        );
    }
    let changed_files =
        optional_string_list_arg(args, &["changedFiles", "changed_files", "files", "file"]);
    if !changed_files.is_empty() {
        payload.insert(
            "changedFiles".into(),
            serde_json::Value::Array(
                changed_files.into_iter().map(serde_json::Value::String).collect(),
            ),
        );
    }
    (!payload.is_empty()).then_some(serde_json::Value::Object(payload))
}

fn optional_string_list_arg(args: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    for key in keys {
        let Some(value) = args.get(*key) else {
            continue;
        };
        let values = string_list_arg(value);
        if !values.is_empty() {
            return values;
        }
    }
    Vec::new()
}

fn string_list_arg(value: &serde_json::Value) -> Vec<String> {
    if let Some(value) = value.as_str() {
        let value = value.trim();
        return (!value.is_empty()).then(|| value.to_string()).into_iter().collect();
    }
    let Some(values) = value.as_array() else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) async fn handle_work_item_review_request_changes(
    req: Request,
    host: &RuntimeHost,
) -> Response {
    let Some(note) = optional_string_arg(&req.args, &["note"]) else {
        return Response::err("note required");
    };
    if note.trim().is_empty() {
        return Response::err("note required");
    }
    let target_status = match request_changes_target_status(&req.args) {
        Ok(status) => status,
        Err(err) => return err,
    };
    let run = match resolve_review_changes_run(host, &req.args) {
        Ok(run) => run,
        Err(err) => return err,
    };
    if run.kind != roux_core::WorkItemRunKind::Implementation {
        return Response::err("work item changes can only be requested for implementation runs");
    }
    if run.status != roux_core::WorkItemRunStatus::Review {
        return Response::err("work item changes can only be requested for review runs");
    }

    let title =
        optional_string_arg(&req.args, &["title"]).unwrap_or_else(|| "Review feedback".to_string());
    let attachment_input = roux_core::AttachmentInput {
        target_kind: roux_core::AttachmentTargetKind::WorkItem,
        target_id: run.work_item_id.clone(),
        title: Some(title),
        content_kind: roux_core::AttachmentContentKind::Text,
        content: note.clone(),
        mime_type: Some("text/markdown".to_string()),
        source_path: None,
    };

    let payload = serde_json::json!({
        "reason": "changesRequested",
        "requestedBy": optional_string_arg(&req.args, &["requestedBy", "requested_by"])
            .unwrap_or_else(|| "user".to_string()),
        "note": note,
        "targetStatus": target_status.as_str(),
    });
    match host.work_item_handle.request_changes(
        &run.id,
        target_status,
        payload,
        Some(attachment_input),
    ) {
        Ok(Some((item, run, Some(attachment)))) => {
            match serde_json::to_value(roux_core::WorkItemReviewRequestChangesResult {
                item,
                run,
                attachment,
            }) {
                Ok(value) => Response::success(value),
                Err(err) => Response::err(format!(
                    "failed to serialize work item review request changes: {err}"
                )),
            }
        }
        Ok(Some((_item, _run, None))) => {
            Response::err("work item review run cannot request changes without feedback")
        }
        Ok(None) => Response::err("work item review run cannot request changes"),
        Err(err) => Response::err(err),
    }
}

fn request_changes_target_status(
    args: &serde_json::Value,
) -> Result<roux_core::WorkItemStatus, Response> {
    let status = optional_string_arg(args, &["status", "targetStatus", "target_status"])
        .unwrap_or_else(|| roux_core::WorkItemStatus::Doing.as_str().to_string());
    let Some(status) = roux_core::WorkItemStatus::from_str_opt(&status) else {
        return Err(Response::err(format!("unknown status: {status}")));
    };
    match status {
        roux_core::WorkItemStatus::Doing | roux_core::WorkItemStatus::Ready => Ok(status),
        _ => Err(Response::err("request changes status must be doing or ready")),
    }
}

fn resolve_review_changes_run(
    host: &RuntimeHost,
    args: &serde_json::Value,
) -> Result<roux_core::WorkItemRun, Response> {
    if let Some(run_id) = optional_string_arg(args, &["runId", "run_id"]) {
        return match host.work_item_handle.get_run(&run_id) {
            Ok(Some(run)) => Ok(run),
            Ok(None) => Err(Response::err("work item run not found")),
            Err(err) => Err(Response::err(err)),
        };
    }
    if let Some(work_item_id) = optional_string_arg(args, &["workItemId", "work_item_id"]) {
        return latest_review_implementation_run(host, &work_item_id);
    }
    let Some(id) = optional_string_arg(args, &["id"]) else {
        return Err(Response::err("id or runId required"));
    };
    match host.work_item_handle.get_run(&id) {
        Ok(Some(run)) => Ok(run),
        Ok(None) => latest_review_implementation_run(host, &id),
        Err(err) => Err(Response::err(err)),
    }
}

fn latest_review_implementation_run(
    host: &RuntimeHost,
    work_item_id: &str,
) -> Result<roux_core::WorkItemRun, Response> {
    let runs = host.work_item_handle.list_runs(Some(work_item_id)).map_err(Response::err)?;
    runs.into_iter()
        .filter(|run| run.kind == roux_core::WorkItemRunKind::Implementation)
        .filter(|run| run.status == roux_core::WorkItemRunStatus::Review)
        .max_by_key(|run| (run.updated_at, run.created_at, run.id.clone()))
        .ok_or_else(|| Response::err("work item has no review run to request changes"))
}

type ProfileDispatchFuture<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;
type ProfileDispatcher = for<'a> fn(
    &'a RuntimeHost,
    &'a roux_core::WorkItem,
    &'a str,
    &'a str,
    &'a str,
    &'a DaemonIdentity,
    bool,
) -> ProfileDispatchFuture<'a>;
type AfterSessionCreatedHook = fn(&RuntimeHost, &str, &roux_core::Session);

fn real_profile_dispatcher<'a>(
    host: &'a RuntimeHost,
    item: &'a roux_core::WorkItem,
    run_id: &'a str,
    session_id: &'a str,
    profile_id: &'a str,
    identity: &'a DaemonIdentity,
    force_start: bool,
) -> ProfileDispatchFuture<'a> {
    Box::pin(run_dispatched_profile(
        host,
        item,
        run_id,
        session_id,
        profile_id,
        identity,
        force_start,
    ))
}

fn real_planning_profile_dispatcher<'a>(
    host: &'a RuntimeHost,
    item: &'a roux_core::WorkItem,
    run_id: &'a str,
    session_id: &'a str,
    profile_id: &'a str,
    identity: &'a DaemonIdentity,
    _force_start: bool,
) -> ProfileDispatchFuture<'a> {
    Box::pin(run_dispatched_planning_profile(host, item, run_id, session_id, profile_id, identity))
}

async fn plan_work_item_run(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Result<roux_core::WorkItemPlanResult, Response> {
    plan_work_item_run_with_hooks(req, host, identity, real_planning_profile_dispatcher).await
}

async fn plan_work_item_run_with_hooks(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
    dispatch_profile: ProfileDispatcher,
) -> Result<roux_core::WorkItemPlanResult, Response> {
    let Some(item_id) = optional_string_arg(&req.args, &["id"]) else {
        return Err(Response::err("id required"));
    };
    let item = match host.work_item_handle.get(&item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return Err(Response::err("work item not found")),
        Err(err) => return Err(Response::err(err)),
    };
    let replace_active = bool_arg(&req.args, &["replaceActive", "replace_active"]).unwrap_or(false);

    if let Some(run) = active_work_item_run(host, &item_id)? {
        if run.kind == roux_core::WorkItemRunKind::Planning {
            if replace_active {
                stop_planning_run_for_replacement(host, &run).await?;
            } else {
                let Some(session_id) = run.session_id.as_deref() else {
                    return Err(Response::err("active planning run has no session"));
                };
                let session = match host.session_handle.get(session_id).await {
                    Ok(Some(session)) => session,
                    Ok(None) => return Err(Response::err("active planning session not found")),
                    Err(err) => return Err(Response::err(err.to_string())),
                };
                return Ok(roux_core::WorkItemPlanResult { item, run, session });
            }
        } else {
            return Err(Response::err("work item already has an active run"));
        }
    }

    let settings = load_daemon_settings();
    let profile_id = optional_string_arg(&req.args, &["profile", "agentProfile", "agent_profile"])
        .or_else(|| item.agent_profile.clone())
        .unwrap_or_else(|| settings.kanban.default_agent_profile.clone());
    let Some(profile) = roux_core::providers::resolve_profile(&profile_id, &settings) else {
        return Err(Response::err(format!("agent profile not found: {profile_id}")));
    };
    if let Some(reason) = autonomous_profile_rejection_reason(&profile) {
        return Err(Response::err(reason));
    }

    let repo_path = resolve_planning_repo_path(&req, host, &item).await?;
    let name = optional_string_arg(&req.args, &["name"]).unwrap_or_else(|| item.title.clone());
    let explicit_worktree_path =
        optional_nullable_string_arg(&req.args, &["worktreePath", "worktree_path"]);
    let requested_branch =
        optional_nullable_string_arg(&req.args, &["branch", "worktreeBranch", "worktree_branch"])
            .or_else(|| item.branch.clone());
    let base = optional_nullable_string_arg(&req.args, &["base", "startPoint", "start_point"])
        .or_else(|| item.base_branch.clone())
        .or_else(|| Some("main".to_string()));
    let fetch_first = bool_arg(&req.args, &["fetchFirst", "fetch_first"]).or(item.fetch_first);
    let worktree_path = explicit_worktree_path.or_else(|| {
        if requested_branch.is_none() {
            item.worktree_path.clone()
        } else {
            None
        }
    });
    let branch = if worktree_path.is_some() {
        requested_branch
    } else {
        requested_branch.or_else(|| Some(default_work_item_branch(&item)))
    };
    let base_branch = base.clone();
    let mut session_args = serde_json::json!({
        "repoPath": repo_path.clone(),
        "name": name,
        "projectId": item.project_id.clone(),
        "profile": profile_id.clone(),
    });
    if let Some(worktree_path) = worktree_path.as_deref() {
        session_args["worktreePath"] = serde_json::Value::String(worktree_path.to_string());
    }
    if let Some(branch) = branch.as_deref() {
        session_args["branch"] = serde_json::Value::String(branch.to_string());
    }
    if let Some(base) = base.as_deref() {
        session_args["base"] = serde_json::Value::String(base.to_string());
    }
    if let Some(fetch_first) = fetch_first {
        session_args["fetchFirst"] = serde_json::Value::Bool(fetch_first);
    }

    let session_create_req = Request {
        command: "session-create-shell".to_string(),
        session_id: None,
        pane_id: None,
        auth_token: req.auth_token.clone(),
        args: session_args,
    };
    let session_resp = handle_session_create_shell(session_create_req, host, identity).await;
    if !session_resp.ok {
        return Err(session_resp);
    }

    let session_id = session_resp
        .data
        .as_ref()
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let Some(session_id) = session_id else {
        return Err(Response::err("planning session created but id missing from response"));
    };
    let session = match host.session_handle.get(&session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Err(Response::err("planning session created but not found")),
        Err(err) => return Err(Response::err(err.to_string())),
    };

    let provider = provider_slug(profile.provider).map(str::to_string);
    let worktree_path = Some(session.worktree_path.as_str());
    let branch = (!session.branch.trim().is_empty()).then_some(session.branch.as_str());
    let run = match host.work_item_handle.create_planning_run(
        &item_id,
        Some(&session_id),
        provider.as_deref(),
        Some(&profile_id),
        worktree_path,
        branch,
    ) {
        Ok(run) => run,
        Err(err) => {
            return Err(Response::err(format!(
                "work item planning run create failed; session was preserved: {err}"
            )));
        }
    };
    let item = persist_work_item_planning_target(
        host,
        &item,
        &repo_path,
        &profile_id,
        base_branch.as_deref(),
        worktree_path,
        branch,
        fetch_first,
    )?;

    let _ = host.work_item_handle.append_run_event(
        &run.id,
        roux_core::WorkItemRunEventKind::Lifecycle,
        serde_json::json!({
            "stage": "sessionCreated",
            "sessionId": session_id.clone(),
            "worktreePath": worktree_path,
            "branch": branch,
        }),
    );
    let mut dispatch_item = item.clone();
    dispatch_item.repo_path = Some(repo_path);
    dispatch_item.agent_profile = Some(profile_id.clone());
    let pty_id = match dispatch_profile(
        host,
        &dispatch_item,
        &run.id,
        &session_id,
        &profile_id,
        identity,
        false,
    )
    .await
    {
        Ok(pty_id) => pty_id,
        Err(err) => {
            let _ = host.work_item_handle.set_run_status(
                &run.id,
                roux_core::WorkItemRunStatus::Failed,
                serde_json::json!({
                    "reason": "promptDispatchFailed",
                    "message": err.clone(),
                    "sessionId": session_id.clone(),
                }),
            );
            return Err(Response::err(err));
        }
    };
    let run = persist_work_item_run_pty_id(host, &run.id, &pty_id)?;
    start_work_item_run_output_monitor(host.clone(), run.id.clone(), pty_id);

    let _ = host.work_item_handle.append_run_event(
        &run.id,
        roux_core::WorkItemRunEventKind::Lifecycle,
        serde_json::json!({
            "stage": "promptDispatched",
            "sessionId": session_id.clone(),
        }),
    );
    let run = match host.work_item_handle.set_run_status(
        &run.id,
        roux_core::WorkItemRunStatus::Running,
        serde_json::json!({
            "reason": "promptDispatched",
            "sessionId": session_id.clone(),
        }),
    ) {
        Ok(Some(run)) => run,
        Ok(None) => host.work_item_handle.get_run(&run.id).ok().flatten().unwrap_or(run),
        Err(_) => run,
    };

    Ok(roux_core::WorkItemPlanResult { item, session, run })
}

#[allow(clippy::too_many_arguments)]
fn persist_work_item_planning_target(
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
    repo_path: &str,
    profile_id: &str,
    base_branch: Option<&str>,
    worktree_path: Option<&str>,
    branch: Option<&str>,
    fetch_first: Option<bool>,
) -> Result<roux_core::WorkItem, Response> {
    let input = roux_core::WorkItemInput {
        title: item.title.clone(),
        body: item.body.clone(),
        status: Some(roux_core::WorkItemStatus::Ready),
        repo_path: Some(repo_path.to_string()),
        agent_profile: Some(profile_id.to_string()),
        base_branch: base_branch.map(str::to_string),
        worktree_path: worktree_path.map(str::to_string),
        branch: branch.map(str::to_string),
        fetch_first,
        start_error: None,
        project_id: item.project_id.clone(),
        parent_id: item.parent_id.clone(),
        external_ref: None,
        sort_order: Some(item.sort_order),
        field_presence: roux_core::WorkItemInputPresence {
            start_error: true,
            ..roux_core::WorkItemInputPresence::default()
        },
    };
    match host.work_item_handle.update(&item.id, input) {
        Ok(Some(item)) => Ok(item),
        Ok(None) => Err(Response::err("work item not found after planning session creation")),
        Err(err) => Err(Response::err(err)),
    }
}

async fn stop_planning_run_for_replacement(
    host: &RuntimeHost,
    run: &roux_core::WorkItemRun,
) -> Result<(), Response> {
    let stopped_run = match host.work_item_handle.set_run_status(
        &run.id,
        roux_core::WorkItemRunStatus::Stopped,
        serde_json::json!({
            "reason": "replan",
            "sessionId": run.session_id.clone(),
        }),
    ) {
        Ok(Some(run)) => run,
        Ok(None) => match host.work_item_handle.get_run(&run.id) {
            Ok(Some(run)) if run.status == roux_core::WorkItemRunStatus::Stopped => run,
            Ok(Some(_)) => return Err(Response::err("active planning run status was not updated")),
            Ok(None) => return Err(Response::err("active planning run not found")),
            Err(err) => return Err(Response::err(err)),
        },
        Err(err) => return Err(Response::err(err)),
    };
    cleanup_stopped_work_item_run_session(host, &stopped_run).await?;
    Ok(())
}

fn active_work_item_run(
    host: &RuntimeHost,
    item_id: &str,
) -> Result<Option<roux_core::WorkItemRun>, Response> {
    let runs = host.work_item_handle.list_runs(Some(item_id)).map_err(Response::err)?;
    Ok(runs.into_iter().find(is_active_work_item_run))
}

async fn resolve_planning_repo_path(
    req: &Request,
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
) -> Result<String, Response> {
    if let Some(path) = optional_string_arg(&req.args, &["repoPath", "repo_path"]) {
        return Ok(path);
    }
    if let Some(path) = item.repo_path.clone() {
        return Ok(path);
    }
    if let Some(pid) = &item.project_id {
        match host.project_handle.get(pid).await {
            Ok(Some(project)) => {
                if let Some(root) = project.repo_roots.into_iter().next() {
                    return Ok(root);
                }
            }
            Ok(None) => return Err(Response::err("project not found")),
            Err(err) => return Err(Response::err(err.to_string())),
        }
    }
    std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|err| Response::err(format!("failed to resolve planning directory: {err}")))
}

fn noop_after_session_created(_: &RuntimeHost, _: &str, _: &roux_core::Session) {}

async fn start_work_item_run(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Result<roux_core::WorkItemStartResult, Response> {
    start_work_item_run_with_hooks(
        req,
        host,
        identity,
        real_profile_dispatcher,
        noop_after_session_created,
    )
    .await
}

async fn start_work_item_run_with_hooks(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
    dispatch_profile: ProfileDispatcher,
    after_session_created: AfterSessionCreatedHook,
) -> Result<roux_core::WorkItemStartResult, Response> {
    let Some(item_id) = optional_string_arg(&req.args, &["id"]) else {
        return Err(Response::err("id required"));
    };
    let mut item = match host.work_item_handle.get(&item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return Err(Response::err("work item not found")),
        Err(err) => return Err(Response::err(err)),
    };
    let force_start = bool_arg(&req.args, &["forceStart", "force_start"]).unwrap_or(false);

    if item.session_id.is_none() {
        match host.work_item_handle.has_active_run(&item_id) {
            Ok(true) => {
                return Err(record_start_failure_response(
                    host,
                    &item_id,
                    "work item already has an active run",
                    None,
                    None,
                    None,
                    None,
                    None,
                ));
            }
            Ok(false) => {}
            Err(err) => return Err(Response::err(err)),
        }
    }

    let settings = load_daemon_settings();
    let profile_id = optional_string_arg(&req.args, &["profile", "agentProfile", "agent_profile"])
        .or_else(|| item.agent_profile.clone())
        .unwrap_or_else(|| settings.kanban.default_agent_profile.clone());
    let Some(profile) = roux_core::providers::resolve_profile(&profile_id, &settings) else {
        return Err(record_start_failure_response(
            host,
            &item_id,
            &format!("agent profile not found: {profile_id}"),
            None,
            None,
            Some(&profile_id),
            None,
            None,
        ));
    };
    if let Some(reason) = autonomous_profile_rejection_reason(&profile) {
        return Err(record_start_failure_response(
            host,
            &item_id,
            reason,
            None,
            None,
            Some(&profile_id),
            None,
            None,
        ));
    }

    // Resolve repo_path: explicit arg -> card repo_path -> project.repo_roots[0] -> error.
    let repo_path = if let Some(path) = optional_string_arg(&req.args, &["repoPath", "repo_path"]) {
        path
    } else if let Some(path) = item.repo_path.clone() {
        path
    } else if let Some(pid) = &item.project_id {
        match host.project_handle.get(pid).await {
            Ok(Some(project)) => match project.repo_roots.into_iter().next() {
                Some(root) => root,
                None => {
                    return Err(record_start_failure_response(
                        host,
                        &item_id,
                        "project has no repo_roots",
                        None,
                        None,
                        Some(&profile_id),
                        None,
                        None,
                    ));
                }
            },
            Ok(None) => {
                return Err(record_start_failure_response(
                    host,
                    &item_id,
                    "project not found",
                    None,
                    None,
                    Some(&profile_id),
                    None,
                    None,
                ));
            }
            Err(err) => return Err(Response::err(err.to_string())),
        }
    } else {
        return Err(record_start_failure_response(
            host,
            &item_id,
            "repoPath or project required",
            None,
            None,
            Some(&profile_id),
            None,
            None,
        ));
    };

    let name = optional_string_arg(&req.args, &["name"]).unwrap_or_else(|| item.title.clone());
    let explicit_worktree_path =
        optional_nullable_string_arg(&req.args, &["worktreePath", "worktree_path"]);
    let requested_branch =
        optional_nullable_string_arg(&req.args, &["branch", "worktreeBranch", "worktree_branch"])
            .or_else(|| item.branch.clone());
    let base = optional_nullable_string_arg(&req.args, &["base", "startPoint", "start_point"])
        .or_else(|| item.base_branch.clone())
        .or_else(|| Some("main".to_string()));
    let fetch_first = bool_arg(&req.args, &["fetchFirst", "fetch_first"]).or(item.fetch_first);
    let worktree_path = explicit_worktree_path.or_else(|| {
        if requested_branch.is_none() {
            item.worktree_path.clone()
        } else {
            None
        }
    });
    let branch = if worktree_path.is_some() {
        requested_branch
    } else {
        requested_branch.or_else(|| Some(default_work_item_branch(&item)))
    };
    let base_branch = base.clone();

    enforce_work_item_start_plan_gate(
        host,
        &item,
        force_start,
        Some(&profile_id),
        Some(&repo_path),
        base_branch.as_deref(),
    )?;

    if let Some(session_id) = item.session_id.clone() {
        let session = match host.session_handle.get(&session_id).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                return Err(record_start_failure_response(
                    host,
                    &item_id,
                    "bound session not found",
                    Some(&session_id),
                    item.worktree_path.as_deref(),
                    Some(&profile_id),
                    Some(&repo_path),
                    base_branch.as_deref(),
                ));
            }
            Err(err) => return Err(Response::err(err.to_string())),
        };
        let provider = provider_slug(profile.provider).map(str::to_string);
        let worktree_path = Some(session.worktree_path.as_str());
        let branch = (!session.branch.trim().is_empty()).then_some(session.branch.as_str());
        let run = match host.work_item_handle.create_starting_run(
            &item_id,
            Some(&session_id),
            provider.as_deref(),
            Some(&profile_id),
            worktree_path,
            branch,
        ) {
            Ok(run) => run,
            Err(err) => {
                return Err(Response::err(format!(
                    "work item run create failed; bound session was preserved: {err}"
                )));
            }
        };
        let _ = host.work_item_handle.append_run_event(
            &run.id,
            roux_core::WorkItemRunEventKind::Lifecycle,
            serde_json::json!({
                "stage": "sessionReused",
                "sessionId": session_id.clone(),
                "worktreePath": worktree_path,
                "branch": branch,
            }),
        );
        let mut dispatch_item = item.clone();
        dispatch_item.worktree_path = worktree_path.map(str::to_string);
        dispatch_item.agent_profile = Some(profile_id.clone());
        dispatch_item.repo_path = Some(repo_path.clone());
        dispatch_item.base_branch = base_branch.clone();

        let pty_id = match dispatch_profile(
            host,
            &dispatch_item,
            &run.id,
            &session_id,
            &profile_id,
            identity,
            force_start,
        )
        .await
        {
            Ok(pty_id) => pty_id,
            Err(err) => {
                let _ = host.work_item_handle.set_run_status(
                    &run.id,
                    roux_core::WorkItemRunStatus::Failed,
                    serde_json::json!({
                        "reason": "promptDispatchFailed",
                        "message": err.clone(),
                        "sessionId": session_id.clone(),
                    }),
                );
                let _ = host.work_item_handle.record_start_failure(
                    &item_id,
                    &err,
                    Some(&session_id),
                    worktree_path,
                    Some(&profile_id),
                    Some(&repo_path),
                    base_branch.as_deref(),
                );
                return Err(Response::err(err));
            }
        };
        let run = persist_work_item_run_pty_id(host, &run.id, &pty_id)?;
        start_work_item_run_output_monitor(host.clone(), run.id.clone(), pty_id);

        let _ = host.work_item_handle.append_run_event(
            &run.id,
            roux_core::WorkItemRunEventKind::Lifecycle,
            serde_json::json!({
                "stage": "promptDispatched",
                "sessionId": session_id.clone(),
            }),
        );
        let run = match host.work_item_handle.set_run_status(
            &run.id,
            roux_core::WorkItemRunStatus::Running,
            serde_json::json!({
                "reason": "promptDispatched",
                "sessionId": session_id.clone(),
            }),
        ) {
            Ok(Some(run)) => run,
            Ok(None) => host.work_item_handle.get_run(&run.id).ok().flatten().unwrap_or(run),
            Err(_) => run,
        };
        item = match host.work_item_handle.complete_start(
            &item_id,
            &session_id,
            worktree_path,
            Some(&profile_id),
            Some(&repo_path),
            base_branch.as_deref(),
            item.sort_order,
        ) {
            Ok(Some(item)) => item,
            Ok(None) => return Err(Response::err("work item not found after prompt dispatch")),
            Err(err) => return Err(Response::err(err)),
        };

        return Ok(roux_core::WorkItemStartResult { item, session, run });
    }

    let mut session_args = serde_json::json!({
        "repoPath": repo_path.clone(),
        "name": name,
        "projectId": item.project_id.clone(),
        "profile": profile_id.clone(),
    });
    if let Some(worktree_path) = worktree_path {
        session_args["worktreePath"] = serde_json::Value::String(worktree_path);
    }
    if let Some(branch) = branch {
        session_args["branch"] = serde_json::Value::String(branch);
    }
    if let Some(base) = base {
        session_args["base"] = serde_json::Value::String(base);
    }
    if let Some(fetch_first) = fetch_first {
        session_args["fetchFirst"] = serde_json::Value::Bool(fetch_first);
    }

    let session_create_req = Request {
        command: "session-create-shell".to_string(),
        session_id: None,
        pane_id: None,
        auth_token: req.auth_token.clone(),
        args: session_args,
    };
    let session_resp = handle_session_create_shell(session_create_req, host, identity).await;
    if !session_resp.ok {
        let message =
            session_resp.error.clone().unwrap_or_else(|| "session creation failed".to_string());
        let _ = host.work_item_handle.record_start_failure(
            &item_id,
            &message,
            None,
            None,
            Some(&profile_id),
            Some(&repo_path),
            base_branch.as_deref(),
        );
        return Err(session_resp);
    }

    let session_id = session_resp
        .data
        .as_ref()
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let Some(session_id) = session_id else {
        return Err(record_start_failure_response(
            host,
            &item_id,
            "session created but id missing from response",
            None,
            None,
            Some(&profile_id),
            Some(&repo_path),
            base_branch.as_deref(),
        ));
    };

    let session = match host.session_handle.get(&session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            return Err(record_start_failure_response(
                host,
                &item_id,
                "session created but not found in daemon state",
                Some(&session_id),
                None,
                Some(&profile_id),
                Some(&repo_path),
                base_branch.as_deref(),
            ));
        }
        Err(err) => return Err(Response::err(err.to_string())),
    };
    after_session_created(host, &item_id, &session);
    let provider = provider_slug(profile.provider).map(str::to_string);
    let worktree_path = Some(session.worktree_path.as_str());
    let branch = (!session.branch.trim().is_empty()).then_some(session.branch.as_str());
    let run = match host.work_item_handle.create_starting_run(
        &item_id,
        Some(&session_id),
        provider.as_deref(),
        Some(&profile_id),
        worktree_path,
        branch,
    ) {
        Ok(run) => run,
        Err(err) => {
            if !err.contains("active work item run already exists") {
                let _ = host.work_item_handle.record_start_failure(
                    &item_id,
                    &err,
                    Some(&session_id),
                    worktree_path,
                    Some(&profile_id),
                    Some(&repo_path),
                    base_branch.as_deref(),
                );
            }
            return Err(Response::err(format!(
                "work item run create failed; session was preserved: {err}"
            )));
        }
    };

    let _ = host.work_item_handle.append_run_event(
        &run.id,
        roux_core::WorkItemRunEventKind::Lifecycle,
        serde_json::json!({
            "stage": "sessionCreated",
            "sessionId": session_id.clone(),
            "worktreePath": worktree_path,
            "branch": branch,
        }),
    );
    let mut dispatch_item = item.clone();
    dispatch_item.session_id = Some(session_id.clone());
    dispatch_item.worktree_path = worktree_path.map(str::to_string);
    dispatch_item.agent_profile = Some(profile_id.clone());
    dispatch_item.repo_path = Some(repo_path.clone());
    dispatch_item.base_branch = base_branch.clone();

    let pty_id = match dispatch_profile(
        host,
        &dispatch_item,
        &run.id,
        &session_id,
        &profile_id,
        identity,
        force_start,
    )
    .await
    {
        Ok(pty_id) => pty_id,
        Err(err) => {
            let _ = host.work_item_handle.set_run_status(
                &run.id,
                roux_core::WorkItemRunStatus::Failed,
                serde_json::json!({
                    "reason": "promptDispatchFailed",
                    "message": err.clone(),
                    "sessionId": session_id.clone(),
                }),
            );
            let _ = host.work_item_handle.record_start_failure(
                &item_id,
                &err,
                Some(&session_id),
                worktree_path,
                Some(&profile_id),
                Some(&repo_path),
                base_branch.as_deref(),
            );
            return Err(Response::err(err));
        }
    };
    let run = persist_work_item_run_pty_id(host, &run.id, &pty_id)?;
    start_work_item_run_output_monitor(host.clone(), run.id.clone(), pty_id);

    let _ = host.work_item_handle.append_run_event(
        &run.id,
        roux_core::WorkItemRunEventKind::Lifecycle,
        serde_json::json!({
            "stage": "promptDispatched",
            "sessionId": session_id.clone(),
        }),
    );
    let run = match host.work_item_handle.set_run_status(
        &run.id,
        roux_core::WorkItemRunStatus::Running,
        serde_json::json!({
            "reason": "promptDispatched",
            "sessionId": session_id.clone(),
        }),
    ) {
        Ok(Some(run)) => run,
        Ok(None) => host.work_item_handle.get_run(&run.id).ok().flatten().unwrap_or(run),
        Err(_) => run,
    };
    item = match host.work_item_handle.complete_start(
        &item_id,
        &session_id,
        worktree_path,
        Some(&profile_id),
        Some(&repo_path),
        base_branch.as_deref(),
        item.sort_order,
    ) {
        Ok(Some(item)) => item,
        Ok(None) => return Err(Response::err("work item not found after prompt dispatch")),
        Err(err) => return Err(Response::err(err)),
    };

    Ok(roux_core::WorkItemStartResult { item, session, run })
}

fn enforce_work_item_start_plan_gate(
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
    force_start: bool,
    agent_profile: Option<&str>,
    repo_path: Option<&str>,
    base_branch: Option<&str>,
) -> Result<(), Response> {
    if force_start {
        return Ok(());
    }
    let attachments = host
        .work_item_handle
        .list_attachments(Some(roux_core::AttachmentTargetKind::WorkItem), Some(&item.id))
        .map_err(Response::err)?;
    if select_latest_work_item_plan_attachment(attachments.iter()).is_some() {
        return Ok(());
    }
    Err(record_start_failure_response(
        host,
        &item.id,
        "work item requires an attached implementation plan before start; use forceStart to override",
        item.session_id.as_deref(),
        item.worktree_path.as_deref(),
        agent_profile,
        repo_path,
        base_branch,
    ))
}

fn record_start_failure_response(
    host: &RuntimeHost,
    item_id: &str,
    message: &str,
    session_id: Option<&str>,
    worktree_path: Option<&str>,
    agent_profile: Option<&str>,
    repo_path: Option<&str>,
    base_branch: Option<&str>,
) -> Response {
    let _ = host.work_item_handle.record_start_failure(
        item_id,
        message,
        session_id,
        worktree_path,
        agent_profile,
        repo_path,
        base_branch,
    );
    Response::err(message.to_string())
}

fn work_item_run_pty_id(replace_primary: bool, session_id: &str, run_id: &str) -> String {
    if replace_primary {
        session_id.to_string()
    } else {
        format!("{session_id}-{run_id}")
    }
}

fn persist_work_item_run_pty_id(
    host: &RuntimeHost,
    run_id: &str,
    pty_id: &str,
) -> Result<roux_core::WorkItemRun, Response> {
    match host.work_item_handle.set_run_pty_id(run_id, Some(pty_id)) {
        Ok(Some(run)) => Ok(run),
        Ok(None) => Err(Response::err("work item run not found after prompt dispatch")),
        Err(err) => Err(Response::err(err)),
    }
}

fn work_item_run_replaces_primary(host: &RuntimeHost, item_id: &str) -> bool {
    let Ok(runs) = host.work_item_handle.list_runs(Some(item_id)) else {
        return false;
    };
    let active_implementation_runs = runs
        .iter()
        .filter(|run| run.kind == roux_core::WorkItemRunKind::Implementation)
        .filter(|run| is_active_work_item_run(run))
        .count();
    active_implementation_runs <= 1
}

fn autonomous_profile_rejection_reason(profile: &roux_core::SpawnProfile) -> Option<&'static str> {
    if !matches!(profile.provider, Some(roux_core::Provider::Claude | roux_core::Provider::Codex)) {
        return Some("agentProfile must be an autonomous Claude or Codex profile");
    }
    if matches!(profile.startup_behavior, Some(roux_core::StartupBehavior::TypeOnly)) {
        return Some("agentProfile must auto-run instead of type-only");
    }
    if profile.startup_command.as_deref().map(str::trim).filter(|cmd| !cmd.is_empty()).is_none() {
        return Some("agentProfile must define a startup command");
    }
    None
}

fn default_work_item_branch(item: &roux_core::WorkItem) -> String {
    let short_id: String =
        item.id.chars().filter(|ch| ch.is_ascii_alphanumeric()).take(8).collect();
    let short_id = if short_id.is_empty() { "item".to_string() } else { short_id };
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in item.title.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
        if slug.len() >= 48 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        format!("roux/card-{short_id}")
    } else {
        format!("roux/card-{short_id}-{slug}")
    }
}

fn provider_slug(provider: Option<roux_core::Provider>) -> Option<&'static str> {
    match provider {
        Some(roux_core::Provider::Claude) => Some("claude"),
        Some(roux_core::Provider::Codex) => Some("codex"),
        None => None,
    }
}

pub(super) async fn handle_work_item_runs_list(req: Request, host: &RuntimeHost) -> Response {
    let work_item_id = optional_string_arg(&req.args, &["workItemId", "work_item_id"]);
    match host.work_item_handle.list_runs(work_item_id.as_deref()) {
        Ok(runs) => match serde_json::to_value(runs) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item runs: {err}")),
        },
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_run_events(req: Request, host: &RuntimeHost) -> Response {
    let Some(run_id) = optional_string_arg(&req.args, &["runId", "run_id"]) else {
        return Response::err("runId required");
    };
    match host.work_item_handle.list_run_events(&run_id) {
        Ok(events) => match serde_json::to_value(events) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item run events: {err}")),
        },
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_run_stop(req: Request, host: &RuntimeHost) -> Response {
    let Some(run_id) = optional_string_arg(&req.args, &["id", "runId", "run_id"]) else {
        return Response::err("runId required");
    };
    let run = match host.work_item_handle.get_run(&run_id) {
        Ok(Some(run)) => run,
        Ok(None) => return Response::err("work item run not found"),
        Err(err) => return Response::err(err),
    };
    if matches!(
        run.status,
        roux_core::WorkItemRunStatus::Stopped
            | roux_core::WorkItemRunStatus::ChangesRequested
            | roux_core::WorkItemRunStatus::Done
            | roux_core::WorkItemRunStatus::Failed
    ) {
        return terminal_work_item_run_stop_response(host, run).await;
    }

    let stopped_run = match host.work_item_handle.set_run_status(
        &run_id,
        roux_core::WorkItemRunStatus::Stopped,
        serde_json::json!({
            "reason": "user",
            "sessionId": run.session_id.clone(),
        }),
    ) {
        Ok(Some(run)) => run,
        Ok(None) => match host.work_item_handle.get_run(&run_id) {
            Ok(Some(run))
                if matches!(
                    run.status,
                    roux_core::WorkItemRunStatus::Stopped
                        | roux_core::WorkItemRunStatus::ChangesRequested
                        | roux_core::WorkItemRunStatus::Done
                        | roux_core::WorkItemRunStatus::Failed
                ) =>
            {
                return terminal_work_item_run_stop_response(host, run).await;
            }
            Ok(Some(_)) => return Response::err("work item run status was not updated"),
            Ok(None) => return Response::err("work item run not found"),
            Err(err) => return Response::err(err),
        },
        Err(err) => return Response::err(err),
    };

    if let Err(response) = cleanup_stopped_work_item_run_session(host, &stopped_run).await {
        return response;
    }

    match serde_json::to_value(stopped_run) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize work item run: {err}")),
    }
}

async fn terminal_work_item_run_stop_response(
    host: &RuntimeHost,
    run: roux_core::WorkItemRun,
) -> Response {
    if run.status == roux_core::WorkItemRunStatus::Stopped {
        if let Err(response) = cleanup_stopped_work_item_run_session(host, &run).await {
            return response;
        }
    }
    match serde_json::to_value(run) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize work item run: {err}")),
    }
}

async fn cleanup_stopped_work_item_run_session(
    host: &RuntimeHost,
    run: &roux_core::WorkItemRun,
) -> Result<(), Response> {
    let Some(session_id) = run.session_id.as_deref() else {
        return Ok(());
    };
    if has_active_work_item_run_for_session(host, session_id)? {
        if let Some(pty_id) = run.pty_id.as_deref() {
            let _ = host.pty_handle.remove(pty_id).await;
        }
        return Ok(());
    }
    kill_session_ptys(host, session_id).await;
    host.session_handle.archive(session_id).await.map_err(|err| Response::err(err.to_string()))
}

fn has_active_work_item_run_for_session(
    host: &RuntimeHost,
    session_id: &str,
) -> Result<bool, Response> {
    let runs = host.work_item_handle.list_runs(None).map_err(Response::err)?;
    Ok(runs
        .iter()
        .any(|run| run.session_id.as_deref() == Some(session_id) && is_active_work_item_run(run)))
}

fn is_active_work_item_run(run: &roux_core::WorkItemRun) -> bool {
    !is_terminal_work_item_run_status(&run.status)
}

fn is_terminal_work_item_run_status(status: &roux_core::WorkItemRunStatus) -> bool {
    matches!(
        status,
        roux_core::WorkItemRunStatus::Done
            | roux_core::WorkItemRunStatus::ChangesRequested
            | roux_core::WorkItemRunStatus::Failed
            | roux_core::WorkItemRunStatus::Stopped
    )
}

pub(super) async fn handle_work_item_decision_create(req: Request, host: &RuntimeHost) -> Response {
    let Some(run_id) = optional_string_arg(&req.args, &["runId", "run_id"]) else {
        return Response::err("runId required");
    };
    let Some(question) = optional_string_arg(&req.args, &["question"]) else {
        return Response::err("question required");
    };
    let options = match parse_decision_options(req.args.get("options")) {
        Ok(options) => options,
        Err(err) => return Response::err(err),
    };
    let default_value = optional_string_arg(&req.args, &["defaultValue", "default_value"]);
    let timeout_at = match parse_decision_timeout_at(&req.args, default_value.as_deref()) {
        Ok(timeout_at) => timeout_at,
        Err(err) => return Response::err(err),
    };
    match host.work_item_handle.create_decision(
        &run_id,
        &question,
        options,
        default_value.as_deref(),
        timeout_at,
    ) {
        Ok(decision) => {
            handle_created_or_timed_out_decision(host, &decision).await;
            match serde_json::to_value(decision) {
                Ok(value) => Response::success(value),
                Err(err) => Response::err(format!("failed to serialize work item decision: {err}")),
            }
        }
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_decisions_list(req: Request, host: &RuntimeHost) -> Response {
    let work_item_id = optional_string_arg(&req.args, &["workItemId", "work_item_id"]);
    if let Err(err) = expire_due_work_item_decisions(host).await {
        return Response::err(err);
    }
    match host.work_item_handle.list_pending_decisions(work_item_id.as_deref()) {
        Ok(decisions) => match serde_json::to_value(decisions) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item decisions: {err}")),
        },
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_work_item_decision_resolve(
    req: Request,
    host: &RuntimeHost,
) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id"]) else {
        return Response::err("id required");
    };
    let Some(value) = optional_string_arg(&req.args, &["value"]) else {
        return Response::err("value required");
    };
    let resolved_by = optional_string_arg(&req.args, &["resolvedBy", "resolved_by"]);
    match host.work_item_handle.resolve_decision(&id, &value, resolved_by.as_deref()) {
        Ok(Some(decision)) => {
            write_resolved_decision_to_run(host, &decision, &value).await;
            match serde_json::to_value(decision) {
                Ok(value) => Response::success(value),
                Err(err) => Response::err(format!("failed to serialize work item decision: {err}")),
            }
        }
        Ok(None) => Response::err("decision not found"),
        Err(err) => Response::err(err),
    }
}

async fn write_resolved_decision_to_run(
    host: &RuntimeHost,
    decision: &roux_core::WorkItemDecision,
    value: &str,
) {
    let run = match host.work_item_handle.get_run(&decision.run_id) {
        Ok(Some(run)) => run,
        Ok(None) => return,
        Err(_) => return,
    };
    let Some(session_id) = run.session_id.as_deref() else {
        return;
    };
    let target_pty_id = run.pty_id.as_deref().unwrap_or(session_id);

    let input = format!("{value}\n");
    if let Err(err) = host.pty_handle.write(target_pty_id, input.into_bytes()).await {
        let _ = host.work_item_handle.append_run_event(
            &run.id,
            roux_core::WorkItemRunEventKind::Error,
            serde_json::json!({
                "decisionId": decision.id,
                "message": format!("failed to write resolved decision to pty: {err}"),
                "sessionId": session_id,
                "ptyId": target_pty_id,
                "stage": "decisionResolutionWrite"
            }),
        );
    }
}

fn parse_decision_options(
    value: Option<&serde_json::Value>,
) -> Result<Vec<roux_core::WorkItemDecisionOption>, String> {
    let Some(value) = value else {
        return Err("options required".into());
    };
    serde_json::from_value(value.clone()).map_err(|err| format!("invalid options: {err}"))
}

fn parse_decision_timeout_at(
    args: &serde_json::Value,
    default_value: Option<&str>,
) -> Result<Option<u64>, String> {
    let timeout_at =
        optional_u64_arg(args, &["timeoutAt", "timeout_at", "expiresAt", "expires_at"])?;
    let timeout_ms = optional_u64_arg(args, &["timeoutMs", "timeout_ms"])?;
    let timeout_seconds = optional_u64_arg(args, &["timeoutSeconds", "timeout_seconds"])?;
    let timeout_at = timeout_at
        .or_else(|| {
            timeout_ms.map(|ms| unix_now_secs().saturating_add(ms.saturating_add(999) / 1000))
        })
        .or_else(|| timeout_seconds.map(|seconds| unix_now_secs().saturating_add(seconds)));
    if timeout_at.is_some() && default_value.is_none() {
        return Err("defaultValue required when timeout is set".into());
    }
    Ok(timeout_at)
}

fn optional_u64_arg(args: &serde_json::Value, keys: &[&str]) -> Result<Option<u64>, String> {
    for key in keys {
        let Some(value) = args.get(*key) else {
            continue;
        };
        if value.is_null() {
            return Ok(None);
        }
        if let Some(value) = value.as_u64() {
            return Ok(Some(value));
        }
        if let Some(value) = value.as_str().map(str::trim).filter(|value| !value.is_empty()) {
            return value
                .parse::<u64>()
                .map(Some)
                .map_err(|_| format!("{key} must be an unsigned integer"));
        }
        return Err(format!("{key} must be an unsigned integer"));
    }
    Ok(None)
}

fn start_work_item_run_output_monitor(host: RuntimeHost, run_id: String, pty_id: String) {
    tokio::spawn(async move {
        let mut attach = match host.pty_handle.attach(&pty_id, PTY_OUTPUT_DEFAULT_POLL_BYTES).await
        {
            Ok(Some(attach)) => attach,
            Ok(None) => {
                let _ = host.work_item_handle.append_run_event(
                    &run_id,
                    roux_core::WorkItemRunEventKind::Error,
                    serde_json::json!({
                        "message": "failed to monitor work item run output: daemon pty not found",
                        "ptyId": pty_id,
                        "stage": "runOutputMonitorAttach"
                    }),
                );
                return;
            }
            Err(err) => {
                let _ = host.work_item_handle.append_run_event(
                    &run_id,
                    roux_core::WorkItemRunEventKind::Error,
                    serde_json::json!({
                        "message": format!("failed to monitor work item run output: {err}"),
                        "ptyId": pty_id,
                        "stage": "runOutputMonitorAttach"
                    }),
                );
                return;
            }
        };

        let mut parser = WorkItemRunOutputParser::default();
        let mut observed_offset = attach.replay_offset;
        if !attach.replay_bytes.is_empty() {
            ingest_work_item_run_output(
                &host,
                &run_id,
                attach.replay_offset,
                &attach.replay_bytes,
                &mut parser,
            );
            observed_offset = attach.replay_offset.saturating_add(attach.replay_bytes.len() as u64);
        }
        if !attach.record.running {
            record_work_item_run_pty_exit(
                &host,
                &run_id,
                attach.record.exit_code,
                attach.record.generation,
            );
            return;
        }

        loop {
            match attach.events.recv().await {
                Ok(PtyOutputEvent::Output(frame)) => {
                    let frame_end = frame.offset.saturating_add(frame.bytes.len() as u64);
                    if frame_end <= observed_offset {
                        continue;
                    }
                    let (offset, bytes) = if frame.offset < observed_offset {
                        let skip = (observed_offset - frame.offset) as usize;
                        (observed_offset, &frame.bytes[skip..])
                    } else {
                        (frame.offset, frame.bytes.as_slice())
                    };
                    ingest_work_item_run_output(&host, &run_id, offset, bytes, &mut parser);
                    observed_offset = frame_end;
                }
                Ok(PtyOutputEvent::Exit { code, generation }) => {
                    record_work_item_run_pty_exit(&host, &run_id, code, generation);
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let _ = host.work_item_handle.append_run_event(
                        &run_id,
                        roux_core::WorkItemRunEventKind::Error,
                        serde_json::json!({
                            "message": format!("run output monitor lagged by {skipped} frame(s)"),
                            "stage": "runOutputMonitor"
                        }),
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn record_work_item_run_pty_exit(
    host: &RuntimeHost,
    run_id: &str,
    code: Option<i32>,
    generation: u64,
) {
    match host.work_item_handle.get_run(run_id) {
        Ok(Some(run))
            if matches!(
                run.status,
                roux_core::WorkItemRunStatus::Stopped
                    | roux_core::WorkItemRunStatus::Review
                    | roux_core::WorkItemRunStatus::ChangesRequested
                    | roux_core::WorkItemRunStatus::Done
                    | roux_core::WorkItemRunStatus::Failed
            ) => {}
        Ok(Some(run)) => {
            let review_requested =
                code == Some(0) && run.kind == roux_core::WorkItemRunKind::Implementation;
            let status = match code {
                Some(0) if review_requested => roux_core::WorkItemRunStatus::Review,
                Some(0) => roux_core::WorkItemRunStatus::Done,
                _ => roux_core::WorkItemRunStatus::Failed,
            };
            let payload = serde_json::json!({
                "reason": "ptyExit",
                "exitCode": code,
                "generation": generation,
                "reviewRequested": review_requested,
            });
            if host.work_item_handle.set_run_status(run_id, status, payload).is_ok()
                && review_requested
            {
                if let Ok(Some(item)) = host.work_item_handle.get(&run.work_item_id) {
                    let _ = host.work_item_handle.move_item(
                        &run.work_item_id,
                        roux_core::WorkItemStatus::Review,
                        item.sort_order,
                    );
                }
            }
        }
        Ok(None) => {}
        Err(_) => {
            let _ = host.work_item_handle.append_run_event(
                run_id,
                roux_core::WorkItemRunEventKind::Error,
                serde_json::json!({
                    "message": "failed to load run while recording PTY exit",
                    "stage": "runOutputMonitorExit"
                }),
            );
        }
    }
}

fn ingest_work_item_run_output(
    host: &RuntimeHost,
    run_id: &str,
    offset: u64,
    bytes: &[u8],
    parser: &mut WorkItemRunOutputParser,
) {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let _ = host.work_item_handle.append_run_event(
        run_id,
        roux_core::WorkItemRunEventKind::Text,
        serde_json::json!({
            "offset": offset,
            "text": text,
        }),
    );

    for decision in parser.ingest(&text) {
        let Ok(decision) = host.work_item_handle.create_decision(
            run_id,
            &decision.question,
            decision.options,
            decision.default_value.as_deref(),
            decision.timeout_at,
        ) else {
            continue;
        };
        tokio::spawn({
            let host = host.clone();
            async move {
                handle_created_or_timed_out_decision(&host, &decision).await;
            }
        });
    }
}

async fn handle_created_or_timed_out_decision(
    host: &RuntimeHost,
    decision: &roux_core::WorkItemDecision,
) {
    match decision.status {
        roux_core::WorkItemDecisionStatus::Pending => {
            schedule_work_item_decision_timeout(host.clone(), decision);
        }
        roux_core::WorkItemDecisionStatus::TimedOut => {
            if let Some(value) = decision.resolved_value.clone() {
                write_resolved_decision_to_run(host, decision, &value).await;
            }
        }
        roux_core::WorkItemDecisionStatus::Resolved => {}
    }
}

async fn expire_due_work_item_decisions(
    host: &RuntimeHost,
) -> Result<Vec<roux_core::WorkItemDecision>, String> {
    let decisions = host.work_item_handle.expire_due_decisions()?;
    for decision in &decisions {
        if let Some(value) = decision.resolved_value.clone() {
            write_resolved_decision_to_run(host, decision, &value).await;
        }
    }
    Ok(decisions)
}

pub(super) async fn schedule_pending_work_item_decision_timeouts(
    host: RuntimeHost,
) -> Result<(), String> {
    expire_due_work_item_decisions(&host).await?;
    let decisions = host.work_item_handle.list_pending_decisions(None)?;
    for decision in decisions {
        schedule_work_item_decision_timeout(host.clone(), &decision);
    }
    Ok(())
}

fn schedule_work_item_decision_timeout(host: RuntimeHost, decision: &roux_core::WorkItemDecision) {
    if decision.status != roux_core::WorkItemDecisionStatus::Pending
        || decision.timeout_at.is_none()
        || decision.default_value.is_none()
    {
        return;
    }
    let decision_id = decision.id.clone();
    let timeout_at = decision.timeout_at.unwrap();
    tokio::spawn(async move {
        let now = unix_now_secs();
        if timeout_at > now {
            tokio::time::sleep(std::time::Duration::from_secs(timeout_at - now)).await;
        }
        let Ok(Some(decision)) = host.work_item_handle.timeout_decision_to_default(&decision_id)
        else {
            return;
        };
        if let Some(value) = decision.resolved_value.clone() {
            write_resolved_decision_to_run(&host, &decision, &value).await;
        }
    });
}

#[derive(Default)]
struct WorkItemRunOutputParser {
    line_buffer: String,
}

impl WorkItemRunOutputParser {
    fn ingest(&mut self, text: &str) -> Vec<AgentDecisionPrompt> {
        let mut decisions = Vec::new();
        self.line_buffer.push_str(text);
        while let Some(newline) = self.line_buffer.find('\n') {
            let mut line = self.line_buffer.drain(..=newline).collect::<String>();
            line.truncate(line.trim_end_matches(['\r', '\n']).len());
            if let Some(decision) = parse_agent_decision_line(&line) {
                decisions.push(decision);
            }
        }
        if self.line_buffer.len() > 64 * 1024 {
            let keep_from = self.line_buffer.len().saturating_sub(8 * 1024);
            self.line_buffer.replace_range(..keep_from, "");
        }
        decisions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentDecisionPrompt {
    question: String,
    options: Vec<roux_core::WorkItemDecisionOption>,
    default_value: Option<String>,
    timeout_at: Option<u64>,
}

fn parse_agent_decision_line(line: &str) -> Option<AgentDecisionPrompt> {
    let line = strip_ansi_sequences(line).trim().to_string();
    let start = line.find('{')?;
    let end = line.rfind('}')?;
    if end <= start {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&line[start..=end]).ok()?;
    let decision = decision_payload(&value)?;
    let question = string_field(decision, &["question", "prompt", "message"])?.trim().to_string();
    if question.is_empty() {
        return None;
    }
    let options_value = decision.get("options").or_else(|| decision.get("choices"))?;
    let options = parse_agent_decision_options(options_value)?;
    let default_value =
        string_field(decision, &["defaultValue", "default", "default_value"]).map(str::to_string);
    let timeout_at = parse_decision_timeout_at(decision, default_value.as_deref()).ok().flatten();
    Some(AgentDecisionPrompt { question, options, default_value, timeout_at })
}

fn decision_payload(value: &serde_json::Value) -> Option<&serde_json::Value> {
    if is_decision_marker(value) {
        return Some(value);
    }
    let nested = value.get("decision").or_else(|| value.get("decisionPrompt"))?;
    if is_decision_marker(nested) || nested.get("question").is_some() {
        Some(nested)
    } else {
        None
    }
}

fn is_decision_marker(value: &serde_json::Value) -> bool {
    ["type", "event", "kind"].iter().any(|key| {
        value.get(*key).and_then(|value| value.as_str()).is_some_and(|marker| {
            matches!(marker, "decision" | "decisionPrompt" | "decision_prompt")
        })
    })
}

fn parse_agent_decision_options(
    value: &serde_json::Value,
) -> Option<Vec<roux_core::WorkItemDecisionOption>> {
    let array = value.as_array()?;
    let mut options = Vec::new();
    for value in array {
        let option = if let Some(label) = value.as_str() {
            roux_core::WorkItemDecisionOption { value: label.to_string(), label: label.to_string() }
        } else {
            let value_text = string_field(value, &["value", "id", "key", "label", "text"])?;
            let label_text = string_field(value, &["label", "text", "title", "value", "id", "key"])
                .unwrap_or(value_text);
            roux_core::WorkItemDecisionOption {
                value: value_text.to_string(),
                label: label_text.to_string(),
            }
        };
        if option.value.trim().is_empty() || option.label.trim().is_empty() {
            return None;
        }
        options.push(option);
    }
    (!options.is_empty()).then_some(options)
}

fn string_field<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key).and_then(|value| value.as_str()))
}

fn strip_ansi_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() || next == '\u{7}' {
                    break;
                }
            }
        } else if !ch.is_control() || ch == '\t' {
            output.push(ch);
        }
    }
    output
}

/// Type the resolved profile's startup sequence (env, setup, agent command
/// with the project prompt folded in) into a freshly-dispatched session's PTY
/// so the card runs an agent headlessly.
async fn run_dispatched_profile(
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
    run_id: &str,
    session_id: &str,
    profile_id: &str,
    identity: &DaemonIdentity,
    force_start: bool,
) -> Result<String, String> {
    let session = host.session_handle.get(session_id).await.ok().flatten();
    let settings = load_daemon_settings();
    let plan_context = load_work_item_plan_prompt_context(host, &item.id, force_start);
    let review_feedback = load_work_item_review_feedback_prompt_context(host, &item.id);
    let task_prompt = render_work_item_task_prompt(
        item,
        run_id,
        session.as_ref(),
        &settings,
        plan_context.as_ref(),
        review_feedback.as_ref(),
    );
    run_dispatched_profile_with_task_prompt(
        host,
        item,
        run_id,
        session_id,
        profile_id,
        identity,
        &task_prompt,
        false,
    )
    .await
}

async fn run_dispatched_planning_profile(
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
    run_id: &str,
    session_id: &str,
    profile_id: &str,
    identity: &DaemonIdentity,
) -> Result<String, String> {
    let session = host.session_handle.get(session_id).await.ok().flatten();
    let settings = load_daemon_settings();
    let task_prompt = render_work_item_planning_prompt(item, run_id, session.as_ref(), &settings);
    run_dispatched_profile_with_task_prompt(
        host,
        item,
        run_id,
        session_id,
        profile_id,
        identity,
        &task_prompt,
        true,
    )
    .await
}

async fn run_dispatched_profile_with_task_prompt(
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
    run_id: &str,
    session_id: &str,
    profile_id: &str,
    identity: &DaemonIdentity,
    task_prompt: &str,
    planning: bool,
) -> Result<String, String> {
    let settings = load_daemon_settings();
    let Some(profile) = roux_core::providers::resolve_profile(profile_id, &settings) else {
        return Err(format!("agent profile not found: {profile_id}"));
    };
    let profile = if planning {
        roux_core::providers::profile_with_planning_constraints(&profile)
    } else {
        profile
    };
    let session = host
        .session_handle
        .get(session_id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    let append = render_dispatch_project_prompt(host, item, session_id, &profile, &settings).await;
    let append_opt = (!append.trim().is_empty()).then_some(append.as_str());
    let Some(command) = roux_core::providers::profile_startup_command_with_initial_prompt(
        &profile,
        append_opt,
        task_prompt,
    ) else {
        return Err("agentProfile did not produce startup command".to_string());
    };

    let replace_primary = planning || work_item_run_replaces_primary(host, &item.id);
    let pty_id = if replace_primary {
        session.primary_pty_id.clone().unwrap_or_else(|| session_id.to_string())
    } else {
        work_item_run_pty_id(false, session_id, run_id)
    };
    if replace_primary {
        host.pty_handle
            .remove(&pty_id)
            .await
            .map_err(|err| format!("failed to replace session shell for work item run: {err}"))?;
    }

    let working_dir = profile_working_dir(&profile, &session);
    let env_args = serde_json::Value::Null;
    match host
        .pty_handle
        .spawn_task(
            command,
            PtySpawnRequest {
                id: Some(pty_id.clone()),
                working_dir: Some(working_dir),
                session_id: Some(session_id.to_string()),
                pane_id: Some(if replace_primary {
                    format!("{session_id}-main")
                } else {
                    pty_id.clone()
                }),
                project_id: item.project_id.clone(),
                worktree_path: session.is_worktree.then(|| session.worktree_path.clone()),
                env: parse_pty_env_request(&env_args, identity),
                profile: Some(profile_id.to_string()),
                profile_data: Some(profile.clone()),
                terminal_defaults: settings.terminal_defaults.clone(),
                role: if replace_primary {
                    roux_core::PtyRole::SessionPrimary
                } else {
                    roux_core::PtyRole::Secondary
                },
                ..PtySpawnRequest::default()
            },
        )
        .await
    {
        Ok(_) => Ok(pty_id),
        Err(err) if replace_primary => {
            Err(cleanup_failed_agent_launch_session(host, session_id, err).await)
        }
        Err(err) => Err(format!("failed to launch agent task in session: {err}")),
    }
}

async fn cleanup_failed_agent_launch_session(
    host: &RuntimeHost,
    session_id: &str,
    err: impl std::fmt::Display,
) -> String {
    kill_session_ptys(host, session_id).await;
    let archive_result = host.session_handle.archive(session_id).await;
    let mut message = format!("failed to launch agent task in session: {err}");
    if let Err(archive_err) = archive_result {
        message.push_str(&format!("; additionally failed to archive session: {archive_err}"));
    } else {
        message.push_str("; session archived after failed agent launch");
    }
    message
}

fn profile_working_dir(profile: &roux_core::SpawnProfile, session: &roux_core::Session) -> PathBuf {
    let Some(cwd) = profile.cwd_override.as_deref().map(str::trim).filter(|cwd| !cwd.is_empty())
    else {
        return PathBuf::from(&session.worktree_path);
    };
    let cwd = PathBuf::from(cwd);
    if cwd.is_absolute() {
        cwd
    } else {
        PathBuf::from(&session.worktree_path).join(cwd)
    }
}

fn render_work_item_planning_prompt(
    item: &roux_core::WorkItem,
    run_id: &str,
    session: Option<&roux_core::Session>,
    settings: &roux_core::RouxSettings,
) -> String {
    let item_id = sanitize_card_prompt_field(&item.id);
    let run_id = sanitize_card_prompt_field(run_id);
    let title = sanitize_card_prompt_field(&item.title);
    let body = item.body.as_deref().map(sanitize_card_prompt_field).unwrap_or_default();
    let external_url =
        item.external_url.as_deref().map(sanitize_card_prompt_field).unwrap_or_default();
    let repo_path = item.repo_path.as_deref().map(sanitize_card_prompt_field);
    let worktree_path = session
        .map(|session| sanitize_card_prompt_field(&session.worktree_path))
        .or_else(|| item.worktree_path.as_deref().map(sanitize_card_prompt_field));
    let session_id = session
        .map(|session| sanitize_card_prompt_field(&session.id))
        .or_else(|| item.session_id.as_deref().map(sanitize_card_prompt_field));
    let roux_cli_path = sanitize_card_prompt_field(&resolve_roux_cli_prompt_path());

    let mut prompt = String::new();
    prompt.push_str("Plan this Roux board card before implementation.\n\nTitle:\n");
    prompt.push_str(if title.is_empty() { "Untitled" } else { &title });
    if !body.is_empty() {
        prompt.push_str("\n\nCurrent description:\n");
        prompt.push_str(&body);
    }
    if !external_url.is_empty() {
        prompt.push_str("\n\nExternal link:\n");
        prompt.push_str(&external_url);
    }
    prompt.push_str("\n\nPlanning context:\n");
    prompt.push_str("- Roux work item id: ");
    prompt.push_str(if item_id.is_empty() { "unknown" } else { &item_id });
    prompt.push_str("\n- Roux run id: ");
    prompt.push_str(if run_id.is_empty() { "unknown" } else { &run_id });
    prompt.push_str("\n- Repository path: ");
    prompt.push_str(repo_path.as_deref().unwrap_or("unspecified"));
    prompt.push_str("\n- Planning workspace: ");
    prompt.push_str(worktree_path.as_deref().unwrap_or("unknown"));
    prompt.push_str("\n- Roux session id: ");
    prompt.push_str(session_id.as_deref().unwrap_or("unknown"));
    prompt.push_str("\n- Roux CLI path: ");
    prompt.push_str(&roux_cli_path);
    prompt.push_str("\n- Roux CLI help: `");
    prompt.push_str(&roux_cli_path);
    prompt.push_str(" --help`");
    prompt.push_str(
        "\n\nInstructions:\n\
         - Do not implement the task yet unless the user explicitly asks you to.\n\
	         - Clarify the problem statement and likely acceptance criteria.\n\
	         - Before planning, inspect attached context with `roux document list --work-item ",
    );
    prompt.push_str(if item_id.is_empty() { "<work-item-id>" } else { &item_id });
    prompt.push_str(
        "` and read relevant entries with `roux document get <document-id>`.\n\
	         - Identify likely files, systems, risks, and test strategy.\n\
	         - Suggest project/repo, autonomous agent profile, and base branch when they are missing.\n\
	         - Produce a concise plan that can be copied back onto the card, and attach it with `roux document attach --work-item ",
    );
    prompt.push_str(if item_id.is_empty() { "<work-item-id>" } else { &item_id });
    prompt.push_str(
        " --title \"Plan\" --text \"...\"`.\n\
	         - If you need a human decision, ask in this session and wait for the answer here.\n\
	         - For structured decisions that must be tracked on the card, the Roux CLI is still available: `<Roux CLI path> work-item decision create <Roux run id> \"Question?\" --option yes=Yes --option no=No`.",
    );
    append_custom_prompt_section(
        &mut prompt,
        "Additional planning instructions",
        &settings.kanban.planning_prompt_append,
    );
    prompt
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkItemPlanPromptContext {
    Attached { title: String, document_id: String, source_path: Option<String>, content: String },
    ForceStartedWithoutPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkItemReviewFeedbackPromptContext {
    title: String,
    document_id: String,
    content: String,
}

fn load_work_item_plan_prompt_context(
    host: &RuntimeHost,
    item_id: &str,
    force_start: bool,
) -> Option<WorkItemPlanPromptContext> {
    let attachments = host
        .work_item_handle
        .list_attachments(Some(roux_core::AttachmentTargetKind::WorkItem), Some(item_id))
        .ok()?;
    if let Some(attachment) = select_latest_work_item_plan_attachment(attachments.iter()) {
        let document = host
            .work_item_handle
            .get_attachment_document(&attachment.document_id)
            .ok()
            .flatten()?;
        return Some(WorkItemPlanPromptContext::Attached {
            title: document.attachment.title.unwrap_or_else(|| "Untitled".to_string()),
            document_id: document.attachment.document_id,
            source_path: document.attachment.source_path,
            content: document.content,
        });
    }
    force_start.then_some(WorkItemPlanPromptContext::ForceStartedWithoutPlan)
}

fn load_work_item_review_feedback_prompt_context(
    host: &RuntimeHost,
    item_id: &str,
) -> Option<WorkItemReviewFeedbackPromptContext> {
    if let Some(document_id) = latest_work_item_review_feedback_document_id(host, item_id) {
        if let Ok(Some(document)) = host.work_item_handle.get_attachment_document(&document_id) {
            return Some(review_feedback_context_from_document(document));
        }
    }
    let attachments = host
        .work_item_handle
        .list_attachments(Some(roux_core::AttachmentTargetKind::WorkItem), Some(item_id))
        .ok()?;
    let attachment = select_latest_work_item_review_feedback_attachment(attachments.iter())?;
    let document =
        host.work_item_handle.get_attachment_document(&attachment.document_id).ok().flatten()?;
    Some(review_feedback_context_from_document(document))
}

fn review_feedback_context_from_document(
    document: roux_core::AttachmentDocument,
) -> WorkItemReviewFeedbackPromptContext {
    WorkItemReviewFeedbackPromptContext {
        title: document.attachment.title.unwrap_or_else(|| "Review feedback".to_string()),
        document_id: document.attachment.document_id,
        content: document.content,
    }
}

fn latest_work_item_review_feedback_document_id(
    host: &RuntimeHost,
    item_id: &str,
) -> Option<String> {
    let runs = host.work_item_handle.list_runs(Some(item_id)).ok()?;
    let mut latest: Option<(u64, String, String)> = None;
    for run in runs {
        if run.kind != roux_core::WorkItemRunKind::Implementation {
            continue;
        }
        let Ok(events) = host.work_item_handle.list_run_events(&run.id) else {
            continue;
        };
        for event in events {
            let Some(document_id) = event
                .payload
                .get("feedbackDocumentId")
                .or_else(|| event.payload.get("feedback_document_id"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let candidate = (event.created_at, event.id.clone(), document_id.to_string());
            if latest.as_ref().is_none_or(|current| &candidate > current) {
                latest = Some(candidate);
            }
        }
    }
    latest.map(|(_, _, document_id)| document_id)
}

fn select_latest_work_item_plan_attachment<'a>(
    attachments: impl IntoIterator<Item = &'a roux_core::Attachment>,
) -> Option<&'a roux_core::Attachment> {
    attachments
        .into_iter()
        .filter(|attachment| {
            attachment.target_kind == roux_core::AttachmentTargetKind::WorkItem
                && is_plan_like_attachment(attachment)
        })
        .max_by(|left, right| {
            (left.updated_at, left.created_at, left.document_id.as_str(), left.id.as_str()).cmp(&(
                right.updated_at,
                right.created_at,
                right.document_id.as_str(),
                right.id.as_str(),
            ))
        })
}

fn select_latest_work_item_review_feedback_attachment<'a>(
    attachments: impl IntoIterator<Item = &'a roux_core::Attachment>,
) -> Option<&'a roux_core::Attachment> {
    attachments
        .into_iter()
        .filter(|attachment| {
            attachment.target_kind == roux_core::AttachmentTargetKind::WorkItem
                && attachment.title.as_deref().is_some_and(is_review_feedback_title)
        })
        .max_by(|left, right| {
            (left.updated_at, left.created_at, left.document_id.as_str(), left.id.as_str()).cmp(&(
                right.updated_at,
                right.created_at,
                right.document_id.as_str(),
                right.id.as_str(),
            ))
        })
}

fn is_plan_like_attachment(attachment: &roux_core::Attachment) -> bool {
    attachment.title.as_deref().is_some_and(contains_plan_word)
        || attachment.source_path.as_deref().is_some_and(|path| {
            Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(contains_plan_word)
        })
}

fn is_review_feedback_title(value: &str) -> bool {
    let words = value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>();
    words.iter().any(|part| part == "feedback")
        && words.iter().any(|part| part == "review" || part == "changes")
}

fn contains_plan_word(value: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case("plan"))
}

fn render_work_item_task_prompt(
    item: &roux_core::WorkItem,
    run_id: &str,
    session: Option<&roux_core::Session>,
    settings: &roux_core::RouxSettings,
    plan_context: Option<&WorkItemPlanPromptContext>,
    review_feedback: Option<&WorkItemReviewFeedbackPromptContext>,
) -> String {
    let item_id = sanitize_card_prompt_field(&item.id);
    let run_id = sanitize_card_prompt_field(run_id);
    let title = sanitize_card_prompt_field(&item.title);
    let body = item.body.as_deref().map(sanitize_card_prompt_field).unwrap_or_default();
    let external_url =
        item.external_url.as_deref().map(sanitize_card_prompt_field).unwrap_or_default();
    let repo_path = item.repo_path.as_deref().map(sanitize_card_prompt_field);
    let worktree_path = session
        .map(|session| sanitize_card_prompt_field(&session.worktree_path))
        .or_else(|| item.worktree_path.as_deref().map(sanitize_card_prompt_field));
    let branch = session
        .map(|session| sanitize_card_prompt_field(&session.branch))
        .filter(|branch| !branch.is_empty());
    let base_branch = item.base_branch.as_deref().map(sanitize_card_prompt_field);
    let agent_profile = item.agent_profile.as_deref().map(sanitize_card_prompt_field);
    let session_id = session
        .map(|session| sanitize_card_prompt_field(&session.id))
        .or_else(|| item.session_id.as_deref().map(sanitize_card_prompt_field));
    let roux_cli_path = sanitize_card_prompt_field(&resolve_roux_cli_prompt_path());

    let mut prompt = String::new();
    prompt.push_str("Start work on this Roux board card.\n\nTitle:\n");
    prompt.push_str(if title.is_empty() { "Untitled" } else { &title });
    if !body.is_empty() {
        prompt.push_str("\n\nDescription:\n");
        prompt.push_str(&body);
    }
    if !external_url.is_empty() {
        prompt.push_str("\n\nExternal link:\n");
        prompt.push_str(&external_url);
    }
    append_plan_prompt_context(&mut prompt, plan_context);
    append_review_feedback_prompt_context(&mut prompt, review_feedback);
    prompt.push_str("\n\nExecution context:\n");
    prompt.push_str("- Roux work item id: ");
    prompt.push_str(if item_id.is_empty() { "unknown" } else { &item_id });
    prompt.push_str("\n- Roux run id: ");
    prompt.push_str(if run_id.is_empty() { "unknown" } else { &run_id });
    prompt.push_str("\n- Repository path: ");
    prompt.push_str(repo_path.as_deref().unwrap_or("unknown"));
    prompt.push_str("\n- Worktree path: ");
    prompt.push_str(worktree_path.as_deref().unwrap_or("unknown"));
    prompt.push_str("\n- Current branch: ");
    prompt.push_str(branch.as_deref().unwrap_or("unknown"));
    prompt.push_str("\n- Base branch: ");
    prompt.push_str(base_branch.as_deref().unwrap_or("unspecified"));
    prompt.push_str("\n- Agent profile: ");
    prompt.push_str(agent_profile.as_deref().unwrap_or("unspecified"));
    prompt.push_str("\n- Roux session id: ");
    prompt.push_str(session_id.as_deref().unwrap_or("unknown"));
    prompt.push_str("\n- Roux CLI path: ");
    prompt.push_str(&roux_cli_path);
    prompt.push_str("\n- Roux CLI help: `");
    prompt.push_str(&roux_cli_path);
    prompt.push_str(" --help`");

    prompt.push_str(
        "\n\nInstructions:\n\
	         - Work in the worktree path above.\n\
	         - Before coding, inspect attached context with `roux document list --work-item ",
    );
    prompt.push_str(if item_id.is_empty() { "<work-item-id>" } else { &item_id });
    prompt.push_str(
        "` and read relevant entries with `roux document get <document-id>`.\n\
	         - Treat the attached plan as the source of truth when an approved implementation plan is included above. If it conflicts with the card description, prefer the attached plan unless the user explicitly says otherwise.\n\
	         - When requested review changes are included above, address them before taking unrelated implementation work.\n\
	         - Treat the card description as the source of acceptance criteria when no approved implementation plan is included above and it includes acceptance criteria; otherwise infer the smallest useful acceptance criteria from the card.\n\
	         - Make the necessary code and documentation changes.\n\
         - Commit changes unless the repository or user instructions clearly say not to.\n\
         - Run the relevant tests/checks and report what passed, failed, or was not run.\n\
         - If you need a human decision, ask in this session and wait for the answer here.\n\
         - For structured decisions that must be tracked on the card, the Roux CLI is still available: `<Roux CLI path> work-item decision create <Roux run id> \"Question?\" --option yes=Yes --option no=No`.\n\
         - When the work is complete, run `",
    );
    prompt.push_str(&roux_cli_path);
    prompt.push_str(" work-item review request ");
    prompt.push_str(if run_id.is_empty() { "<run-id>" } else { &run_id });
    prompt.push_str(
        "` with `--summary`, repeated `--test`, and repeated `--changed-file` flags to move this card to Review and persist the review package. Then report the summary, tests, risks, and changed files. If that command fails, report the error. Do not mark the card done yourself.",
    );
    append_custom_prompt_section(
        &mut prompt,
        "Additional implementation instructions",
        &settings.kanban.implementation_prompt_append,
    );
    append_custom_prompt_section(
        &mut prompt,
        "Additional review handoff instructions",
        &settings.kanban.review_prompt_append,
    );
    prompt
}

fn append_plan_prompt_context(
    prompt: &mut String,
    plan_context: Option<&WorkItemPlanPromptContext>,
) {
    match plan_context {
        Some(WorkItemPlanPromptContext::Attached { title, document_id, source_path, content }) => {
            let title = sanitize_card_prompt_field(title);
            let document_id = sanitize_card_prompt_field(document_id);
            let source_path = source_path.as_deref().map(sanitize_card_prompt_field);
            let content = sanitize_card_prompt_field(content);
            prompt.push_str("\n\nApproved implementation plan:\n");
            prompt.push_str("Title: ");
            prompt.push_str(if title.is_empty() { "Untitled" } else { &title });
            prompt.push_str("\nDocument id: ");
            prompt.push_str(if document_id.is_empty() { "unknown" } else { &document_id });
            if let Some(source_path) = source_path.as_deref().filter(|value| !value.is_empty()) {
                prompt.push_str("\nSource path: ");
                prompt.push_str(source_path);
            }
            prompt.push_str("\nContent:\n");
            prompt.push_str(&content);
            prompt
                .push_str("\n\nTreat the attached plan as the source of truth for implementation.");
        }
        Some(WorkItemPlanPromptContext::ForceStartedWithoutPlan) => {
            prompt.push_str(
                "\n\nPlanning gate override:\nNo approved plan was attached; implementation was force-started.",
            );
        }
        None => {}
    }
}

fn append_review_feedback_prompt_context(
    prompt: &mut String,
    review_feedback: Option<&WorkItemReviewFeedbackPromptContext>,
) {
    let Some(review_feedback) = review_feedback else {
        return;
    };
    let title = sanitize_card_prompt_field(&review_feedback.title);
    let document_id = sanitize_card_prompt_field(&review_feedback.document_id);
    let content = sanitize_card_prompt_field(&review_feedback.content);
    prompt.push_str("\n\nRequested review changes:\n");
    prompt.push_str("Title: ");
    prompt.push_str(if title.is_empty() { "Review feedback" } else { &title });
    prompt.push_str("\nDocument id: ");
    prompt.push_str(if document_id.is_empty() { "unknown" } else { &document_id });
    prompt.push_str("\nContent:\n");
    prompt.push_str(&content);
}

fn append_custom_prompt_section(prompt: &mut String, heading: &str, value: &str) {
    let value = sanitize_card_prompt_field(value);
    if value.is_empty() {
        return;
    }
    prompt.push_str("\n\n");
    prompt.push_str(heading);
    prompt.push_str(":\n");
    prompt.push_str(&value);
}

fn resolve_roux_cli_prompt_path() -> String {
    std::env::var("ROUX_CLI")
        .ok()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .or_else(|| std::env::current_exe().ok().map(|path| path.to_string_lossy().into_owned()))
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "roux".to_string())
}

fn sanitize_card_prompt_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\r' => out.push('\n'),
            '\n' | '\t' => out.push(ch),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.trim().to_string()
}

/// Render the dispatched card's project prompt (`--append-system-prompt` text)
/// using the same context the desktop builds. Empty string when the card has
/// no project, the project/session can't be loaded, or rendering fails.
async fn render_dispatch_project_prompt(
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
    session_id: &str,
    profile: &roux_core::SpawnProfile,
    settings: &roux_core::RouxSettings,
) -> String {
    let Some(pid) = item.project_id.as_deref() else {
        return String::new();
    };
    let (Ok(Some(project)), Ok(Some(session))) =
        (host.project_handle.get(pid).await, host.session_handle.get(session_id).await)
    else {
        return String::new();
    };
    let others: Vec<roux_core::Session> = host
        .session_handle
        .list()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.project_id.as_deref() == Some(pid) && s.id != session.id && !s.archived)
        .collect();
    roux_runtime::project_prompt::render_for_session(
        &project,
        &session,
        Some(profile),
        settings,
        &others,
    )
    .unwrap_or_default()
}

pub(super) async fn handle_work_item_import(req: Request, host: &RuntimeHost) -> Response {
    // Accept inline args.items or a path to a JSON file containing { "items": [...] }
    let items_value = if let Some(items) = req.args.get("items") {
        items.clone()
    } else if let Some(path) = req.args.get("path").and_then(|v| v.as_str()) {
        match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str::<serde_json::Value>(&contents) {
                Ok(v) => v.get("items").cloned().unwrap_or(v),
                Err(err) => return Response::err(format!("failed to parse import file: {err}")),
            },
            Err(err) => return Response::err(format!("failed to read import file: {err}")),
        }
    } else {
        return Response::err("items or path required");
    };

    let Some(items_array) = items_value.as_array() else {
        return Response::err("items must be an array");
    };

    // First pass: upsert/create each item; build external->id map for parent resolution.
    let mut external_to_id: std::collections::HashMap<(String, String), String> =
        Default::default();
    let mut imported_ids: Vec<String> = Vec::new();
    // (item_id, provider, parent_external_id) to resolve in second pass
    let mut parent_links: Vec<(String, String, String)> = Vec::new();

    for item_val in items_array {
        let title = item_val.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.is_empty() {
            return Response::err("each import item must have a title");
        }
        let external_ref = parse_import_external_ref(item_val);
        let input = roux_core::WorkItemInput {
            title,
            body: item_val.get("body").and_then(|v| v.as_str()).map(str::to_string),
            status: item_val
                .get("status")
                .and_then(|v| v.as_str())
                .and_then(roux_core::WorkItemStatus::from_str_opt),
            repo_path: optional_string_arg(item_val, &["repoPath", "repo_path"]),
            agent_profile: optional_string_arg(item_val, &["agentProfile", "agent_profile"]),
            base_branch: optional_string_arg(item_val, &["baseBranch", "base_branch", "base"]),
            worktree_path: optional_string_arg(item_val, &["worktreePath", "worktree_path"]),
            branch: optional_string_arg(item_val, &["branch", "worktreeBranch", "worktree_branch"]),
            fetch_first: bool_arg(item_val, &["fetchFirst", "fetch_first"]),
            start_error: optional_nullable_string_arg(item_val, &["startError", "start_error"]),
            project_id: optional_string_arg(item_val, &["projectId", "project_id"]),
            parent_id: None, // resolved in second pass
            external_ref,
            sort_order: None,
            field_presence: work_item_input_presence(item_val),
        };
        // Both paths are silent during import; a single Imported event is
        // broadcast at the end so subscribers see one consistent batch signal.
        let item = if input.external_ref.is_some() {
            match host.work_item_handle.upsert_by_external(input) {
                Ok(item) => item,
                Err(err) => return Response::err(err),
            }
        } else {
            match host.work_item_handle.insert_silent(input) {
                Ok(item) => item,
                Err(err) => return Response::err(err),
            }
        };

        if let (Some(provider), Some(ext_id)) = (&item.provider, &item.external_id) {
            external_to_id.insert((provider.clone(), ext_id.clone()), item.id.clone());
        }

        if let Some(peid) = item_val.get("parentExternalId").and_then(|v| v.as_str()) {
            if let Some(provider) = &item.provider {
                parent_links.push((item.id.clone(), provider.clone(), peid.to_string()));
            }
        }

        imported_ids.push(item.id.clone());
    }

    // Second pass: resolve parentExternalId -> parent_id within the batch.
    let mut second_pass_errors: Vec<String> = Vec::new();
    for (item_id, provider, parent_ext_id) in parent_links {
        if let Some(parent_id) = external_to_id.get(&(provider, parent_ext_id)) {
            match host.work_item_handle.get(&item_id) {
                Ok(Some(existing)) => {
                    let ext_ref = existing.provider.as_ref().map(|p| roux_core::ExternalRef {
                        provider: p.clone(),
                        external_id: existing.external_id.clone().unwrap_or_default(),
                        url: existing.external_url.clone(),
                    });
                    let update = roux_core::WorkItemInput {
                        title: existing.title,
                        body: existing.body,
                        status: Some(existing.status),
                        repo_path: existing.repo_path,
                        agent_profile: existing.agent_profile,
                        base_branch: existing.base_branch,
                        worktree_path: existing.worktree_path,
                        branch: existing.branch,
                        fetch_first: existing.fetch_first,
                        start_error: existing.start_error,
                        project_id: existing.project_id,
                        parent_id: Some(parent_id.clone()),
                        external_ref: ext_ref,
                        sort_order: Some(existing.sort_order),
                        field_presence: Default::default(),
                    };
                    if let Err(err) = host.work_item_handle.update_silent(&item_id, update) {
                        second_pass_errors.push(format!("parent link for {item_id}: {err}"));
                    }
                }
                Ok(None) => {
                    second_pass_errors.push(format!("parent link for {item_id}: item not found"));
                }
                Err(err) => {
                    second_pass_errors.push(format!("parent link for {item_id}: {err}"));
                }
            }
        }
    }
    // Broadcast before returning so the frontend hydrates even when parent links
    // partially failed - items are already durable in the DB.
    host.work_item_handle.broadcast_imported(imported_ids.clone());

    if !second_pass_errors.is_empty() {
        return Response::err(format!(
            "import succeeded but {} parent link(s) failed: {}",
            second_pass_errors.len(),
            second_pass_errors.join("; ")
        ));
    }

    Response::success(serde_json::json!({ "imported": imported_ids.len(), "ids": imported_ids }))
}

fn parse_import_external_ref(item_val: &serde_json::Value) -> Option<roux_core::ExternalRef> {
    let ext = item_val.get("externalRef").or_else(|| item_val.get("external_ref"))?;
    let provider = ext.get("provider").and_then(|v| v.as_str())?;
    let external_id =
        ext.get("externalId").or_else(|| ext.get("external_id")).and_then(|v| v.as_str())?;
    Some(roux_core::ExternalRef {
        provider: provider.to_string(),
        external_id: external_id.to_string(),
        url: ext.get("url").and_then(|v| v.as_str()).map(str::to_string),
    })
}

pub(super) async fn handle_work_item_events_stream<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_work_item_events_stream_inner(req, writer, host, identity).await;
    let _ = writer.shutdown().await;
    result
}

pub(super) async fn handle_work_item_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_work_item_event_frame(
            writer,
            &WorkItemEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }
    let mut rx = host.work_item_handle.subscribe_events();
    if !write_work_item_event_frame(writer, &WorkItemEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_work_item_event_frame(
                    writer,
                    &WorkItemEventFrame::Event { event: Box::new(event) },
                )
                .await
                {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = WorkItemEventFrame::Warning {
                    message: format!("dropped {skipped} buffered work-item event(s)"),
                };
                if !write_work_item_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_work_item_event_frame<W>(writer: &mut W, frame: &WorkItemEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::handle_request;
    use super::*;
    use roux_runtime::host::RuntimeHostConfig;
    #[cfg(not(windows))]
    use roux_runtime::pty_service::PtySpawnRequest;

    #[test]
    fn parses_agent_decision_json_line() {
        let line = r#"{"type":"decision","question":"Choose path?","options":[{"value":"existing","label":"Use existing"},{"value":"new","label":"Create new"}],"defaultValue":"existing","timeoutAt":123}"#;

        let decision = parse_agent_decision_line(line).expect("decision should parse");

        assert_eq!(decision.question, "Choose path?");
        assert_eq!(decision.default_value.as_deref(), Some("existing"));
        assert_eq!(decision.timeout_at, Some(123));
        assert_eq!(
            decision.options,
            vec![
                roux_core::WorkItemDecisionOption {
                    value: "existing".into(),
                    label: "Use existing".into(),
                },
                roux_core::WorkItemDecisionOption {
                    value: "new".into(),
                    label: "Create new".into(),
                },
            ]
        );
    }

    #[test]
    fn parses_nested_decision_with_string_options() {
        let line = "\u{1b}[32magent\u{1b}[0m {\"decision\":{\"question\":\"Pick one\",\"options\":[\"A\",\"B\"],\"default\":\"A\"}}";

        let decision = parse_agent_decision_line(line).expect("nested decision should parse");

        assert_eq!(decision.question, "Pick one");
        assert_eq!(decision.default_value.as_deref(), Some("A"));
        assert_eq!(
            decision.options,
            vec![
                roux_core::WorkItemDecisionOption { value: "A".into(), label: "A".into() },
                roux_core::WorkItemDecisionOption { value: "B".into(), label: "B".into() },
            ]
        );
    }
    async fn make_host_and_identity(
        dir: &tempfile::TempDir,
    ) -> (RuntimeHost, DaemonIdentity, Vec<tokio::task::JoinHandle<()>>) {
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
        (host, identity, joins)
    }

    async fn shutdown_host(host: RuntimeHost, joins: Vec<tokio::task::JoinHandle<()>>) {
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    fn req(command: &str, args: serde_json::Value) -> Request {
        Request {
            command: command.to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args,
        }
    }

    fn session(id: &str) -> roux_core::Session {
        roux_core::Session {
            id: id.to_string(),
            name: format!("Session {id}"),
            repo_root: "/repo".to_string(),
            worktree_path: "/repo".to_string(),
            branch: "main".to_string(),
            is_worktree: false,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: None,
            is_git_repo: false,
            name_override: None,
            primary_pty_id: None,
            archived: false,
            ended_at: None,
            blueprint_id: None,
            pinned_pr_url: None,
        }
    }

    #[cfg(not(windows))]
    fn git(repo: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("failed to invoke git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(not(windows))]
    fn init_repo(repo: &std::path::Path) {
        std::fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "-q", "-b", "main"]);
        git(repo, &["config", "user.email", "t@t.test"]);
        git(repo, &["config", "user.name", "Test"]);
        git(repo, &["commit", "--allow-empty", "-m", "init"]);
    }

    #[cfg(not(windows))]
    async fn start_agent_work_item(
        host: &RuntimeHost,
        identity: &DaemonIdentity,
        repo: &std::path::Path,
        item_id: &str,
    ) -> (String, String, serde_json::Value) {
        let result = start_work_item_run_with_hooks(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            host,
            identity,
            dummy_agent_dispatcher,
            noop_after_session_created,
        )
        .await
        .expect("work-item-start should succeed");
        let data = serde_json::to_value(&result).expect("serialize start result");
        let run_id = data["run"]["id"].as_str().unwrap().to_string();
        let session_id = data["run"]["sessionId"].as_str().unwrap().to_string();
        assert_eq!(data["item"]["status"], "doing");
        (run_id, session_id, data)
    }

    fn failing_profile_dispatcher<'a>(
        _host: &'a RuntimeHost,
        _item: &'a roux_core::WorkItem,
        _run_id: &'a str,
        _session_id: &'a str,
        _profile_id: &'a str,
        _identity: &'a DaemonIdentity,
        _force_start: bool,
    ) -> ProfileDispatchFuture<'a> {
        Box::pin(async { Err("simulated prompt dispatch failure".to_string()) })
    }

    fn successful_profile_dispatcher<'a>(
        _host: &'a RuntimeHost,
        _item: &'a roux_core::WorkItem,
        _run_id: &'a str,
        session_id: &'a str,
        _profile_id: &'a str,
        _identity: &'a DaemonIdentity,
        _force_start: bool,
    ) -> ProfileDispatchFuture<'a> {
        Box::pin(async { Ok(session_id.to_string()) })
    }

    /// Test dispatcher that mirrors the real agent dispatch's PTY-id scheme and
    /// session-reuse semantics, but spawns a long-lived dependency-free command
    /// instead of the `claude` binary. CI runners have no `claude` on PATH, so
    /// the real dispatcher's PTY exits immediately (exit 127) and the
    /// run-output monitor marks the run failed — which makes any "session stays
    /// bound / run stays active" assertion environment-dependent. This keeps the
    /// run's PTY alive so those assertions exercise the feature, not the host.
    #[cfg(not(windows))]
    fn dummy_agent_dispatcher<'a>(
        host: &'a RuntimeHost,
        item: &'a roux_core::WorkItem,
        run_id: &'a str,
        session_id: &'a str,
        _profile_id: &'a str,
        _identity: &'a DaemonIdentity,
        _force_start: bool,
    ) -> ProfileDispatchFuture<'a> {
        Box::pin(dispatch_dummy_agent(host, item, run_id, session_id))
    }

    #[cfg(not(windows))]
    async fn dispatch_dummy_agent(
        host: &RuntimeHost,
        item: &roux_core::WorkItem,
        run_id: &str,
        session_id: &str,
    ) -> Result<String, String> {
        let session = host
            .session_handle
            .get(session_id)
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("session not found: {session_id}"))?;

        // Same PTY-id and primary-replacement decision the real dispatcher makes
        // (see `run_dispatched_profile_with_task_prompt`), so the ids the tests
        // assert on are produced by the production helpers rather than guessed.
        let replace_primary = work_item_run_replaces_primary(host, &item.id);
        let pty_id = if replace_primary {
            session.primary_pty_id.clone().unwrap_or_else(|| session_id.to_string())
        } else {
            work_item_run_pty_id(false, session_id, run_id)
        };
        if replace_primary {
            host.pty_handle.remove(&pty_id).await.map_err(|err| {
                format!("failed to replace session shell for work item run: {err}")
            })?;
        }

        host.pty_handle
            .spawn_task(
                // Long-lived, dependency-free command so the run's PTY stays
                // alive regardless of whether `claude` is installed.
                "sleep 600".to_string(),
                PtySpawnRequest {
                    id: Some(pty_id.clone()),
                    working_dir: Some(PathBuf::from(&session.worktree_path)),
                    session_id: Some(session_id.to_string()),
                    pane_id: Some(if replace_primary {
                        format!("{session_id}-main")
                    } else {
                        pty_id.clone()
                    }),
                    project_id: item.project_id.clone(),
                    worktree_path: session.is_worktree.then(|| session.worktree_path.clone()),
                    role: if replace_primary {
                        roux_core::PtyRole::SessionPrimary
                    } else {
                        roux_core::PtyRole::Secondary
                    },
                    ..PtySpawnRequest::default()
                },
            )
            .await
            .map(|_| pty_id)
            .map_err(|err| format!("failed to launch dummy agent task in session: {err}"))
    }

    fn create_active_run_after_session_created(
        host: &RuntimeHost,
        item_id: &str,
        _session: &roux_core::Session,
    ) {
        host.work_item_handle
            .create_starting_run(
                item_id,
                Some("winning-session"),
                Some("claude"),
                Some("claude"),
                Some("/winning-worktree"),
                Some("winning-branch"),
            )
            .expect("winning start should create an active run");
    }

    #[cfg(not(windows))]
    async fn spawn_task_work_item_run(
        host: &RuntimeHost,
        working_dir: &std::path::Path,
        command: &str,
        pty_id: &str,
    ) -> (String, String) {
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput { title: "Run card".into(), ..Default::default() })
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some(pty_id), None, None, None, None)
            .unwrap();
        let record = host
            .pty_handle
            .spawn_task(
                command.to_string(),
                PtySpawnRequest {
                    id: Some(pty_id.to_string()),
                    working_dir: Some(working_dir.to_path_buf()),
                    session_id: Some(pty_id.to_string()),
                    profile: Some("task".to_string()),
                    initial_size: Some((80, 24)),
                    ..PtySpawnRequest::default()
                },
            )
            .await
            .unwrap();
        start_work_item_run_output_monitor(host.clone(), run.id.clone(), record.id.clone());
        (run.id, record.id)
    }

    #[cfg(not(windows))]
    async fn wait_for_run_status(
        host: &RuntimeHost,
        run_id: &str,
        expected: roux_core::WorkItemRunStatus,
    ) -> roux_core::WorkItemRun {
        for _ in 0..60 {
            let run = host.work_item_handle.get_run(run_id).unwrap().unwrap();
            if run.status == expected {
                return run;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let run = host.work_item_handle.get_run(run_id).unwrap().unwrap();
        panic!("run {run_id} status was {:?}, expected {expected:?}", run.status);
    }

    #[test]
    fn work_item_task_prompt_includes_card_context_and_sanitizes_control_chars() {
        let item = roux_core::WorkItem {
            id: "wi-1".into(),
            project_id: None,
            parent_id: None,
            title: "Fix tests\u{0007}".into(),
            body: Some("Handle failures\r\nthen report back\u{0003}".into()),
            status: roux_core::WorkItemStatus::Todo,
            repo_path: Some("/repo/main".into()),
            agent_profile: Some("claude".into()),
            base_branch: Some("main".into()),
            worktree_path: Some("/repo/.worktrees/card".into()),
            branch: Some("roux/card".into()),
            fetch_first: Some(false),
            start_error: None,
            session_id: Some("sess-1".into()),
            provider: None,
            external_id: None,
            external_url: Some("https://example.test/task\u{001b}".into()),
            sort_order: 0.0,
            pinned_pr_url: None,
            cost: None,
            created_at: 0,
            updated_at: 0,
        };
        let session = roux_core::Session {
            id: "sess-1".into(),
            name: "Task session".into(),
            repo_root: "/repo/main".into(),
            worktree_path: "/repo/.worktrees/card".into(),
            branch: "roux/card-wi1-fix-tests".into(),
            is_worktree: true,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: None,
            is_git_repo: true,
            name_override: None,
            primary_pty_id: Some("sess-1".into()),
            archived: false,
            ended_at: None,
            blueprint_id: None,
            pinned_pr_url: None,
        };

        let prompt = render_work_item_task_prompt(
            &item,
            "run-1",
            Some(&session),
            &roux_core::RouxSettings::default(),
            None,
            None,
        );

        assert!(prompt.contains("Title:\nFix tests"));
        assert!(prompt.contains("Description:\nHandle failures\n\nthen report back"));
        assert!(prompt.contains("External link:\nhttps://example.test/task"));
        assert!(prompt.contains("Roux work item id: wi-1"));
        assert!(prompt.contains("Roux run id: run-1"));
        assert!(prompt.contains("Repository path: /repo/main"));
        assert!(prompt.contains("Worktree path: /repo/.worktrees/card"));
        assert!(prompt.contains("Current branch: roux/card-wi1-fix-tests"));
        assert!(prompt.contains("Base branch: main"));
        assert!(prompt.contains("Agent profile: claude"));
        assert!(prompt.contains("Roux session id: sess-1"));
        assert!(prompt.contains("Roux CLI path:"));
        assert!(prompt.contains("Roux CLI help:"));
        assert!(prompt.contains("ask in this session"));
        assert!(prompt.contains("work-item decision create"));
        assert!(!prompt.contains("\"type\":\"decision\""));
        assert!(prompt.contains("Run the relevant tests/checks"));
        assert!(prompt.contains("work-item review request run-1"));
        assert!(prompt.contains("--summary"));
        assert!(prompt.contains("--test"));
        assert!(prompt.contains("--changed-file"));
        assert!(prompt.contains("Do not mark the card done yourself"));
        assert!(!prompt.contains('\u{0007}'));
        assert!(!prompt.contains('\u{0003}'));
        assert!(!prompt.contains('\u{001b}'));
    }

    #[test]
    fn work_item_task_prompt_includes_attached_plan_as_source_of_truth() {
        let item = roux_core::WorkItem {
            id: "wi-1".into(),
            project_id: None,
            parent_id: None,
            title: "Fix tests".into(),
            body: Some("Original card notes".into()),
            status: roux_core::WorkItemStatus::Ready,
            repo_path: Some("/repo/main".into()),
            agent_profile: Some("claude".into()),
            base_branch: Some("main".into()),
            worktree_path: Some("/repo/.worktrees/card".into()),
            branch: Some("roux/card".into()),
            fetch_first: Some(false),
            start_error: None,
            session_id: Some("sess-1".into()),
            provider: None,
            external_id: None,
            external_url: None,
            sort_order: 0.0,
            pinned_pr_url: None,
            cost: None,
            created_at: 0,
            updated_at: 0,
        };
        let plan = WorkItemPlanPromptContext::Attached {
            title: "Plan".into(),
            document_id: "wi-1.att-plan".into(),
            source_path: Some("/tmp/plan.md".into()),
            content: "1. Add the failing test.\n2. Implement the narrow fix.".into(),
        };

        let prompt = render_work_item_task_prompt(
            &item,
            "run-1",
            None,
            &roux_core::RouxSettings::default(),
            Some(&plan),
            None,
        );

        assert!(prompt.contains("Approved implementation plan:"));
        assert!(prompt.contains("Title: Plan"));
        assert!(prompt.contains("Document id: wi-1.att-plan"));
        assert!(prompt.contains("Source path: /tmp/plan.md"));
        assert!(prompt.contains("1. Add the failing test."));
        assert!(prompt.contains("Treat the attached plan as the source of truth"));
    }

    #[test]
    fn work_item_task_prompt_notes_force_start_without_attached_plan() {
        let item = roux_core::WorkItem {
            id: "wi-1".into(),
            project_id: None,
            title: "Fix tests".into(),
            body: None,
            status: roux_core::WorkItemStatus::Ready,
            parent_id: None,
            agent_profile: Some("claude".into()),
            repo_path: Some("/repo/main".into()),
            base_branch: Some("main".into()),
            worktree_path: None,
            branch: None,
            fetch_first: None,
            start_error: None,
            session_id: None,
            provider: None,
            external_id: None,
            external_url: None,
            sort_order: 0.0,
            pinned_pr_url: None,
            cost: None,
            created_at: 0,
            updated_at: 0,
        };

        let prompt = render_work_item_task_prompt(
            &item,
            "run-1",
            None,
            &roux_core::RouxSettings::default(),
            Some(&WorkItemPlanPromptContext::ForceStartedWithoutPlan),
            None,
        );

        assert!(prompt.contains("No approved plan was attached; implementation was force-started."));
    }

    #[test]
    fn work_item_task_prompt_includes_requested_review_changes() {
        let item = roux_core::WorkItem {
            id: "wi-1".into(),
            project_id: None,
            title: "Fix tests".into(),
            body: None,
            status: roux_core::WorkItemStatus::Doing,
            parent_id: None,
            agent_profile: Some("claude".into()),
            repo_path: Some("/repo/main".into()),
            base_branch: Some("main".into()),
            worktree_path: None,
            branch: None,
            fetch_first: None,
            start_error: None,
            session_id: None,
            provider: None,
            external_id: None,
            external_url: None,
            sort_order: 0.0,
            pinned_pr_url: None,
            cost: None,
            created_at: 0,
            updated_at: 0,
        };
        let feedback = WorkItemReviewFeedbackPromptContext {
            title: "Review feedback".into(),
            document_id: "wi-1.feedback".into(),
            content: "Please add coverage for the retry path.".into(),
        };

        let prompt = render_work_item_task_prompt(
            &item,
            "run-2",
            None,
            &roux_core::RouxSettings::default(),
            None,
            Some(&feedback),
        );

        assert!(prompt.contains("Requested review changes:"));
        assert!(prompt.contains("Document id: wi-1.feedback"));
        assert!(prompt.contains("Please add coverage for the retry path."));
        assert!(prompt.contains("address them before taking unrelated implementation work"));
    }

    #[test]
    fn newest_plan_like_attachment_wins_plan_selection() {
        let older = roux_core::Attachment {
            id: "att-old".into(),
            document_id: "wi-1.att-old".into(),
            target_kind: roux_core::AttachmentTargetKind::WorkItem,
            target_id: "wi-1".into(),
            title: Some("Plan".into()),
            content_kind: roux_core::AttachmentContentKind::Text,
            mime_type: Some("text/markdown".into()),
            source_path: None,
            byte_len: 10,
            sha256: "old".into(),
            created_at: 1,
            updated_at: 1,
        };
        let notes = roux_core::Attachment {
            id: "att-notes".into(),
            document_id: "wi-1.att-notes".into(),
            target_kind: roux_core::AttachmentTargetKind::WorkItem,
            target_id: "wi-1".into(),
            title: Some("Notes".into()),
            content_kind: roux_core::AttachmentContentKind::Text,
            mime_type: Some("text/markdown".into()),
            source_path: Some("/tmp/notes.md".into()),
            byte_len: 10,
            sha256: "notes".into(),
            created_at: 3,
            updated_at: 3,
        };
        let newer = roux_core::Attachment {
            id: "att-new".into(),
            document_id: "wi-1.att-new".into(),
            target_kind: roux_core::AttachmentTargetKind::WorkItem,
            target_id: "wi-1".into(),
            title: Some("Implementation Plan".into()),
            content_kind: roux_core::AttachmentContentKind::Text,
            mime_type: Some("text/markdown".into()),
            source_path: None,
            byte_len: 10,
            sha256: "new".into(),
            created_at: 2,
            updated_at: 4,
        };
        let attachments = [older, notes, newer];

        let selected = select_latest_work_item_plan_attachment(attachments.iter())
            .expect("plan attachment should be selected");

        assert_eq!(selected.id, "att-new");
    }

    #[test]
    fn work_item_prompts_append_kanban_custom_instructions() {
        let item = roux_core::WorkItem {
            id: "wi-1".into(),
            project_id: None,
            title: "Fix tests".into(),
            body: None,
            status: roux_core::WorkItemStatus::Todo,
            parent_id: None,
            agent_profile: Some("claude".into()),
            repo_path: Some("/repo/main".into()),
            base_branch: Some("main".into()),
            worktree_path: None,
            branch: None,
            fetch_first: None,
            start_error: None,
            session_id: None,
            provider: None,
            external_id: None,
            external_url: None,
            sort_order: 0.0,
            pinned_pr_url: None,
            cost: None,
            created_at: 0,
            updated_at: 0,
        };
        let settings = roux_core::RouxSettings {
            kanban: roux_core::KanbanSettings {
                planning_prompt_append: "Ask about release timing.".into(),
                implementation_prompt_append: "Use narrow commits.".into(),
                review_prompt_append: "Summarize review risks.".into(),
                ..roux_core::KanbanSettings::default()
            },
            ..roux_core::RouxSettings::default()
        };

        let plan_prompt = render_work_item_planning_prompt(&item, "plan-run-1", None, &settings);
        assert!(plan_prompt.contains("Roux work item id: wi-1"));
        assert!(plan_prompt.contains("Roux run id: plan-run-1"));
        assert!(
            plan_prompt.contains("Additional planning instructions:\nAsk about release timing.")
        );
        assert!(plan_prompt.contains("Roux CLI path:"));
        assert!(plan_prompt.contains("ask in this session"));
        assert!(plan_prompt.contains("work-item decision create"));
        assert!(plan_prompt.contains("roux document list --work-item wi-1"));
        assert!(plan_prompt.contains("roux document get <document-id>"));
        assert!(plan_prompt.contains("roux document attach --work-item wi-1"));
        assert!(!plan_prompt.contains("\"type\":\"decision\""));

        let task_prompt = render_work_item_task_prompt(&item, "run-1", None, &settings, None, None);
        assert!(task_prompt.contains("roux document list --work-item wi-1"));
        assert!(task_prompt.contains("roux document get <document-id>"));
        assert!(
            task_prompt.contains("Additional implementation instructions:\nUse narrow commits.")
        );
        assert!(task_prompt
            .contains("Additional review handoff instructions:\nSummarize review risks."));
    }

    #[tokio::test]
    async fn daemon_work_item_crud_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        // list - empty initially
        let resp =
            handle_request(req("work-item-list", serde_json::json!({})), &host, &identity).await;
        assert!(resp.ok, "list should succeed");
        assert_eq!(resp.data.as_ref().unwrap().as_array().unwrap().len(), 0);

        // create
        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Fix login bug" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "create should succeed");
        let item = resp.data.as_ref().unwrap();
        assert_eq!(item["title"], "Fix login bug");
        assert_eq!(item["status"], "todo");
        let id = item["id"].as_str().unwrap().to_string();

        // list - one item
        let resp =
            handle_request(req("work-item-list", serde_json::json!({})), &host, &identity).await;
        assert_eq!(resp.data.unwrap().as_array().unwrap().len(), 1);

        // update
        let resp = handle_request(
            req(
                "work-item-update",
                serde_json::json!({ "id": id, "title": "Fix login bug (updated)" }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "update should succeed");
        assert_eq!(resp.data.as_ref().unwrap()["title"], "Fix login bug (updated)");

        // move
        let resp = handle_request(
            req(
                "work-item-move",
                serde_json::json!({ "id": id, "status": "doing", "sortOrder": 1.0 }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "move should succeed");
        assert_eq!(resp.data.as_ref().unwrap()["status"], "doing");

        let resp = handle_request(
            req("work-item-move", serde_json::json!({ "id": id, "status": "review" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "move without sort should succeed");
        assert_eq!(resp.data.as_ref().unwrap()["status"], "review");
        assert!(resp.data.as_ref().unwrap()["sortOrder"].as_f64().unwrap() > 1.0);

        // delete
        let resp = handle_request(
            req("work-item-delete", serde_json::json!({ "id": id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "delete should succeed");
        assert_eq!(resp.data.as_ref().unwrap()["id"], id);

        // list - empty again
        let resp =
            handle_request(req("work-item-list", serde_json::json!({})), &host, &identity).await;
        assert_eq!(resp.data.unwrap().as_array().unwrap().len(), 0);

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_document_attach_and_get_for_session() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;
        host.session_handle.add(session("sess-1")).await.unwrap();

        let resp = handle_request(
            req(
                "document-attach",
                serde_json::json!({
                    "targetKind": "session",
                    "targetId": "sess-1",
                    "title": "Plan",
                    "contentKind": "text",
                    "content": "Use the narrow implementation plan.",
                    "mimeType": "text/markdown",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "document attach should succeed: {:?}", resp.error);
        let document_id = resp.data.as_ref().unwrap()["documentId"].as_str().unwrap().to_string();
        assert!(document_id.starts_with("sess-1."));

        let resp = handle_request(
            req("document-get", serde_json::json!({ "id": document_id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "document get should succeed: {:?}", resp.error);
        let document = resp.data.as_ref().unwrap();
        assert_eq!(document["attachment"]["targetKind"], "session");
        assert_eq!(document["content"], "Use the narrow implementation plan.");

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_document_list_rejects_target_id_without_kind() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("document-list", serde_json::json!({ "targetId": "ambiguous-id" })),
            &host,
            &identity,
        )
        .await;
        assert!(!resp.ok, "targetId without targetKind should fail");
        assert!(resp.error.as_deref().unwrap_or("").contains("targetKind required"));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_create_requires_title() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp =
            handle_request(req("work-item-create", serde_json::json!({})), &host, &identity).await;
        assert!(!resp.ok, "create without title should fail");
        assert!(resp.error.as_deref().unwrap_or("").contains("title required"));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_not_found_errors() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-update", serde_json::json!({ "id": "no-such-id", "title": "x" })),
            &host,
            &identity,
        )
        .await;
        assert!(!resp.ok);

        let resp = handle_request(
            req("work-item-move", serde_json::json!({ "id": "no-such-id", "status": "doing" })),
            &host,
            &identity,
        )
        .await;
        assert!(!resp.ok);

        let resp = handle_request(
            req("work-item-delete", serde_json::json!({ "id": "no-such-id" })),
            &host,
            &identity,
        )
        .await;
        assert!(!resp.ok);

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_capabilities_advertised() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp =
            handle_request(req("daemon-status", serde_json::json!({})), &host, &identity).await;
        assert!(resp.ok);
        let caps = resp.data.as_ref().unwrap()["capabilities"].as_array().unwrap().clone();
        for cap in &[
            "work-item-list",
            "work-item-create",
            "work-item-update",
            "work-item-move",
            "work-item-delete",
            "work-item-attach-session",
            "work-item-detach-session",
            "work-item-start",
            "work-item-review-request",
            "work-item-events",
        ] {
            assert!(caps.contains(&serde_json::json!(cap)), "missing capability: {cap}");
        }

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_creates_session_and_binds_it() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Write tests" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let (run_id, session_id, data) =
            start_agent_work_item(&host, &identity, &repo, &item_id).await;
        assert_eq!(data["item"]["agentProfile"], "claude");
        assert!(data["item"]["worktreePath"].as_str().unwrap().contains("roux-card"));
        assert_eq!(data["run"]["ptyId"], session_id);

        // The work item should now have session_id bound
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.session_id.as_deref(), Some(session_id.as_str()), "session_id bound");
        assert_eq!(item.status, roux_core::WorkItemStatus::Doing);

        // A card with a live binding starts another run inside the same
        // durable session rather than creating a second top-level session.
        let result2 = start_work_item_run_with_hooks(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id.clone(),
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
            dummy_agent_dispatcher,
            noop_after_session_created,
        )
        .await
        .expect("second start should reuse bound session");
        let second_data = serde_json::to_value(&result2).expect("serialize second start result");
        let second_run_id = second_data["run"]["id"].as_str().unwrap();
        let second_pty_id = format!("{session_id}-{second_run_id}");
        assert_eq!(second_data["session"]["id"], session_id);
        assert_eq!(second_data["run"]["sessionId"], session_id);
        assert_eq!(second_data["run"]["ptyId"], second_pty_id);
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(
            item.session_id.as_deref(),
            Some(session_id.as_str()),
            "session_id keeps the original binding",
        );
        assert_eq!(item.status, roux_core::WorkItemStatus::Doing);
        let runs = host.work_item_handle.list_runs(Some(&item_id)).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|run| run.session_id.as_deref() == Some(session_id.as_str())));
        let first_run = runs.iter().find(|run| run.id == run_id).unwrap();
        assert_eq!(first_run.pty_id.as_deref(), Some(session_id.as_str()));
        let second_run = runs.iter().find(|run| run.id == second_run_id).unwrap();
        assert_eq!(second_run.pty_id.as_deref(), Some(second_pty_id.as_str()));
        let sessions = host.session_handle.list().await.unwrap();
        assert_eq!(sessions.len(), 1);
        let ptys = host.pty_handle.list().await.unwrap();
        assert_eq!(
            ptys.iter()
                .filter(|pty| pty.info.session_id.as_deref() == Some(session_id.as_str()))
                .count(),
            2
        );

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_requires_attached_plan_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Needs plan".into(),
                status: Some(roux_core::WorkItemStatus::Ready),
                ..Default::default()
            })
            .unwrap();
        let item_id = item.id.clone();

        let blocked = start_work_item_run_with_hooks(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id.clone(),
                    "repoPath": repo,
                    "profile": "claude",
                }),
            ),
            &host,
            &identity,
            dummy_agent_dispatcher,
            noop_after_session_created,
        )
        .await
        .expect_err("start without attached plan should fail");
        assert!(!blocked.ok);
        let message = blocked.error.as_deref().unwrap_or_default();
        assert!(message.contains("attached implementation plan"), "{message}");
        let stored = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(stored.status, roux_core::WorkItemStatus::Ready);
        assert_eq!(stored.start_error.as_deref(), Some(message));
        assert!(host.work_item_handle.list_runs(Some(&item_id)).unwrap().is_empty());
        assert!(host.session_handle.list().await.unwrap().is_empty());

        let forced = start_work_item_run_with_hooks(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id.clone(),
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
            dummy_agent_dispatcher,
            noop_after_session_created,
        )
        .await
        .expect("forceStart should bypass the attached-plan gate");
        let session_id = forced.run.session_id.clone().expect("forced start session id");

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_attach_session_command_binds_card() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;
        host.session_handle.add(session("sess-1")).await.unwrap();
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput { title: "Attach me".into(), ..Default::default() })
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-attach-session",
                serde_json::json!({ "id": item.id, "sessionId": "sess-1" }),
            ),
            &host,
            &identity,
        )
        .await;

        assert!(resp.ok, "attach failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["sessionId"], "sess-1");
        let item = host.work_item_handle.get(&item.id).unwrap().unwrap();
        assert_eq!(item.session_id.as_deref(), Some("sess-1"));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_attach_session_sets_blank_session_project_after_binding() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;
        host.session_handle.add(session("sess-1")).await.unwrap();
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Attach me".into(),
                project_id: Some("proj-1".into()),
                field_presence: roux_core::WorkItemInputPresence {
                    project_id: true,
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-attach-session",
                serde_json::json!({ "id": item.id, "sessionId": "sess-1" }),
            ),
            &host,
            &identity,
        )
        .await;

        assert!(resp.ok, "attach failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["sessionId"], "sess-1");
        assert_eq!(resp.data.as_ref().unwrap()["projectId"], "proj-1");
        let session = host.session_handle.get("sess-1").await.unwrap().unwrap();
        assert_eq!(session.project_id.as_deref(), Some("proj-1"));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_detach_session_command_clears_binding() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;
        host.session_handle.add(session("sess-1")).await.unwrap();
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput { title: "Detach me".into(), ..Default::default() })
            .unwrap();
        host.work_item_handle.attach_session(&item.id, "sess-1").unwrap().unwrap();

        let resp = handle_request(
            req("work-item-detach-session", serde_json::json!({ "id": item.id })),
            &host,
            &identity,
        )
        .await;

        assert!(resp.ok, "detach failed: {:?}", resp.error);
        assert!(resp.data.as_ref().unwrap()["sessionId"].is_null());
        let item = host.work_item_handle.get(&item.id).unwrap().unwrap();
        assert!(item.session_id.is_none());

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_uses_kanban_default_agent_profile() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run the agent" })),
            &host,
            &identity,
        )
        .await;
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = handle_request(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "start without card agent should use Kanban default");
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Doing);
        assert_eq!(item.agent_profile.as_deref(), Some("claude"));
        let session_id = item.session_id.clone().expect("started card session");
        let ptys = host.pty_handle.list().await.unwrap();
        let pty = ptys
            .iter()
            .find(|pty| pty.info.session_id.as_deref() == Some(session_id.as_str()))
            .expect("started card pty");
        assert_eq!(pty.kind, roux_runtime::pty_service::PtyKind::Task);
        assert!(
            pty.command
                .as_deref()
                .unwrap_or_default()
                .contains("Start work on this Roux board card."),
            "agent command should include seeded work-item prompt: {:?}",
            pty.command
        );
        let _ = host.pty_handle.kill(&session_id).await;

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_includes_latest_attached_plan_in_agent_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Implement workflow gate".into(),
                repo_path: Some(repo.to_string_lossy().into_owned()),
                agent_profile: Some("claude".into()),
                status: Some(roux_core::WorkItemStatus::Ready),
                field_presence: roux_core::WorkItemInputPresence {
                    repo_path: true,
                    agent_profile: true,
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
        let latest_plan = host
            .work_item_handle
            .create_attachment(roux_core::AttachmentInput {
                target_kind: roux_core::AttachmentTargetKind::WorkItem,
                target_id: item.id.clone(),
                title: Some("Implementation Plan".into()),
                content_kind: roux_core::AttachmentContentKind::Text,
                content: "Use the attached plan as the execution source of truth.".into(),
                mime_type: Some("text/markdown".into()),
                source_path: Some("/tmp/workflow-plan.md".into()),
            })
            .unwrap();

        let resp = handle_request(
            req("work-item-start", serde_json::json!({ "id": item.id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "start failed: {:?}", resp.error);
        let item = host.work_item_handle.get(&item.id).unwrap().unwrap();
        let session_id = item.session_id.clone().expect("started card session");
        let ptys = host.pty_handle.list().await.unwrap();
        let pty = ptys
            .iter()
            .find(|pty| pty.info.session_id.as_deref() == Some(session_id.as_str()))
            .expect("started card pty");
        let command = pty.command.as_deref().unwrap_or_default();
        assert!(command.contains("Approved implementation plan:"), "{command}");
        assert!(command.contains(&latest_plan.document_id), "{command}");
        assert!(
            command.contains("Use the attached plan as the execution source of truth."),
            "{command}"
        );
        let _ = host.pty_handle.kill(&session_id).await;

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_requires_repo_or_project() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req(
                "work-item-create",
                serde_json::json!({
                    "title": "Run the agent",
                    "agentProfile": "claude",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = handle_request(
            req("work-item-start", serde_json::json!({ "id": item_id })),
            &host,
            &identity,
        )
        .await;
        assert!(!resp.ok, "start without repo/project should fail");
        assert!(resp.error.as_deref().unwrap_or_default().contains("repoPath or project required"));
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Todo);
        assert_eq!(item.start_error.as_deref(), Some("repoPath or project required"));
        assert_eq!(item.agent_profile.as_deref(), Some("claude"));
        assert!(host.work_item_handle.list_runs(Some(&item_id)).unwrap().is_empty());
        assert!(host.session_handle.list().await.unwrap().is_empty());

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_rejects_non_autonomous_profile() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run the agent" })),
            &host,
            &identity,
        )
        .await;
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = handle_request(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "plain-shell",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(!resp.ok, "plain shell profile should not be startable");
        assert!(resp
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("autonomous Claude or Codex profile"));
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Todo);
        assert!(item
            .start_error
            .as_deref()
            .unwrap_or_default()
            .contains("autonomous Claude or Codex profile"));
        assert!(host.work_item_handle.list_runs(Some(&item_id)).unwrap().is_empty());
        assert!(host.session_handle.list().await.unwrap().is_empty());

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_prompt_dispatch_failure_preserves_session_and_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = start_work_item_run_with_hooks(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
            failing_profile_dispatcher,
            noop_after_session_created,
        )
        .await
        .expect_err("prompt dispatch failure should return an error response");
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("simulated prompt dispatch failure"));

        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Todo);
        assert_eq!(item.start_error.as_deref(), Some("simulated prompt dispatch failure"));
        let session_id = item.session_id.clone().expect("failed start should preserve session id");
        let worktree_path =
            item.worktree_path.clone().expect("failed start should preserve worktree path");
        assert!(worktree_path.contains("roux-card"));

        let runs = host.work_item_handle.list_runs(Some(&item_id)).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, roux_core::WorkItemRunStatus::Failed);
        assert_eq!(runs[0].session_id.as_deref(), Some(session_id.as_str()));
        assert_eq!(runs[0].worktree_path.as_deref(), Some(worktree_path.as_str()));

        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert_eq!(session.worktree_path, worktree_path);
        let ptys = host.pty_handle.list().await.unwrap();
        assert!(
            ptys.iter().any(|pty| pty.info.session_id.as_deref() == Some(session_id.as_str())),
            "failed start should keep the PTY available for inspection"
        );

        let events = host.work_item_handle.list_run_events(&runs[0].id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::Lifecycle
                && event.payload.get("stage").and_then(|stage| stage.as_str())
                    == Some("sessionCreated")
        }));
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::StatusChanged
                && event.payload.get("reason").and_then(|reason| reason.as_str())
                    == Some("promptDispatchFailed")
        }));

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn failed_agent_launch_cleanup_archives_session_and_ptys() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let response = handle_session_create_shell(
            Request {
                command: "session-create-shell".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "session-a",
                    "repoPath": dir.path(),
                    "name": "Daemon Session",
                    "profile": "plain-shell",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(response.ok, "session create failed: {:?}", response.error);
        let session_id = response.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        let ptys = host.pty_handle.list().await.unwrap();
        assert!(
            ptys.iter().any(|pty| pty.info.session_id.as_deref() == Some(session_id.as_str())),
            "session shell should create a primary PTY before cleanup"
        );

        let error =
            cleanup_failed_agent_launch_session(&host, &session_id, "simulated spawn failure")
                .await;
        assert!(error.contains("failed to launch agent task in session"));
        assert!(error.contains("session archived after failed agent launch"));
        let ptys = host.pty_handle.list().await.unwrap();
        assert!(
            !ptys.iter().any(|pty| pty.info.session_id.as_deref() == Some(session_id.as_str())),
            "failed agent launch cleanup should not leave dangling session PTYs"
        );
        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert!(session.archived);
        assert!(session.primary_pty_id.is_none());

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_race_does_not_bind_losing_session_to_card() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = start_work_item_run_with_hooks(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
            real_profile_dispatcher,
            create_active_run_after_session_created,
        )
        .await
        .expect_err("losing start should fail when another run wins the race");
        assert!(!resp.ok);
        assert!(resp
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("active work item run already exists"));

        let sessions = host.session_handle.list().await.unwrap();
        assert_eq!(sessions.len(), 1, "losing start session should be preserved");
        let losing_session_id = sessions[0].id.clone();
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_ne!(item.session_id.as_deref(), Some(losing_session_id.as_str()));
        assert_ne!(item.worktree_path.as_deref(), Some(sessions[0].worktree_path.as_str()));
        assert!(item.start_error.is_none());

        let runs = host.work_item_handle.list_runs(Some(&item_id)).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].session_id.as_deref(), Some("winning-session"));
        assert_eq!(runs[0].status, roux_core::WorkItemRunStatus::Starting);

        let _ = host.pty_handle.kill(&losing_session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_forwards_session_target_args() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let worktree = dir.path().join("existing-worktree");
        std::fs::create_dir_all(&worktree).unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Original card title" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = handle_request(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "name": "Prompt-selected name",
                    "worktreePath": worktree,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "start failed: {:?}", resp.error);
        let session_id = resp.data.as_ref().unwrap()["session"]["id"].as_str().unwrap().to_string();

        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert_eq!(session.name, "Prompt-selected name");
        assert_eq!(session.worktree_path, worktree.to_string_lossy());

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_start_returns_run_and_lists_it() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = handle_request(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "start failed: {:?}", resp.error);
        let run_id = resp.data.as_ref().unwrap()["run"]["id"].as_str().unwrap().to_string();
        let session_id =
            resp.data.as_ref().unwrap()["run"]["sessionId"].as_str().unwrap().to_string();
        assert_eq!(resp.data.as_ref().unwrap()["run"]["workItemId"], item_id);
        assert_eq!(resp.data.as_ref().unwrap()["run"]["status"], "running");

        let resp = handle_request(
            req("work-item-runs-list", serde_json::json!({ "workItemId": item_id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let runs = resp.data.as_ref().unwrap().as_array().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["id"], run_id);

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_plan_creates_session_and_moves_card_to_planning() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req(
                "work-item-create",
                serde_json::json!({ "title": "Clarify card", "body": "Need a plan first" }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let resp = plan_work_item_run_with_hooks(
            req(
                "work-item-plan",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
            successful_profile_dispatcher,
        )
        .await
        .expect("work-item-plan should succeed");
        let run_id = resp.run.id.clone();
        let session_id = resp.run.session_id.clone().expect("planning run session id");
        assert_eq!(resp.run.kind, roux_core::WorkItemRunKind::Planning);
        assert_eq!(resp.run.status, roux_core::WorkItemRunStatus::Running);
        assert_eq!(resp.item.status, roux_core::WorkItemStatus::Ready);
        assert!(resp.item.session_id.is_none());
        assert_eq!(resp.item.branch.as_deref(), resp.run.branch.as_deref());
        assert_eq!(resp.item.worktree_path.as_deref(), resp.run.worktree_path.as_deref());
        let planned_branch = resp.item.branch.as_deref().unwrap();
        assert!(planned_branch.starts_with("roux/card-"));
        assert!(planned_branch.ends_with("-clarify-card"));
        assert!(resp.item.worktree_path.as_deref().unwrap().contains("roux-card"));
        assert_eq!(resp.session.id, session_id);
        assert_eq!(resp.session.name, "Clarify card");

        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Ready);
        assert!(item.session_id.is_none());
        assert_eq!(item.branch.as_deref(), resp.run.branch.as_deref());
        assert_eq!(item.worktree_path.as_deref(), resp.run.worktree_path.as_deref());
        let events = host.work_item_handle.list_run_events(&run_id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::Lifecycle
                && event.payload.get("stage").and_then(|stage| stage.as_str())
                    == Some("promptDispatched")
        }));

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_plan_reuses_active_planning_run() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Clarify card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let first = plan_work_item_run_with_hooks(
            req(
                "work-item-plan",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
            successful_profile_dispatcher,
        )
        .await
        .expect("first plan should succeed");
        let first_run_id = first.run.id.clone();
        let first_session_id = first.run.session_id.clone().expect("planning run session id");

        let second = plan_work_item_run_with_hooks(
            req(
                "work-item-plan",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                }),
            ),
            &host,
            &identity,
            successful_profile_dispatcher,
        )
        .await
        .expect("second plan should reuse active planning run");
        assert_eq!(second.run.id, first_run_id);
        assert_eq!(second.run.session_id.as_deref(), Some(first_session_id.as_str()));

        let runs = host.work_item_handle.list_runs(Some(&item_id)).unwrap();
        assert_eq!(runs.len(), 1);
        let sessions = host.session_handle.list().await.unwrap();
        assert_eq!(sessions.len(), 1);

        let _ = host.pty_handle.kill(&first_session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_plan_replace_active_stops_existing_planning_run() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Clarify card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let first = plan_work_item_run_with_hooks(
            req(
                "work-item-plan",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                }),
            ),
            &host,
            &identity,
            successful_profile_dispatcher,
        )
        .await
        .expect("first plan should succeed");
        let first_run_id = first.run.id.clone();
        let first_session_id = first.run.session_id.clone().expect("planning run session id");

        let second = plan_work_item_run_with_hooks(
            req(
                "work-item-plan",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "replaceActive": true,
                }),
            ),
            &host,
            &identity,
            successful_profile_dispatcher,
        )
        .await
        .expect("replacement plan should succeed");
        let second_session_id = second.run.session_id.clone().expect("replacement session id");
        assert_ne!(second.run.id, first_run_id);
        assert_ne!(second_session_id, first_session_id);

        let first_run = host.work_item_handle.get_run(&first_run_id).unwrap().unwrap();
        assert_eq!(first_run.status, roux_core::WorkItemRunStatus::Stopped);
        let first_session = host.session_handle.get(&first_session_id).await.unwrap().unwrap();
        assert!(first_session.archived);

        let runs = host.work_item_handle.list_runs(Some(&item_id)).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs.iter().filter(|run| run.status == roux_core::WorkItemRunStatus::Running).count(),
            1
        );

        let _ = host.pty_handle.kill(&second_session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_run_output_detects_decision_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let (run_id, session_id, _) =
            start_agent_work_item(&host, &identity, &repo, &item_id).await;

        let line = r#"{"type":"decision","question":"Choose path?","options":[{"value":"existing","label":"Use existing"},{"value":"new","label":"Create new"}],"defaultValue":"existing"}"#;
        let mut parser = WorkItemRunOutputParser::default();
        ingest_work_item_run_output(&host, &run_id, 0, format!("{line}\n").as_bytes(), &mut parser);

        let mut decisions = Vec::new();
        for _ in 0..40 {
            decisions = host.work_item_handle.list_pending_decisions(Some(&item_id)).unwrap();
            if !decisions.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        assert_eq!(decisions.len(), 1, "pending decision should be detected from output");
        assert_eq!(decisions[0].run_id, run_id);
        assert_eq!(decisions[0].question, "Choose path?");
        assert_eq!(decisions[0].default_value.as_deref(), Some("existing"));

        let run = host.work_item_handle.get_run(&run_id).unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Blocked);

        let events = host.work_item_handle.list_run_events(&run_id).unwrap();
        assert!(events.iter().any(|event| event.kind == roux_core::WorkItemRunEventKind::Text));
        assert!(events.iter().any(|event| event.kind == roux_core::WorkItemRunEventKind::Decision));

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_run_requests_review_on_zero_pty_exit() {
        let dir = tempfile::tempdir().unwrap();
        let (host, _identity, joins) = make_host_and_identity(&dir).await;

        let (run_id, _pty_id) =
            spawn_task_work_item_run(&host, dir.path(), "exit 0", "task-exit-zero").await;

        let run = wait_for_run_status(&host, &run_id, roux_core::WorkItemRunStatus::Review).await;

        assert!(run.ended_at.is_some());
        let item = host.work_item_handle.get(&run.work_item_id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Review);
        let events = host.work_item_handle.list_run_events(&run_id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::StatusChanged
                && event.payload["status"] == "review"
                && event.payload["reason"] == "ptyExit"
                && event.payload["reviewRequested"] == true
        }));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_review_request_moves_run_and_card_to_review() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Request review".into(),
                status: Some(roux_core::WorkItemStatus::Doing),
                ..Default::default()
            })
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some("sess-1"), Some("claude"), Some("claude"), None, None)
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-review-request",
                serde_json::json!({
                    "runId": run.id,
                    "summary": "Implemented review package.",
                    "tests": ["npm run test"],
                    "changedFiles": ["src/lib/workItems/reviewPackage.ts"],
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "review request failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["item"]["status"], "review");
        assert_eq!(resp.data.as_ref().unwrap()["run"]["status"], "review");

        let item = host.work_item_handle.get(&item.id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Review);
        let run = host.work_item_handle.get_run(&run.id).unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Review);
        let events = host.work_item_handle.list_run_events(&run.id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::StatusChanged
                && event.payload["status"] == "review"
                && event.payload["reason"] == "reviewRequested"
                && event.payload["requestedBy"] == "agent"
        }));
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::Result
                && event.payload["summary"] == "Implemented review package."
                && event.payload["tests"][0] == "npm run test"
                && event.payload["changedFiles"][0] == "src/lib/workItems/reviewPackage.ts"
        }));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_review_request_rejects_planning_run() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput { title: "Plan only".into(), ..Default::default() })
            .unwrap();
        let run = host
            .work_item_handle
            .create_planning_run(
                &item.id,
                Some("sess-1"),
                Some("claude"),
                Some("claude"),
                None,
                None,
            )
            .unwrap();

        let resp = handle_request(
            req("work-item-review-request", serde_json::json!({ "runId": run.id })),
            &host,
            &identity,
        )
        .await;
        assert!(!resp.ok);
        assert!(resp.error.as_deref().unwrap_or_default().contains("review can only be requested"));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_review_request_changes_moves_card_back_and_attaches_feedback() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Review me".into(),
                status: Some(roux_core::WorkItemStatus::Review),
                ..Default::default()
            })
            .unwrap();
        host.work_item_handle.attach_session(&item.id, "sess-1").unwrap().unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some("sess-1"), Some("claude"), Some("claude"), None, None)
            .unwrap();
        host.work_item_handle
            .set_run_status(
                &run.id,
                roux_core::WorkItemRunStatus::Review,
                serde_json::json!({ "reason": "test" }),
            )
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-review-request-changes",
                serde_json::json!({
                    "runId": run.id,
                    "note": "Please add retry coverage.",
                    "status": "ready",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "request changes failed: {:?}", resp.error);
        let data = resp.data.as_ref().unwrap();
        assert_eq!(data["item"]["status"], "ready");
        assert!(data["item"]["sessionId"].is_null());
        assert_eq!(data["run"]["status"], "changesRequested");
        assert_eq!(data["attachment"]["targetKind"], "workItem");
        assert_eq!(data["attachment"]["targetId"], item.id);
        let document_id = data["attachment"]["documentId"].as_str().unwrap();
        let document = host
            .work_item_handle
            .get_attachment_document(document_id)
            .unwrap()
            .expect("feedback attachment document");
        assert_eq!(document.content, "Please add retry coverage.");

        let item = host.work_item_handle.get(&item.id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Ready);
        assert!(item.session_id.is_none());
        assert!(!host.work_item_handle.has_active_run(&item.id).unwrap());
        let run = host.work_item_handle.get_run(&run.id).unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::ChangesRequested);
        assert_eq!(run.session_id.as_deref(), Some("sess-1"));
        let events = host.work_item_handle.list_run_events(&run.id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::StatusChanged
                && event.payload["status"] == "changes_requested"
                && event.payload["reason"] == "changesRequested"
                && event.payload["targetStatus"] == "ready"
                && event.payload["feedbackDocumentId"] == document_id
        }));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_review_feedback_prompt_context_uses_custom_titled_feedback() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Review me".into(),
                status: Some(roux_core::WorkItemStatus::Review),
                ..Default::default()
            })
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some("sess-1"), Some("claude"), Some("claude"), None, None)
            .unwrap();
        host.work_item_handle
            .set_run_status(
                &run.id,
                roux_core::WorkItemRunStatus::Review,
                serde_json::json!({ "reason": "test" }),
            )
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-review-request-changes",
                serde_json::json!({
                    "runId": run.id,
                    "title": "Retry path notes",
                    "note": "Please add retry coverage.",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "request changes failed: {:?}", resp.error);

        let feedback = load_work_item_review_feedback_prompt_context(&host, &item.id)
            .expect("custom-titled feedback should reload for the next prompt");

        assert_eq!(feedback.title, "Retry path notes");
        assert_eq!(feedback.content, "Please add retry coverage.");

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_review_accept_moves_run_and_card_to_done() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Review me".into(),
                status: Some(roux_core::WorkItemStatus::Review),
                ..Default::default()
            })
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some("sess-1"), Some("claude"), Some("claude"), None, None)
            .unwrap();
        host.work_item_handle
            .set_run_status(
                &run.id,
                roux_core::WorkItemRunStatus::Review,
                serde_json::json!({ "reason": "test" }),
            )
            .unwrap();

        let resp = handle_request(
            req("work-item-review-accept", serde_json::json!({ "id": item.id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "review accept failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["item"]["status"], "done");
        assert_eq!(resp.data.as_ref().unwrap()["run"]["status"], "done");

        let item = host.work_item_handle.get(&item.id).unwrap().unwrap();
        assert_eq!(item.status, roux_core::WorkItemStatus::Done);
        let run = host.work_item_handle.get_run(&run.id).unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Done);
        let events = host.work_item_handle.list_run_events(&run.id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::StatusChanged
                && event.payload["status"] == "done"
                && event.payload["reason"] == "reviewAccepted"
        }));

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_run_marks_failed_on_nonzero_pty_exit() {
        let dir = tempfile::tempdir().unwrap();
        let (host, _identity, joins) = make_host_and_identity(&dir).await;

        let (run_id, _pty_id) =
            spawn_task_work_item_run(&host, dir.path(), "exit 7", "task-exit-nonzero").await;

        let run = wait_for_run_status(&host, &run_id, roux_core::WorkItemRunStatus::Failed).await;

        assert!(run.ended_at.is_some());
        let events = host.work_item_handle.list_run_events(&run_id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::StatusChanged
                && event.payload["status"] == "failed"
                && event.payload["exitCode"] == 7
        }));

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn record_work_item_run_pty_exit_does_not_overwrite_stopped_runs() {
        let dir = tempfile::tempdir().unwrap();
        let (host, _identity, joins) = make_host_and_identity(&dir).await;
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput { title: "Task".into(), ..Default::default() })
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some("sess-1"), None, None, None, None)
            .unwrap();
        host.work_item_handle
            .set_run_status(
                &run.id,
                roux_core::WorkItemRunStatus::Stopped,
                serde_json::json!({ "reason": "user" }),
            )
            .unwrap();

        record_work_item_run_pty_exit(&host, &run.id, Some(0), 1);

        let run = host.work_item_handle.get_run(&run.id).unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Stopped);
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_run_stop_archives_session_and_records_event() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let (run_id, session_id, _) =
            start_agent_work_item(&host, &identity, &repo, &item_id).await;

        let resp = handle_request(
            req("work-item-run-stop", serde_json::json!({ "runId": run_id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "run stop failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["status"], "stopped");
        assert!(resp.data.as_ref().unwrap()["endedAt"].is_number());

        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert!(session.archived);
        assert!(session.primary_pty_id.is_none());
        assert!(host.pty_handle.snapshot(&session_id, 64).await.unwrap().is_none());

        let resp = handle_request(
            req("work-item-run-events", serde_json::json!({ "runId": run_id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let events = resp.data.as_ref().unwrap().as_array().unwrap();
        assert!(events.iter().any(|event| {
            event["kind"] == "statusChanged" && event["payload"]["status"] == "stopped"
        }));

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_run_stop_keeps_shared_session_for_active_sibling_run() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let (first_run_id, session_id, first_data) =
            start_agent_work_item(&host, &identity, &repo, &item_id).await;
        let first_pty_id =
            first_data["run"]["ptyId"].as_str().expect("first run pty id").to_string();

        let result = start_work_item_run_with_hooks(
            req(
                "work-item-start",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": repo,
                    "profile": "claude",
                    "forceStart": true,
                }),
            ),
            &host,
            &identity,
            dummy_agent_dispatcher,
            noop_after_session_created,
        )
        .await
        .expect("second start should succeed");
        let second_data = serde_json::to_value(&result).expect("serialize second start result");
        let second_run_id = second_data["run"]["id"].as_str().unwrap().to_string();
        let second_pty_id = second_data["run"]["ptyId"].as_str().unwrap().to_string();

        let resp = handle_request(
            req("work-item-run-stop", serde_json::json!({ "runId": second_run_id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "run stop failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["status"], "stopped");

        let first_run = host.work_item_handle.get_run(&first_run_id).unwrap().unwrap();
        assert!(is_active_work_item_run(&first_run));
        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert!(!session.archived);
        assert!(host.pty_handle.snapshot(&first_pty_id, 64).await.unwrap().is_some());
        assert!(host.pty_handle.snapshot(&second_pty_id, 64).await.unwrap().is_none());

        let _ = host.pty_handle.kill(&first_pty_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_run_stop_retries_cleanup_for_already_stopped_run() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let (run_id, session_id, _) =
            start_agent_work_item(&host, &identity, &repo, &item_id).await;

        host.work_item_handle
            .set_run_status(
                &run_id,
                roux_core::WorkItemRunStatus::Stopped,
                serde_json::json!({ "reason": "preexisting" }),
            )
            .unwrap();

        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert!(!session.archived);
        assert!(host.pty_handle.snapshot(&session_id, 64).await.unwrap().is_some());

        let resp = handle_request(
            req("work-item-run-stop", serde_json::json!({ "runId": run_id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "run stop failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["status"], "stopped");

        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert!(session.archived);
        assert!(session.primary_pty_id.is_none());
        assert!(host.pty_handle.snapshot(&session_id, 64).await.unwrap().is_none());

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn terminal_work_item_run_stop_response_cleans_stopped_run_session() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Run card" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let (run_id, session_id, _) =
            start_agent_work_item(&host, &identity, &repo, &item_id).await;

        host.work_item_handle
            .set_run_status(
                &run_id,
                roux_core::WorkItemRunStatus::Stopped,
                serde_json::json!({ "reason": "preexisting" }),
            )
            .unwrap();
        let run = host.work_item_handle.get_run(&run_id).unwrap().unwrap();

        let resp = terminal_work_item_run_stop_response(&host, run).await;
        assert!(resp.ok, "terminal run response failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["status"], "stopped");

        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert!(session.archived);
        assert!(host.pty_handle.snapshot(&session_id, 64).await.unwrap().is_none());

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_decision_commands_persist_and_resolve_with_events() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Needs decision".into(),
                ..Default::default()
            })
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some("sess-1"), None, None, None, None)
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-decision-create",
                serde_json::json!({
                    "runId": run.id,
                    "question": "Choose path?",
                    "options": [
                        { "value": "existing", "label": "Use existing" },
                        { "value": "new", "label": "Create new" }
                    ],
                    "defaultValue": "existing",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "decision create failed: {:?}", resp.error);
        let decision_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        assert_eq!(resp.data.as_ref().unwrap()["status"], "pending");

        let resp = handle_request(
            req("work-item-decisions-list", serde_json::json!({ "workItemId": item.id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        assert_eq!(resp.data.as_ref().unwrap().as_array().unwrap().len(), 1);

        let resp = handle_request(
            req(
                "work-item-decision-resolve",
                serde_json::json!({
                    "id": decision_id,
                    "value": "new",
                    "resolvedBy": "test",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "decision resolve failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["status"], "resolved");
        assert_eq!(resp.data.as_ref().unwrap()["resolvedValue"], "new");

        let resp = handle_request(
            req("work-item-run-events", serde_json::json!({ "runId": run.id })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let events = resp.data.as_ref().unwrap().as_array().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["kind"], "decision");
        assert_eq!(events[1]["kind"], "decisionResolved");
        assert_eq!(events[2]["kind"], "error");
        assert_eq!(events[2]["payload"]["stage"], "decisionResolutionWrite");

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_decision_resolve_writes_choice_to_run() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput { title: "Needs answer".into(), ..Default::default() })
            .unwrap();
        let session_id = "decision-session";
        let pty_id = "decision-resolve-pty";
        host.pty_handle
            .spawn_task(
                "cat".into(),
                PtySpawnRequest {
                    id: Some(pty_id.into()),
                    working_dir: Some(dir.path().to_path_buf()),
                    session_id: Some(session_id.into()),
                    profile: Some("task".into()),
                    initial_size: Some((80, 24)),
                    ..PtySpawnRequest::default()
                },
            )
            .await
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some(session_id), None, None, None, None)
            .unwrap();
        host.work_item_handle.set_run_pty_id(&run.id, Some(pty_id)).unwrap();
        let decision = host
            .work_item_handle
            .create_decision(
                &run.id,
                "Choose path?",
                vec![
                    roux_core::WorkItemDecisionOption {
                        value: "existing".into(),
                        label: "Use existing".into(),
                    },
                    roux_core::WorkItemDecisionOption {
                        value: "new".into(),
                        label: "Create new".into(),
                    },
                ],
                Some("existing"),
                None,
            )
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-decision-resolve",
                serde_json::json!({
                    "id": decision.id,
                    "value": "new",
                    "resolvedBy": "test",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "decision resolve failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["status"], "resolved");

        let mut output = String::new();
        for _ in 0..40 {
            let snapshot = host.pty_handle.snapshot(pty_id, 1024).await.unwrap().unwrap();
            output = snapshot.output;
            if output.contains("new") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(output.contains("new"), "resolved choice should be written to PTY: {output:?}");

        let _ = host.pty_handle.kill(pty_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_decision_timeout_writes_default_to_run() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;
        let item = host
            .work_item_handle
            .create(roux_core::WorkItemInput {
                title: "Needs timeout".into(),
                ..Default::default()
            })
            .unwrap();
        let pty_id = "decision-timeout-pty";
        host.pty_handle
            .spawn_task(
                "cat".into(),
                PtySpawnRequest {
                    id: Some(pty_id.into()),
                    working_dir: Some(dir.path().to_path_buf()),
                    session_id: Some(pty_id.into()),
                    profile: Some("task".into()),
                    initial_size: Some((80, 24)),
                    ..PtySpawnRequest::default()
                },
            )
            .await
            .unwrap();
        let run = host
            .work_item_handle
            .create_run(&item.id, Some(pty_id), None, None, None, None)
            .unwrap();

        let resp = handle_request(
            req(
                "work-item-decision-create",
                serde_json::json!({
                    "runId": run.id,
                    "question": "Use default?",
                    "options": [{ "value": "yes", "label": "Yes" }],
                    "defaultValue": "yes",
                    "timeoutSeconds": 0,
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "decision create failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["status"], "timedOut");
        assert_eq!(resp.data.as_ref().unwrap()["resolvedValue"], "yes");

        let mut output = String::new();
        for _ in 0..40 {
            let snapshot = host.pty_handle.snapshot(pty_id, 1024).await.unwrap().unwrap();
            output = snapshot.output;
            if output.contains("yes") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(output.contains("yes"), "timed-out default should be written to PTY: {output:?}");

        let events = host.work_item_handle.list_run_events(&run.id).unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == roux_core::WorkItemRunEventKind::DecisionTimedOut));

        let _ = host.pty_handle.kill(pty_id).await;
        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_events_stream_emits_ready_then_events() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        // Run the event stream against a duplex buffer
        let (mut client, mut server) = tokio::io::duplex(4096);

        // Drive the stream handler in the background
        let host_clone = host.clone();
        let identity_clone = identity.clone();
        let stream_task = tokio::spawn(async move {
            handle_work_item_events_stream(
                req("work-item-events", serde_json::json!({})),
                &mut server,
                &host_clone,
                &identity_clone,
            )
            .await
        });

        // Use a single BufReader so buffered data between reads is not lost.
        let mut client_reader = tokio::io::BufReader::new(&mut client);

        let mut buf = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut client_reader, &mut buf).await.unwrap();
        let frame: serde_json::Value = serde_json::from_str(buf.trim()).unwrap();
        assert_eq!(frame["type"], "ready");

        // Trigger a create so the stream emits an event frame
        handle_request(
            req("work-item-create", serde_json::json!({ "title": "Stream test item" })),
            &host,
            &identity,
        )
        .await;

        let mut buf2 = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut client_reader, &mut buf2).await.unwrap();
        let event_frame: serde_json::Value = serde_json::from_str(buf2.trim()).unwrap();
        assert_eq!(event_frame["type"], "event");

        stream_task.abort();
        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_import_inline_items() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-import", serde_json::json!({
                "items": [
                    { "title": "Task A", "externalRef": { "provider": "gh", "externalId": "1" } },
                    { "title": "Task B", "externalRef": { "provider": "gh", "externalId": "2" } },
                    { "title": "Task C" },
                ]
            })),
            &host, &identity,
        ).await;
        assert!(resp.ok, "import failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["imported"], 3);

        let items = host.work_item_handle.list(None).unwrap();
        assert_eq!(items.len(), 3);

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_import_deduplicates_on_reimport() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        // First import
        handle_request(
            req("work-item-import", serde_json::json!({
                "items": [{ "title": "Old title", "externalRef": { "provider": "gh", "externalId": "42" } }]
            })),
            &host, &identity,
        ).await;

        // Re-import same externalId with updated title
        let resp = handle_request(
            req("work-item-import", serde_json::json!({
                "items": [{ "title": "New title", "externalRef": { "provider": "gh", "externalId": "42" } }]
            })),
            &host, &identity,
        ).await;
        assert!(resp.ok);

        let items = host.work_item_handle.list(None).unwrap();
        assert_eq!(items.len(), 1, "no duplicates on re-import");
        assert_eq!(items[0].title, "New title");

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_import_resolves_parent_external_id() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let resp = handle_request(
            req("work-item-import", serde_json::json!({
                "items": [
                    { "title": "Epic", "externalRef": { "provider": "gh", "externalId": "100" } },
                    {
                        "title": "Sub-task",
                        "externalRef": { "provider": "gh", "externalId": "101" },
                        "parentExternalId": "100"
                    }
                ]
            })),
            &host, &identity,
        ).await;
        assert!(resp.ok, "import failed: {:?}", resp.error);

        let items = host.work_item_handle.list(None).unwrap();
        let epic = items.iter().find(|i| i.external_id.as_deref() == Some("100")).unwrap();
        let subtask = items.iter().find(|i| i.external_id.as_deref() == Some("101")).unwrap();
        assert_eq!(subtask.parent_id.as_deref(), Some(epic.id.as_str()), "parent_id resolved");

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_import_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let import_file = dir.path().join("import.json");
        std::fs::write(
            &import_file,
            serde_json::json!({
                "items": [
                    { "title": "From file A" },
                    { "title": "From file B" },
                ]
            })
            .to_string(),
        )
        .unwrap();

        let resp = handle_request(
            req("work-item-import", serde_json::json!({ "path": import_file.to_string_lossy() })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "file import failed: {:?}", resp.error);
        assert_eq!(resp.data.as_ref().unwrap()["imported"], 2);

        shutdown_host(host, joins).await;
    }

    #[tokio::test]
    async fn daemon_work_item_import_broadcasts_imported_event() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        let mut rx = host.work_item_handle.subscribe_events();

        handle_request(
            req(
                "work-item-import",
                serde_json::json!({
                    "items": [{ "title": "Imported" }]
                }),
            ),
            &host,
            &identity,
        )
        .await;

        // Drain until we find the Imported event (there may be a Created event first)
        let mut found = false;
        for _ in 0..5 {
            match rx.try_recv() {
                Ok(roux_core::WorkItemEvent::Imported { ids }) => {
                    assert_eq!(ids.len(), 1);
                    found = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        assert!(found, "Imported event should be broadcast");

        shutdown_host(host, joins).await;
    }
}
