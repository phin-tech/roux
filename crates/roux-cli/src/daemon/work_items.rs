use serde::Serialize;
use tokio::io::AsyncWriteExt;

use roux_runtime::host::RuntimeHost;
use roux_runtime::pty_service::{PtyOutputEvent, PTY_OUTPUT_DEFAULT_POLL_BYTES};

use super::identity::{request_authorized, DaemonIdentity};
use super::protocol::{Request, Response};
use super::{
    bool_arg, handle_session_create_shell, kill_session_ptys, load_daemon_settings,
    optional_nullable_string_arg, optional_string_arg,
};

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum WorkItemEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: roux_core::WorkItemEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
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
        project_id: optional_string_arg(&req.args, &["projectId", "project_id"]),
        parent_id: optional_string_arg(&req.args, &["parentId", "parent_id"]),
        external_ref: None,
        sort_order: req
            .args
            .get("sortOrder")
            .or_else(|| req.args.get("sort_order"))
            .and_then(|v| v.as_f64()),
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
        project_id: optional_string_arg(&req.args, &["projectId", "project_id"]),
        parent_id: optional_string_arg(&req.args, &["parentId", "parent_id"]),
        external_ref: None,
        sort_order: req
            .args
            .get("sortOrder")
            .or_else(|| req.args.get("sort_order"))
            .and_then(|v| v.as_f64()),
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
        .unwrap_or(0.0);
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

pub(super) async fn handle_work_item_dispatch(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    match dispatch_work_item_run(req, host, identity).await {
        Ok(result) => Response::success(result.session),
        Err(resp) => resp,
    }
}

pub(super) async fn handle_work_item_run_dispatch(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    match dispatch_work_item_run(req, host, identity).await {
        Ok(result) => match serde_json::to_value(&result.run) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item run: {err}")),
        },
        Err(resp) => resp,
    }
}

struct WorkItemRunDispatch {
    session: serde_json::Value,
    run: roux_core::WorkItemRun,
}

async fn dispatch_work_item_run(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Result<WorkItemRunDispatch, Response> {
    let Some(item_id) = optional_string_arg(&req.args, &["id"]) else {
        return Err(Response::err("id required"));
    };
    let item = match host.work_item_handle.get(&item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return Err(Response::err("work item not found")),
        Err(err) => return Err(Response::err(err)),
    };

    // Resolve repo_path: explicit arg -> project.repo_roots[0] -> error
    let repo_path = if let Some(path) = optional_string_arg(&req.args, &["repoPath", "repo_path"]) {
        path
    } else if let Some(pid) = &item.project_id {
        match host.project_handle.get(pid).await {
            Ok(Some(project)) => match project.repo_roots.into_iter().next() {
                Some(root) => root,
                None => return Err(Response::err("project has no repo_roots")),
            },
            Ok(None) => return Err(Response::err("project not found")),
            Err(err) => return Err(Response::err(err.to_string())),
        }
    } else {
        return Err(Response::err("repoPath required (work item has no project)"));
    };

    let name = optional_string_arg(&req.args, &["name"]).unwrap_or_else(|| item.title.clone());
    let worktree_path = optional_nullable_string_arg(&req.args, &["worktreePath", "worktree_path"]);
    let branch =
        optional_nullable_string_arg(&req.args, &["branch", "worktreeBranch", "worktree_branch"]);
    let base = optional_nullable_string_arg(&req.args, &["base", "startPoint", "start_point"]);
    let fetch_first = bool_arg(&req.args, &["fetchFirst", "fetch_first"]);

    // Default to the built-in Claude agent when the caller didn't pick a
    // profile, so a dispatched card actually runs an agent (not a bare shell).
    // Callers (incl. the desktop board) can override via the `profile` arg.
    let profile_id =
        optional_string_arg(&req.args, &["profile"]).unwrap_or_else(|| "claude".to_string());

    let mut session_args = serde_json::json!({
        "repoPath": repo_path,
        "name": name,
        "projectId": item.project_id,
        "profile": profile_id,
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
        return Err(session_resp);
    }

    let session_id = session_resp
        .data
        .as_ref()
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let Some(session_id) = session_id else {
        return Err(Response::err("session created but id missing from response"));
    };

    match host.work_item_handle.set_session(&item_id, &session_id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = host.session_handle.remove(&session_id).await;
            return Err(Response::err("work item was removed; session was rolled back"));
        }
        Err(err) => {
            let _ = host.session_handle.remove(&session_id).await;
            return Err(Response::err(format!("set_session failed, session rolled back: {err}")));
        }
    }

    let session = host.session_handle.get(&session_id).await.ok().flatten();
    let settings = load_daemon_settings();
    let provider = roux_core::providers::resolve_profile(&profile_id, &settings)
        .and_then(|profile| provider_slug(profile.provider).map(str::to_string));
    let worktree_path = session.as_ref().map(|s| s.worktree_path.as_str());
    let branch =
        session.as_ref().map(|s| s.branch.as_str()).filter(|branch| !branch.trim().is_empty());
    let run = match host.work_item_handle.create_run(
        &item_id,
        Some(&session_id),
        provider.as_deref(),
        Some(&profile_id),
        worktree_path,
        branch,
    ) {
        Ok(run) => run,
        Err(err) => {
            let _ = host.session_handle.remove(&session_id).await;
            return Err(Response::err(format!("create run failed, session rolled back: {err}")));
        }
    };
    match host.work_item_handle.move_item(
        &item_id,
        roux_core::WorkItemStatus::Doing,
        item.sort_order,
    ) {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = host.session_handle.remove(&session_id).await;
            return Err(Response::err("work item was removed; session was rolled back"));
        }
        Err(err) => {
            let _ = host.session_handle.remove(&session_id).await;
            return Err(Response::err(format!(
                "move work item failed, session rolled back: {err}"
            )));
        }
    }

    start_work_item_run_output_monitor(host.clone(), run.id.clone(), session_id.clone());

    // Bring the agent to life in the now-bound session. Best-effort: the
    // session is already created + bound, so a failure here just leaves a
    // shell prompt rather than failing the dispatch.
    run_dispatched_profile(host, &item, &session_id, &profile_id).await;

    Ok(WorkItemRunDispatch { session: session_resp.data.unwrap_or(serde_json::Value::Null), run })
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
            | roux_core::WorkItemRunStatus::Done
            | roux_core::WorkItemRunStatus::Failed
    ) {
        return match serde_json::to_value(run) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item run: {err}")),
        };
    }

    if let Some(session_id) = run.session_id.as_deref() {
        kill_session_ptys(host, session_id).await;
        if let Err(err) = host.session_handle.archive(session_id).await {
            return Response::err(err.to_string());
        }
    }

    match host.work_item_handle.set_run_status(
        &run_id,
        roux_core::WorkItemRunStatus::Stopped,
        serde_json::json!({
            "reason": "user",
            "sessionId": run.session_id,
        }),
    ) {
        Ok(Some(run)) => match serde_json::to_value(run) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize work item run: {err}")),
        },
        Ok(None) => Response::err("work item run not found"),
        Err(err) => Response::err(err),
    }
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

    let input = format!("{value}\n");
    if let Err(err) = host.pty_handle.write(session_id, input.into_bytes()).await {
        let _ = host.work_item_handle.append_run_event(
            &run.id,
            roux_core::WorkItemRunEventKind::Error,
            serde_json::json!({
                "decisionId": decision.id,
                "message": format!("failed to write resolved decision to session: {err}"),
                "sessionId": session_id,
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
    let status = match code {
        Some(0) => roux_core::WorkItemRunStatus::Done,
        _ => roux_core::WorkItemRunStatus::Failed,
    };
    let payload = serde_json::json!({
        "reason": "ptyExit",
        "exitCode": code,
        "generation": generation,
    });

    match host.work_item_handle.get_run(run_id) {
        Ok(Some(run))
            if matches!(
                run.status,
                roux_core::WorkItemRunStatus::Stopped
                    | roux_core::WorkItemRunStatus::Done
                    | roux_core::WorkItemRunStatus::Failed
            ) => {}
        Ok(Some(_)) => {
            let _ = host.work_item_handle.set_run_status(run_id, status, payload);
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
/// so the card runs an agent headlessly. Best-effort and silent on failure.
async fn run_dispatched_profile(
    host: &RuntimeHost,
    item: &roux_core::WorkItem,
    session_id: &str,
    profile_id: &str,
) {
    let settings = load_daemon_settings();
    let Some(profile) = roux_core::providers::resolve_profile(profile_id, &settings) else {
        return;
    };

    let append = render_dispatch_project_prompt(host, item, session_id, &profile, &settings).await;
    let append_opt = (!append.trim().is_empty()).then_some(append.as_str());
    let task_prompt = render_work_item_task_prompt(item);
    let task_opt = (!task_prompt.trim().is_empty()).then_some(task_prompt.as_str());

    if let Some(input) = roux_core::providers::profile_startup_input_with_initial_task(
        &profile, append_opt, task_opt,
    ) {
        let _ = host.pty_handle.write(session_id, input.into_bytes()).await;
    }
}

fn render_work_item_task_prompt(item: &roux_core::WorkItem) -> String {
    let title = sanitize_card_prompt_field(&item.title);
    let body = item.body.as_deref().map(sanitize_card_prompt_field).unwrap_or_default();
    let external_url =
        item.external_url.as_deref().map(sanitize_card_prompt_field).unwrap_or_default();

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
    prompt.push_str(
        "\n\nInspect the repository, make the necessary changes, and report progress in this session.",
    );
    prompt
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
            project_id: optional_string_arg(item_val, &["projectId", "project_id"]),
            parent_id: None, // resolved in second pass
            external_ref,
            sort_order: None,
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
                        project_id: existing.project_id,
                        parent_id: Some(parent_id.clone()),
                        external_ref: ext_ref,
                        sort_order: Some(existing.sort_order),
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
                if !write_work_item_event_frame(writer, &WorkItemEventFrame::Event { event }).await
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
            session_id: None,
            provider: None,
            external_id: None,
            external_url: Some("https://example.test/task\u{001b}".into()),
            sort_order: 0.0,
            pinned_pr_url: None,
            cost: None,
            created_at: 0,
            updated_at: 0,
        };

        let prompt = render_work_item_task_prompt(&item);

        assert!(prompt.contains("Title:\nFix tests"));
        assert!(prompt.contains("Description:\nHandle failures\n\nthen report back"));
        assert!(prompt.contains("External link:\nhttps://example.test/task"));
        assert!(!prompt.contains('\u{0007}'));
        assert!(!prompt.contains('\u{0003}'));
        assert!(!prompt.contains('\u{001b}'));
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
            "work-item-dispatch",
            "work-item-events",
        ] {
            assert!(caps.contains(&serde_json::json!(cap)), "missing capability: {cap}");
        }

        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_dispatch_creates_session_and_binds_it() {
        let dir = tempfile::tempdir().unwrap();
        let (host, identity, joins) = make_host_and_identity(&dir).await;

        // Create a work item
        let resp = handle_request(
            req("work-item-create", serde_json::json!({ "title": "Write tests" })),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok);
        let item_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        // Subscribe to work-item events before dispatch
        let mut wi_rx = host.work_item_handle.subscribe_events();

        // Dispatch - repoPath points to our temp dir which is a valid path
        let resp = handle_request(
            req(
                "work-item-dispatch",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": dir.path(),
                    "profile": "plain-shell",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "dispatch failed: {:?}", resp.error);
        let session_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        // The work item should now have session_id bound
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.session_id.as_deref(), Some(session_id.as_str()), "session_id bound");
        assert_eq!(item.status, roux_core::WorkItemStatus::Doing);

        // Session, run, and status events should have been broadcast.
        let event = wi_rx.try_recv().expect("SessionBound event");
        assert!(
            matches!(&event, roux_core::WorkItemEvent::SessionBound { id, .. } if id == &item_id),
            "expected SessionBound, got: {event:?}"
        );
        let event = wi_rx.try_recv().expect("RunCreated event");
        assert!(
            matches!(&event, roux_core::WorkItemEvent::RunCreated { run } if run.work_item_id == item_id && run.session_id.as_deref() == Some(session_id.as_str())),
            "expected RunCreated, got: {event:?}"
        );
        let event = wi_rx.try_recv().expect("Moved event");
        assert!(
            matches!(
                &event,
                roux_core::WorkItemEvent::Moved {
                    id,
                    status: roux_core::WorkItemStatus::Doing,
                    ..
                } if id == &item_id
            ),
            "expected Moved, got: {event:?}"
        );

        // A card can have multiple runs. The compatibility session_id points
        // at the latest run, but run history keeps both attempts.
        let resp2 = handle_request(
            req(
                "work-item-dispatch",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": dir.path(),
                    "profile": "plain-shell",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp2.ok, "second dispatch should create another run: {:?}", resp2.error);
        let session_id_2 = resp2.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        assert_ne!(session_id, session_id_2);
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(
            item.session_id.as_deref(),
            Some(session_id_2.as_str()),
            "compatibility session_id points at the latest run",
        );
        assert_eq!(item.status, roux_core::WorkItemStatus::Doing);
        let runs = host.work_item_handle.list_runs(Some(&item_id)).unwrap();
        assert_eq!(runs.len(), 2);

        let _ = host.pty_handle.kill(&session_id).await;
        let _ = host.pty_handle.kill(&session_id_2).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_dispatch_defaults_to_agent_profile() {
        // With no `profile` arg the dispatch resolves the built-in Claude
        // agent and types its startup command into the PTY (best-effort). This
        // exercises the resolve -> build-startup -> pty-write glue; we assert the
        // dispatch still succeeds and binds (the agent run never fails it).
        let dir = tempfile::tempdir().unwrap();
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
                "work-item-dispatch",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": dir.path(),
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "default-profile dispatch failed: {:?}", resp.error);
        let session_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        let item = host.work_item_handle.get(&item_id).unwrap().unwrap();
        assert_eq!(item.session_id.as_deref(), Some(session_id.as_str()));

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_dispatch_forwards_session_target_args() {
        let dir = tempfile::tempdir().unwrap();
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
                "work-item-dispatch",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": dir.path(),
                    "name": "Prompt-selected name",
                    "worktreePath": worktree,
                    "profile": "plain-shell",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "dispatch failed: {:?}", resp.error);
        let session_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let session = host.session_handle.get(&session_id).await.unwrap().unwrap();
        assert_eq!(session.name, "Prompt-selected name");
        assert_eq!(session.worktree_path, worktree.to_string_lossy());

        let _ = host.pty_handle.kill(&session_id).await;
        shutdown_host(host, joins).await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_work_item_run_dispatch_returns_run_and_lists_it() {
        let dir = tempfile::tempdir().unwrap();
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
                "work-item-run-dispatch",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": dir.path(),
                    "profile": "plain-shell",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "run dispatch failed: {:?}", resp.error);
        let run_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        let session_id = resp.data.as_ref().unwrap()["sessionId"].as_str().unwrap().to_string();
        assert_eq!(resp.data.as_ref().unwrap()["workItemId"], item_id);

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
    async fn daemon_work_item_run_output_detects_decision_prompt() {
        let dir = tempfile::tempdir().unwrap();
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
                "work-item-run-dispatch",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": dir.path(),
                    "profile": "plain-shell",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "run dispatch failed: {:?}", resp.error);
        let run_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        let session_id = resp.data.as_ref().unwrap()["sessionId"].as_str().unwrap().to_string();

        let line = r#"{"type":"decision","question":"Choose path?","options":[{"value":"existing","label":"Use existing"},{"value":"new","label":"Create new"}],"defaultValue":"existing"}"#;
        host.pty_handle.write(&session_id, format!("{line}\n").into_bytes()).await.unwrap();

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
    async fn daemon_work_item_run_marks_done_on_zero_pty_exit() {
        let dir = tempfile::tempdir().unwrap();
        let (host, _identity, joins) = make_host_and_identity(&dir).await;

        let (run_id, _pty_id) =
            spawn_task_work_item_run(&host, dir.path(), "exit 0", "task-exit-zero").await;

        let run = wait_for_run_status(&host, &run_id, roux_core::WorkItemRunStatus::Done).await;

        assert!(run.ended_at.is_some());
        let events = host.work_item_handle.list_run_events(&run_id).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == roux_core::WorkItemRunEventKind::StatusChanged
                && event.payload["status"] == "done"
                && event.payload["reason"] == "ptyExit"
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
                "work-item-run-dispatch",
                serde_json::json!({
                    "id": item_id,
                    "repoPath": dir.path(),
                    "profile": "plain-shell",
                }),
            ),
            &host,
            &identity,
        )
        .await;
        assert!(resp.ok, "run dispatch failed: {:?}", resp.error);
        let run_id = resp.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        let session_id = resp.data.as_ref().unwrap()["sessionId"].as_str().unwrap().to_string();

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
        assert_eq!(events.last().unwrap()["kind"], "statusChanged");
        assert_eq!(events.last().unwrap()["payload"]["status"], "stopped");

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
        let pty_id = "decision-resolve-pty";
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
