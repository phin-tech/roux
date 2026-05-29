use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::watch;

use roux_core::{PtyRole, PtyStatus};
use roux_runtime::automation_hooks::{
    context_from_run_request, hook_list_to_value, hook_run_to_value, request_from_socket_args,
    worktree_provider_hooks, AutomationHookManager, HookContext, HookEvent, HookRunSummary,
};
use roux_runtime::host::{RuntimeHost, RuntimeHostConfig};
use roux_runtime::process_service::PROCESS_OUTPUT_DEFAULT_POLL_BYTES;
use roux_runtime::pty_service::{PtyEnvRequest, PtySpawnRequest, PTY_OUTPUT_DEFAULT_POLL_BYTES};
use roux_runtime::terminal_env::NotesEnvInputs;
use roux_runtime::watch_runner::WatchRunner;

use crate::{daemon_log::DaemonLog, paths, platform};

mod identity;
mod messaging;
mod notes;
mod projects;
mod protocol;
mod server;
mod status;
mod streams;
mod watches;
mod work_items;

use identity::{request_authorized, DaemonIdentity};
use messaging::{
    handle_alias_add_member, handle_alias_claim, handle_alias_get, handle_alias_list,
    handle_alias_mode, handle_alias_remove_member, handle_alias_set, handle_alias_unset,
    handle_alias_whoami, handle_bus_publish, handle_bus_subscribe, handle_bus_subscriptions,
    handle_bus_tail, handle_bus_unsubscribe, handle_mailbox_ack, handle_mailbox_clear,
    handle_mailbox_count, handle_mailbox_dismiss, handle_mailbox_get, handle_mailbox_mark_read,
    handle_mailbox_peek, handle_mailbox_post, handle_mailbox_read, handle_mailbox_read_state,
    handle_mailbox_reply, handle_mailbox_retract, handle_mailbox_sent,
};
use notes::{
    handle_notes_append, handle_notes_path, handle_notes_read, handle_notes_search,
    handle_notes_vault_root, handle_notes_write,
};
use projects::{
    handle_project_create, handle_project_list, handle_project_remove, handle_project_rename,
    handle_project_update,
};
use protocol::{Request, Response};
use server::start_socket_server;
use status::{handle_daemon_status, handle_daemon_stop};
use watches::{
    handle_watch_cleanup_orphans, handle_watch_create, handle_watch_find_or_create,
    handle_watch_list, handle_watch_pause, handle_watch_remove, handle_watch_remove_for_session,
    handle_watch_replace, handle_watch_resume,
};
use work_items::{
    handle_work_item_create, handle_work_item_decision_create, handle_work_item_decision_resolve,
    handle_work_item_decisions_list, handle_work_item_delete, handle_work_item_import,
    handle_work_item_list, handle_work_item_move, handle_work_item_plan,
    handle_work_item_review_accept, handle_work_item_run_events, handle_work_item_run_stop,
    handle_work_item_runs_list, handle_work_item_start, handle_work_item_update,
    schedule_pending_work_item_decision_timeouts,
};

const DEFAULT_LATEST_OUTPUT_BYTES: usize = 8 * 1024;
const MAX_LATEST_OUTPUT_BYTES: usize = 64 * 1024;

pub async fn run() -> Result<(), String> {
    paths::migrate_legacy_config_dir();
    let log = DaemonLog::init();

    let project_path = platform::projects_path();
    let session_path = platform::sessions_path();
    let watch_path = platform::watches_path();
    let projects = roux_runtime::project_service::load_persisted_from(&project_path);
    let sessions = roux_runtime::session_service::load_persisted_from(&session_path, &projects);
    let watches = roux_runtime::watch_service::load_persisted_from(&watch_path);
    log.write(&format!(
        "Loaded {} project(s) from {}, {} session(s) from {}, and {} watch(es) from {}",
        projects.len(),
        project_path.display(),
        sessions.len(),
        session_path.display(),
        watches.len(),
        watch_path.display()
    ));

    let services = RuntimeHostConfig {
        initial_sessions: sessions,
        session_persist_path: session_path,
        initial_projects: projects,
        project_persist_path: project_path,
        initial_watches: watches,
        watch_persist_path: Some(watch_path),
        work_item_db_path: platform::work_items_db_path(),
    }
    .build();

    let (host, joins) = services.spawn_with(tokio::spawn);
    match host.work_item_handle.fail_starting_runs_after_restart() {
        Ok(recovered) if !recovered.is_empty() => {
            log.write(&format!(
                "Recovered {} stale starting work-item run(s) after restart",
                recovered.len()
            ));
        }
        Ok(_) => {}
        Err(err) => {
            log.write(&format!("Warning: failed to recover stale starting work-item runs: {err}"));
        }
    }
    if let Err(err) = schedule_pending_work_item_decision_timeouts(host.clone()).await {
        log.write(&format!("Warning: failed to schedule pending work item decisions: {err}"));
    }

    let status_dir = platform::status_dir();
    if let Err(err) =
        roux_runtime::session_status_source::start_watching(status_dir, host.session_handle.clone())
    {
        log.write(&format!("Warning: failed to start session status watcher: {err}"));
    }

    let watch_runner = WatchRunner::new(host.watch_handle.clone(), daemon_hook_manager());
    watch_runner.start_all().await;
    let endpoint = platform::daemon_bind_endpoint();
    let auth_token = daemon_auth_token(&endpoint)?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let identity =
        DaemonIdentity::new(endpoint, log.path().clone(), auth_token).with_shutdown(shutdown_tx);
    let socket_server =
        start_socket_server(host.clone(), watch_runner.clone(), identity.clone(), log.clone())
            .await?;
    log.write(&format!(
        "Started on {}; press Ctrl-C to stop",
        socket_server.endpoint.display_value()
    ));

    wait_for_shutdown_signal(shutdown_rx).await?;
    log.write("Shutdown signal received");

    socket_server.shutdown();
    log.write("Socket server stopped");
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    watch_runner.shutdown();
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    log.write("Runtime services stopped");
    drop(host);

    for join in joins {
        if let Err(err) = join.await {
            log.write(&format!("Daemon task join failed: {err}"));
            return Err(format!("daemon task join failed: {err}"));
        }
    }

    log.write("Shutdown complete");
    Ok(())
}

#[cfg(test)]
async fn handle_request(req: Request, host: &RuntimeHost, identity: &DaemonIdentity) -> Response {
    handle_request_with_watch_runner(req, host, None, identity).await
}

async fn handle_request_with_watch_runner(
    req: Request,
    host: &RuntimeHost,
    watch_runner: Option<&WatchRunner>,
    identity: &DaemonIdentity,
) -> Response {
    if !request_authorized(&req, identity) {
        return Response::err("unauthorized");
    }

    match req.command.as_str() {
        "daemon-status" => handle_daemon_status(host, identity).await,
        "daemon-stop" => handle_daemon_stop(identity).await,
        "session-list" => handle_session_list(host).await,
        "session-poll" => handle_session_poll(req, host).await,
        "session-create" => handle_cli_session_create(req, host, identity).await,
        "session-create-shell" => handle_session_create_shell(req, host, identity).await,
        "session-reconnect-shell" => handle_session_reconnect_shell(req, host, identity).await,
        "session-panes-list" => handle_session_panes_list(req, host).await,
        "session-panes-create" => handle_session_panes_create(req, host, identity).await,
        "session-archive" => handle_session_archive(req, host).await,
        "session-kill" => handle_session_archive(req, host).await,
        "session-restore" => handle_session_restore(req, host).await,
        "session-delete" => handle_session_delete(req, host).await,
        "session-worktree-exists" => handle_session_worktree_exists(req, host).await,
        "session-refresh-branch" => handle_session_refresh_branch(req, host).await,
        "session-rename" => handle_session_rename(req, host).await,
        "session-set-project" => handle_session_set_project(req, host).await,
        "session-set-pinned-pr-url" => handle_session_set_pinned_pr_url(req, host).await,
        "alias-set" => handle_alias_set(req, identity).await,
        "alias-unset" => handle_alias_unset(req, identity).await,
        "alias-claim" => handle_alias_claim(req, identity).await,
        "alias-list" => handle_alias_list(req, identity).await,
        "alias-get" => handle_alias_get(req, identity).await,
        "alias-whoami" => handle_alias_whoami(req, identity).await,
        "alias-add-member" => handle_alias_add_member(req, identity).await,
        "alias-remove-member" => handle_alias_remove_member(req, identity).await,
        "alias-mode" => handle_alias_mode(req, identity).await,
        "mailbox-post" => handle_mailbox_post(req, identity).await,
        "mailbox-peek" => handle_mailbox_peek(req, identity).await,
        "mailbox-read" => handle_mailbox_read(req, identity).await,
        "mailbox-get" => handle_mailbox_get(req, identity).await,
        "mailbox-read-state" => handle_mailbox_read_state(req, identity).await,
        "mailbox-mark-read" => handle_mailbox_mark_read(req, identity).await,
        "mailbox-ack" => handle_mailbox_ack(req, identity).await,
        "mailbox-retract" => handle_mailbox_retract(req, identity).await,
        "mailbox-dismiss" => handle_mailbox_dismiss(req, identity).await,
        "mailbox-count" => handle_mailbox_count(req, identity).await,
        "mailbox-clear" => handle_mailbox_clear(req, identity).await,
        "mailbox-reply" => handle_mailbox_reply(req, identity).await,
        "mailbox-sent" => handle_mailbox_sent(req, identity).await,
        "bus-publish" => handle_bus_publish(req, identity).await,
        "bus-tail" => handle_bus_tail(req, identity).await,
        "bus-subscribe" => handle_bus_subscribe(req, identity).await,
        "bus-unsubscribe" => handle_bus_unsubscribe(req, identity).await,
        "bus-subscriptions" => handle_bus_subscriptions(req, identity).await,
        "project-list" => handle_project_list(host).await,
        "project-create" => handle_project_create(req, host).await,
        "project-remove" => handle_project_remove(req, host).await,
        "project-rename" => handle_project_rename(req, host).await,
        "project-update" => handle_project_update(req, host).await,
        "watch-list" => handle_watch_list(host).await,
        "watch-create" => match watch_runner {
            Some(runner) => handle_watch_create(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-find-or-create" => match watch_runner {
            Some(runner) => handle_watch_find_or_create(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-remove" => match watch_runner {
            Some(runner) => handle_watch_remove(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-pause" => match watch_runner {
            Some(runner) => handle_watch_pause(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-resume" => match watch_runner {
            Some(runner) => handle_watch_resume(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-replace" => match watch_runner {
            Some(runner) => handle_watch_replace(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-remove-for-session" => match watch_runner {
            Some(runner) => handle_watch_remove_for_session(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-cleanup-orphans" => match watch_runner {
            Some(runner) => handle_watch_cleanup_orphans(host, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "work-item-list" => handle_work_item_list(req, host).await,
        "work-item-create" => handle_work_item_create(req, host).await,
        "work-item-update" => handle_work_item_update(req, host).await,
        "work-item-move" => handle_work_item_move(req, host).await,
        "work-item-delete" => handle_work_item_delete(req, host).await,
        "work-item-plan" => handle_work_item_plan(req, host, identity).await,
        "work-item-start" => handle_work_item_start(req, host, identity).await,
        "work-item-review-accept" => handle_work_item_review_accept(req, host).await,
        "work-item-runs-list" => handle_work_item_runs_list(req, host).await,
        "work-item-run-events" => handle_work_item_run_events(req, host).await,
        "work-item-run-stop" => handle_work_item_run_stop(req, host).await,
        "work-item-decision-create" => handle_work_item_decision_create(req, host).await,
        "work-item-decisions-list" => handle_work_item_decisions_list(req, host).await,
        "work-item-decision-resolve" => handle_work_item_decision_resolve(req, host).await,
        "work-item-import" => handle_work_item_import(req, host).await,
        "worktree-list" => handle_worktree_list(req).await,
        "worktree-create" => handle_worktree_create(req).await,
        "worktree-remove" => handle_worktree_remove(req).await,
        "worktree-list-branches" => handle_worktree_list_branches(req).await,
        "git-init" => handle_git_init(req).await,
        "notes-read" => handle_notes_read(req, host).await,
        "notes-write" => handle_notes_write(req, host).await,
        "notes-append" => handle_notes_append(req, host).await,
        "notes-path" => handle_notes_path(req, host).await,
        "notes-search" => handle_notes_search(req).await,
        "notes-vault-root" => handle_notes_vault_root(req).await,
        "hook-show" => handle_hook_show(req).await,
        "hook-preview" => handle_hook_preview(req).await,
        "hook-run" => handle_hook_run(req).await,
        "hook-approve" => handle_hook_approve(req).await,
        "hook-clear-approvals" => handle_hook_clear_approvals().await,
        "hook-log-list" => handle_hook_log_list().await,
        "hook-log-read" => handle_hook_log_read(req).await,
        "run" => handle_daemon_process_start(req, host).await,
        "shell" => handle_session_panes_create(req, host, identity).await,
        "split" => handle_session_panes_create(req, host, identity).await,
        "send" => handle_cli_send(req, host).await,
        "latest-output" => handle_latest_output(req, host).await,
        "daemon-process-start" => handle_daemon_process_start(req, host).await,
        "daemon-process-output" => handle_daemon_process_output(req, host).await,
        "daemon-process-list" => handle_daemon_process_list(host).await,
        "daemon-process-kill" => handle_daemon_process_kill(req, host).await,
        "daemon-pty-spawn-shell" => handle_daemon_pty_spawn_shell(req, host, identity).await,
        "daemon-pty-spawn-task" => handle_daemon_pty_spawn_task(req, host, identity).await,
        "daemon-pty-output" => handle_daemon_pty_output(req, host).await,
        "daemon-pty-list" => handle_daemon_pty_list(host).await,
        "daemon-pty-write" => handle_daemon_pty_write(req, host).await,
        "daemon-pty-resize" => handle_daemon_pty_resize(req, host).await,
        "daemon-pty-detach" => handle_daemon_pty_detach(req, host).await,
        "daemon-pty-attach-pane" => handle_daemon_pty_attach_pane(req, host).await,
        "daemon-pty-mark-read" => handle_daemon_pty_mark_read(req, host).await,
        "daemon-pty-set-name" => handle_daemon_pty_set_name(req, host).await,
        "daemon-pty-kill" => handle_daemon_pty_kill(req, host).await,
        _ => Response::err(format!("unknown daemon command: {}", req.command)),
    }
}

async fn handle_session_list(host: &RuntimeHost) -> Response {
    match host.session_handle.list().await {
        Ok(sessions) => match serde_json::to_value(&sessions) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize sessions: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_poll(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = req.session_id.as_deref() else {
        return Response::err("session_id required");
    };
    match host.session_handle.get(session_id).await {
        Ok(Some(session)) => match serde_json::to_value(&session) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize session: {err}")),
        },
        Ok(None) => Response::err("session not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_rename(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = req.session_id.as_deref() else {
        return Response::err("session_id required (set $ROUX_SESSION_ID or pass --session)");
    };
    let Some(raw) = req.args.get("name").and_then(|name| name.as_str()) else {
        return Response::err("name required");
    };
    let name_override = if raw.trim().is_empty() { None } else { Some(raw.trim().to_string()) };

    if let Err(err) = host.session_handle.set_name_override(session_id, name_override.clone()).await
    {
        return Response::err(err.to_string());
    }

    Response::success(serde_json::json!({
        "session_id": session_id,
        "name_override": name_override,
    }))
}

async fn handle_session_set_project(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let project_id = optional_nullable_string_arg(&req.args, &["projectId", "project_id"]);
    match host.session_handle.set_project(session_id, project_id.clone()).await {
        Ok(()) => Response::success(serde_json::json!({
            "session_id": session_id,
            "project_id": project_id,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_set_pinned_pr_url(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let url = optional_nullable_string_arg(&req.args, &["url", "pinnedPrUrl", "pinned_pr_url"]);
    match host.session_handle.set_pinned_pr_url(session_id, url.clone()).await {
        Ok(()) => Response::success(serde_json::json!({
            "session_id": session_id,
            "url": url,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_cli_session_create(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    if req.args.get("prompt").and_then(|prompt| prompt.as_str()).is_some() {
        return Response::err(
            "daemon session create does not support --prompt until a frontend attaches; create the session, attach, then send input",
        );
    }
    let normalized = match normalize_cli_session_create_request(req, host).await {
        Ok(req) => req,
        Err(response) => return response,
    };

    let create = handle_session_create_shell(normalized, host, identity).await;
    if !create.ok {
        return create;
    }

    let Some(session_id) = create
        .data
        .as_ref()
        .and_then(|data| data.get("id"))
        .and_then(|id| id.as_str())
        .map(str::to_string)
    else {
        return Response::err("session created but response did not include id");
    };

    Response::success(serde_json::json!({ "session_id": session_id }))
}

async fn normalize_cli_session_create_request(
    mut req: Request,
    host: &RuntimeHost,
) -> Result<Request, Response> {
    if req
        .args
        .get("flags")
        .and_then(|flags| flags.as_array())
        .is_some_and(|flags| !flags.is_empty())
    {
        return Err(Response::err(
            "--flag/-f is not supported by daemon session create; bake flags into a spawn profile's startup command instead",
        ));
    }
    let mut args = req.args.as_object().cloned().unwrap_or_default();
    let working_dir = args
        .get("working_dir")
        .or_else(|| args.get("workingDir"))
        .and_then(|working_dir| working_dir.as_str())
        .filter(|working_dir| !working_dir.trim().is_empty())
        .map(str::to_string);

    let mut repo_path = args
        .get("repoPath")
        .or_else(|| args.get("repo_path"))
        .and_then(|repo_path| repo_path.as_str())
        .filter(|repo_path| !repo_path.trim().is_empty())
        .map(str::to_string)
        .or(working_dir);

    if repo_path.is_none() {
        if let Some(session_id) = req.session_id.as_deref() {
            match host.session_handle.get(session_id).await {
                Ok(Some(session)) => repo_path = Some(session.repo_root),
                Ok(None) => {}
                Err(err) => return Err(Response::err(err.to_string())),
            }
        }
    }

    let Some(repo_path) = repo_path else {
        return Err(Response::err("working_dir, repoPath, or session_id required"));
    };
    args.insert("repoPath".to_string(), Value::String(repo_path));

    if let Some(branch) = args.remove("worktree_branch").or_else(|| args.remove("worktreeBranch")) {
        args.insert("branch".to_string(), branch);
    }
    if let Some(start_point) = args.remove("start_point").or_else(|| args.remove("startPoint")) {
        let fetch_first = start_point.as_str().is_some_and(|start| start.starts_with("origin/"));
        args.insert("base".to_string(), start_point);
        args.entry("fetchFirst".to_string()).or_insert(Value::Bool(fetch_first));
    }
    args.entry("profile".to_string()).or_insert(Value::String("claude".to_string()));

    req.args = Value::Object(args);
    Ok(req)
}

async fn handle_session_create_shell(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(repo_path) = req
        .args
        .get("repoPath")
        .or_else(|| req.args.get("repo_path"))
        .and_then(|repo_path| repo_path.as_str())
    else {
        return Response::err("repoPath required");
    };
    let name = req.args.get("name").and_then(|name| name.as_str()).unwrap_or("New Session");
    let id = req
        .args
        .get("id")
        .and_then(|id| id.as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    match host.session_handle.get(&id).await {
        Ok(Some(_)) => return Response::err(format!("session {id} already exists")),
        Ok(None) => {}
        Err(err) => return Response::err(err.to_string()),
    }

    let settings = load_daemon_settings();
    let target = parse_daemon_session_target(&req.args);
    let (work_dir, actual_branch, owns_worktree) =
        match resolve_daemon_session_target(repo_path, target, &settings, daemon_hook_manager())
            .await
        {
            Ok(resolved) => resolved,
            Err(err) => return Response::err(err),
        };

    let pane_id = format!("{id}-main");
    let profile = req.args.get("profile").and_then(|profile| profile.as_str()).map(str::to_string);
    let initial_size = parse_initial_size(&req.args);
    let project_id = req
        .args
        .get("projectId")
        .or_else(|| req.args.get("project_id"))
        .and_then(|project_id| project_id.as_str())
        .map(str::to_string);
    let blueprint_id = req
        .args
        .get("blueprintId")
        .or_else(|| req.args.get("blueprint_id"))
        .and_then(|blueprint_id| blueprint_id.as_str())
        .map(str::to_string);
    let spawn = host
        .pty_handle
        .spawn_shell(PtySpawnRequest {
            id: Some(id.clone()),
            working_dir: Some(PathBuf::from(&work_dir)),
            session_id: Some(id.clone()),
            pane_id: Some(pane_id),
            project_id: project_id.clone(),
            worktree_path: owns_worktree.then(|| work_dir.clone()),
            notes: parse_notes_env(&req.args),
            env: parse_pty_env_request(&req.args, identity),
            profile: profile.clone(),
            initial_size,
            role: roux_core::PtyRole::SessionPrimary,
        })
        .await;
    if let Err(err) = spawn {
        if owns_worktree {
            let _ = roux_core::remove_worktree(repo_path, &work_dir);
        }
        return Response::err(err.to_string());
    }

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let session = roux_core::Session {
        id: id.clone(),
        name: name.to_string(),
        repo_root: repo_path.to_string(),
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: owns_worktree,
        status: roux_core::SessionStatus::Idle,
        model: None,
        cost: None,
        created_at: now,
        project_id,
        is_git_repo: is_git_repo(repo_path),
        name_override: None,
        primary_pty_id: Some(id.clone()),
        archived: false,
        ended_at: None,
        blueprint_id,
        pinned_pr_url: None,
    };

    if let Err(err) = host.session_handle.add(session.clone()).await {
        let _ = host.pty_handle.kill(&id).await;
        if session.is_worktree {
            let _ = roux_core::remove_worktree(&session.repo_root, &session.worktree_path);
        }
        return Response::err(err.to_string());
    }

    match serde_json::to_value(session) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize session: {err}")),
    }
}

async fn handle_session_reconnect_shell(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let session = match host.session_handle.get(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    };
    let primary_pty_id = session.primary_pty_id.as_deref().unwrap_or(&session.id).to_string();
    let _ = host.pty_handle.remove(&primary_pty_id).await;
    let pane_id = format!("{}-main", session.id);
    let initial_size = parse_initial_size(&req.args);
    let profile = req.args.get("profile").and_then(|profile| profile.as_str()).map(str::to_string);
    let spawn = host
        .pty_handle
        .spawn_shell(PtySpawnRequest {
            id: Some(session.id.clone()),
            working_dir: Some(PathBuf::from(&session.worktree_path)),
            session_id: Some(session.id.clone()),
            pane_id: Some(pane_id),
            project_id: session.project_id.clone(),
            worktree_path: session.is_worktree.then(|| session.worktree_path.clone()),
            notes: parse_notes_env(&req.args),
            env: parse_pty_env_request(&req.args, identity),
            profile,
            initial_size,
            role: roux_core::PtyRole::SessionPrimary,
        })
        .await;
    if let Err(err) = spawn {
        return Response::err(err.to_string());
    }

    if let Err(err) =
        host.session_handle.update_status(&session.id, roux_core::SessionStatus::Idle).await
    {
        let _ = host.pty_handle.kill(&session.id).await;
        return Response::err(err.to_string());
    }
    match host.session_handle.get(&session.id).await {
        Ok(Some(updated)) => serialize_session(updated),
        Ok(None) => Response::err("session not found after reconnect"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_archive(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    kill_session_ptys(host, session_id).await;
    if let Err(err) = host.session_handle.archive(session_id).await {
        return Response::err(err.to_string());
    }
    match host.session_handle.get(session_id).await {
        Ok(Some(session)) => serialize_session(session),
        Ok(None) => Response::err("session not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_restore(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    if let Err(err) = host.session_handle.restore(session_id).await {
        return Response::err(err.to_string());
    }
    if let Err(err) =
        host.session_handle.update_status(session_id, roux_core::SessionStatus::Disconnected).await
    {
        return Response::err(err.to_string());
    }
    match host.session_handle.get(session_id).await {
        Ok(Some(session)) => serialize_session(session),
        Ok(None) => Response::err("session not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_delete(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    kill_session_ptys(host, session_id).await;
    if let Err(err) = host.session_handle.remove(session_id).await {
        return Response::err(err.to_string());
    }
    Response::success(serde_json::json!({ "session_id": session_id }))
}

async fn handle_session_worktree_exists(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    match host.session_handle.get(session_id).await {
        Ok(Some(session)) => Response::success(serde_json::json!({
            "session_id": session_id,
            "exists": Path::new(&session.worktree_path).exists(),
        })),
        Ok(None) => Response::err("session not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_refresh_branch(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let session = match host.session_handle.get(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    };
    let is_git_repo = is_git_repo(&session.worktree_path);
    if is_git_repo != session.is_git_repo {
        if let Err(err) = host.session_handle.set_git_repo(session_id, is_git_repo).await {
            return Response::err(err.to_string());
        }
    }
    if !is_git_repo {
        return Response::success(serde_json::json!({ "branch": session.branch }));
    }
    let branch = get_current_branch(&session.worktree_path)
        .filter(|branch| !branch.is_empty())
        .unwrap_or(session.branch);
    if let Err(err) = host.session_handle.set_branch(session_id, branch.clone()).await {
        return Response::err(err.to_string());
    }
    Response::success(serde_json::json!({ "branch": branch }))
}

async fn kill_session_ptys(host: &RuntimeHost, session_id: &str) {
    let ptys = host.pty_handle.list().await.unwrap_or_default();
    for pty in ptys {
        if pty.info.session_id.as_deref() == Some(session_id) {
            let _ = host.pty_handle.remove(&pty.id).await;
        }
    }
}

fn serialize_session(session: roux_core::Session) -> Response {
    match serde_json::to_value(session) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize session: {err}")),
    }
}

fn daemon_hook_manager() -> AutomationHookManager {
    AutomationHookManager::from_config_root(platform::app_config_dir())
}

async fn handle_hook_show(req: Request) -> Response {
    handle_hook_show_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_show_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let repo_path = req
        .args
        .get("repoPath")
        .or_else(|| req.args.get("repo_path"))
        .and_then(|value| value.as_str());
    match hooks.list_hooks(repo_path) {
        Ok(items) => Response::success(hook_list_to_value(items)),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_preview(req: Request) -> Response {
    handle_hook_preview_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_preview_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let request = match request_from_socket_args(req.args) {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    let settings = load_daemon_settings();
    let wt_available = resolve_wt_binary(&settings).is_some();
    let (event, context) =
        match context_from_run_request(request, Some(settings.worktree_provider), wt_available) {
            Ok(parts) => parts,
            Err(err) => return Response::err(err.to_string()),
        };
    match hooks.preview(event, &context) {
        Ok(items) => match serde_json::to_value(items) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize hook preview: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_run(req: Request) -> Response {
    handle_hook_run_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_run_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let request = match request_from_socket_args(req.args) {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    let settings = load_daemon_settings();
    let wt_available = resolve_wt_binary(&settings).is_some();
    let (event, context) =
        match context_from_run_request(request, Some(settings.worktree_provider), wt_available) {
            Ok(parts) => parts,
            Err(err) => return Response::err(err.to_string()),
        };
    let result = if event.is_blocking() {
        hooks.run_blocking(event, context).await
    } else {
        hooks.run_background(event, context).await
    };
    match result {
        Ok(ran) => Response::success(hook_run_to_value(HookRunSummary {
            event: event.as_str().into(),
            ran,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_approve(req: Request) -> Response {
    handle_hook_approve_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_approve_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(approval_id) = req
        .args
        .get("approvalId")
        .or_else(|| req.args.get("approval_id"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return Response::err("approvalId required");
    };
    match hooks.approve(approval_id) {
        Ok(()) => Response::success(Value::Null),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_clear_approvals() -> Response {
    handle_hook_clear_approvals_with_hooks(daemon_hook_manager()).await
}

async fn handle_hook_clear_approvals_with_hooks(hooks: AutomationHookManager) -> Response {
    match hooks.clear_approvals() {
        Ok(()) => Response::success(Value::Null),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_log_list() -> Response {
    handle_hook_log_list_with_hooks(daemon_hook_manager()).await
}

async fn handle_hook_log_list_with_hooks(hooks: AutomationHookManager) -> Response {
    match serde_json::to_value(hooks.list_logs()) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize hook logs: {err}")),
    }
}

async fn handle_hook_log_read(req: Request) -> Response {
    handle_hook_log_read_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_log_read_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(path) = req
        .args
        .get("path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return Response::err("path required");
    };
    match hooks.read_log(path) {
        Ok(content) => Response::success(Value::String(content)),
        Err(err) => Response::err(err.to_string()),
    }
}

fn build_daemon_post_worktree_create_context(
    provider: roux_core::WorktreeProvider,
    wt_available: bool,
    repo_path: &str,
    branch: &str,
    worktree_path: &str,
) -> HookContext {
    let mut context =
        HookContext::new(HookEvent::PostWorktreeCreate).with_provider(provider, wt_available);
    context.repo_path = Some(repo_path.to_string());
    context.worktree_path = Some(worktree_path.to_string());
    context.branch = Some(branch.to_string());
    context.cwd = Some(worktree_path.to_string());
    context.provider_hooks_ran =
        worktree_provider_hooks(HookEvent::PostWorktreeCreate, context.worktrunk);
    context
}

fn build_daemon_post_worktree_remove_context(
    provider: roux_core::WorktreeProvider,
    wt_available: bool,
    repo_path: &str,
    worktree_path: &str,
) -> HookContext {
    let mut context =
        HookContext::new(HookEvent::PostWorktreeRemove).with_provider(provider, wt_available);
    context.repo_path = Some(repo_path.to_string());
    context.worktree_path = Some(worktree_path.to_string());
    context.cwd = Some(repo_path.to_string());
    context.provider_hooks_ran =
        worktree_provider_hooks(HookEvent::PostWorktreeRemove, context.worktrunk);
    context
}

async fn handle_worktree_list(req: Request) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let repo_path = repo_path.to_string();
    let settings = load_daemon_settings();
    match tokio::task::spawn_blocking(move || {
        let wt = resolve_wt_binary(&settings);
        roux_core::list_worktrees_enriched(&repo_path, wt.as_ref()).map_err(|err| err.to_string())
    })
    .await
    {
        Ok(Ok(worktrees)) => match serde_json::to_value(worktrees) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize worktrees: {err}")),
        },
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-list task failed: {err}")),
    }
}

async fn handle_worktree_create(req: Request) -> Response {
    handle_worktree_create_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_worktree_create_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let Some(branch) = req.args.get("branch").and_then(|branch| branch.as_str()) else {
        return Response::err("branch required");
    };
    let repo_path = repo_path.to_string();
    let branch = branch.to_string();
    let start_point = optional_string_arg(&req.args, &["startPoint", "start_point", "base"]);
    let base_path = optional_string_arg(&req.args, &["basePath", "base_path"]);
    let fetch_first = bool_arg(&req.args, &["fetchFirst", "fetch_first"]).unwrap_or(false);
    let settings = load_daemon_settings();
    let provider = settings.worktree_provider;
    let wt = resolve_wt_binary(&settings);
    let wt_available = wt.is_some();
    let pre_context = HookContext {
        repo_path: Some(repo_path.clone()),
        branch: Some(branch.clone()),
        cwd: Some(repo_path.clone()),
        ..HookContext::new(HookEvent::PreWorktreeCreate).with_provider(provider, wt_available)
    };
    if let Err(err) = hooks.run_blocking(HookEvent::PreWorktreeCreate, pre_context).await {
        return Response::err(err.to_string());
    }
    let post_hooks = hooks.clone();
    let post_repo_path = repo_path.clone();
    let post_branch = branch.clone();

    match tokio::task::spawn_blocking(move || {
        if fetch_first {
            roux_core::fetch_origin(&repo_path).map_err(|err| err.to_string())?;
        }
        let base_path = base_path.as_deref().or(settings.worktree_base_path.as_deref());
        roux_core::create_worktree_with_provider(
            &repo_path,
            &branch,
            base_path,
            start_point.as_deref(),
            provider,
            wt.as_ref(),
        )
        .map_err(|err| err.to_string())
    })
    .await
    {
        Ok(Ok(path)) => {
            let context = build_daemon_post_worktree_create_context(
                provider,
                wt_available,
                &post_repo_path,
                &post_branch,
                &path,
            );
            post_hooks.spawn_background(HookEvent::PostWorktreeCreate, context);
            Response::success(serde_json::json!({ "path": path }))
        }
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-create task failed: {err}")),
    }
}

async fn handle_worktree_remove(req: Request) -> Response {
    handle_worktree_remove_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_worktree_remove_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let Some(worktree_path) = req
        .args
        .get("worktreePath")
        .or_else(|| req.args.get("worktree_path"))
        .and_then(|path| path.as_str())
    else {
        return Response::err("worktreePath required");
    };
    let repo_path = repo_path.to_string();
    let worktree_path = worktree_path.to_string();
    let response_repo_path = repo_path.clone();
    let response_worktree_path = worktree_path.clone();
    let also_branch = bool_arg(&req.args, &["alsoBranch", "also_branch"]).unwrap_or(false);
    let force = bool_arg(&req.args, &["force"]).unwrap_or(false);
    let settings = load_daemon_settings();
    let provider = settings.worktree_provider;
    let wt = resolve_wt_binary(&settings);
    let wt_available = wt.is_some();
    let pre_context = HookContext {
        repo_path: Some(repo_path.clone()),
        worktree_path: Some(worktree_path.clone()),
        cwd: Some(worktree_path.clone()),
        ..HookContext::new(HookEvent::PreWorktreeRemove).with_provider(provider, wt_available)
    };
    if let Err(err) = hooks.run_blocking(HookEvent::PreWorktreeRemove, pre_context).await {
        return Response::err(err.to_string());
    }
    let post_hooks = hooks.clone();

    match tokio::task::spawn_blocking(move || {
        roux_core::remove_worktree_with_provider(
            &repo_path,
            &worktree_path,
            also_branch,
            force,
            provider,
            wt.as_ref(),
        )
        .map_err(|err| err.to_string())
    })
    .await
    {
        Ok(Ok(())) => {
            let context = build_daemon_post_worktree_remove_context(
                provider,
                wt_available,
                &response_repo_path,
                &response_worktree_path,
            );
            post_hooks.spawn_background(HookEvent::PostWorktreeRemove, context);
            Response::success(serde_json::json!({
                "repoPath": response_repo_path,
                "worktreePath": response_worktree_path,
            }))
        }
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-remove task failed: {err}")),
    }
}

async fn handle_worktree_list_branches(req: Request) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let repo_path = repo_path.to_string();
    match tokio::task::spawn_blocking(move || list_branches(&repo_path)).await {
        Ok(Ok(branches)) => match serde_json::to_value(branches) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize branches: {err}")),
        },
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-list-branches task failed: {err}")),
    }
}

async fn handle_git_init(req: Request) -> Response {
    let Some(path) = req.args.get("path").and_then(|path| path.as_str()) else {
        return Response::err("path required");
    };
    let path = path.to_string();
    let response_path = path.clone();
    match tokio::task::spawn_blocking(move || git_init(&path)).await {
        Ok(Ok(())) => Response::success(serde_json::json!({ "path": response_path })),
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("git-init task failed: {err}")),
    }
}

async fn handle_cli_send(req: Request, host: &RuntimeHost) -> Response {
    let Some(text) = req.args.get("text").and_then(|text| text.as_str()) else {
        return Response::err("text required");
    };
    let enter = req.args.get("enter").and_then(|enter| enter.as_bool()).unwrap_or(true);
    let mut data = text.as_bytes().to_vec();
    if enter {
        data.push(b'\r');
    }

    let pty_id = match resolve_cli_send_pty_id(&req, host).await {
        Ok(pty_id) => pty_id,
        Err(response) => return response,
    };
    match host.pty_handle.write(&pty_id, data.clone()).await {
        Ok(()) => Response::success(serde_json::json!({ "id": pty_id, "bytes": data.len() })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_latest_output(req: Request, host: &RuntimeHost) -> Response {
    let max_bytes = latest_output_max_bytes(&req.args);
    let pty_id = match resolve_latest_output_pty_id(&req, host).await {
        Ok(pty_id) => pty_id,
        Err(response) => return response,
    };
    match host.pty_handle.snapshot(&pty_id, max_bytes).await {
        Ok(Some(snapshot)) => {
            let pane_id = req.pane_id.clone().or_else(|| daemon_record_pane_id(&snapshot.record));
            Response::success(latest_output_payload(
                snapshot.record.info.session_id,
                pane_id,
                snapshot.record.id,
                max_bytes,
                &snapshot.output_bytes,
            ))
        }
        Ok(None) => Response::err(format!("pty not found: {pty_id}")),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn resolve_latest_output_pty_id(
    req: &Request,
    host: &RuntimeHost,
) -> Result<String, Response> {
    let ptys = host.pty_handle.list().await.map_err(|err| Response::err(err.to_string()))?;

    if let Some(pane_id) = req.pane_id.as_deref().filter(|pane_id| !pane_id.trim().is_empty()) {
        return ptys
            .iter()
            .find(|pty| {
                pty_matches_pane(pty, pane_id)
                    && req
                        .session_id
                        .as_deref()
                        .is_none_or(|session_id| pty.info.session_id.as_deref() == Some(session_id))
            })
            .map(|pty| pty.id.clone())
            .ok_or_else(|| Response::err(format!("daemon PTY not found for pane {pane_id}")));
    }

    resolve_cli_send_pty_id(req, host).await
}

fn latest_output_max_bytes(args: &Value) -> usize {
    args.get("max_bytes")
        .or_else(|| args.get("maxBytes"))
        .and_then(|value| value.as_u64())
        .map(|bytes| (bytes as usize).clamp(1, MAX_LATEST_OUTPUT_BYTES))
        .unwrap_or(DEFAULT_LATEST_OUTPUT_BYTES)
}

fn latest_output_payload(
    session_id: Option<String>,
    pane_id: Option<String>,
    pty_id: String,
    max_bytes: usize,
    bytes: &[u8],
) -> Value {
    let mut data = serde_json::Map::new();
    data.insert("session_id".into(), optional_string_value(session_id));
    data.insert("pane_id".into(), optional_string_value(pane_id));
    data.insert("pty_id".into(), Value::String(pty_id));
    data.insert("max_bytes".into(), Value::Number(max_bytes.into()));
    data.insert("byte_count".into(), Value::Number(bytes.len().into()));
    data.insert("replay_bytes_base64".into(), Value::String(BASE64_STANDARD.encode(bytes)));
    if let Ok(text) = std::str::from_utf8(bytes) {
        data.insert("text".into(), Value::String(text.to_string()));
    }
    Value::Object(data)
}

fn optional_string_value(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

async fn resolve_cli_send_pty_id(req: &Request, host: &RuntimeHost) -> Result<String, Response> {
    let ptys = host.pty_handle.list().await.map_err(|err| Response::err(err.to_string()))?;

    if let Some(pane_id) = req.pane_id.as_deref().filter(|pane_id| !pane_id.trim().is_empty()) {
        return ptys
            .iter()
            .find(|pty| {
                pty_matches_pane(pty, pane_id)
                    && req
                        .session_id
                        .as_deref()
                        .is_none_or(|session_id| pty.info.session_id.as_deref() == Some(session_id))
            })
            .map(|pty| pty.id.clone())
            .ok_or_else(|| Response::err(format!("daemon PTY not found for pane {pane_id}")));
    }

    if let Some(pane_type) = req
        .args
        .get("pane_type")
        .and_then(|pane_type| pane_type.as_str())
        .filter(|pane_type| !pane_type.trim().is_empty())
    {
        let Some(session_id) = req.session_id.as_deref() else {
            return Err(Response::err("session_id required when using pane_type"));
        };
        return ptys
            .iter()
            .find(|pty| {
                pty.info.session_id.as_deref() == Some(session_id)
                    && pty.info.profile.as_deref() == Some(pane_type)
            })
            .map(|pty| pty.id.clone())
            .ok_or_else(|| {
                Response::err(format!(
                    "daemon PTY with profile {pane_type} not found for session {session_id}"
                ))
            });
    }

    let Some(session_id) = req.session_id.as_deref() else {
        return Err(Response::err("session_id or pane_id required"));
    };

    if let Some(primary_pty_id) = host
        .session_handle
        .get(session_id)
        .await
        .map_err(|err| Response::err(err.to_string()))?
        .and_then(|session| session.primary_pty_id)
    {
        return Ok(primary_pty_id);
    }

    ptys.iter()
        .find(|pty| {
            pty.info.session_id.as_deref() == Some(session_id)
                && matches!(pty.info.role, PtyRole::SessionPrimary)
        })
        .map(|pty| pty.id.clone())
        .ok_or_else(|| {
            Response::err(format!("primary daemon PTY not found for session {session_id}"))
        })
}

fn pty_matches_pane(pty: &roux_runtime::pty_service::PtyRecord, pane_id: &str) -> bool {
    pty.id == pane_id
        || pty.info.id == pane_id
        || matches!(&pty.info.status, PtyStatus::RunningAttached { pane_id: attached } if attached == pane_id)
}

fn daemon_record_pane_id(pty: &roux_runtime::pty_service::PtyRecord) -> Option<String> {
    match &pty.info.status {
        PtyStatus::RunningAttached { pane_id } => Some(pane_id.clone()),
        _ => None,
    }
}

async fn handle_session_panes_list(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = req.session_id.as_deref().filter(|session_id| !session_id.is_empty())
    else {
        return Response::err("session_id required");
    };
    match host.session_handle.get(session_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    }

    let mut ptys: Vec<_> = match host.pty_handle.list().await {
        Ok(ptys) => ptys
            .into_iter()
            .filter(|pty| pty.info.session_id.as_deref() == Some(session_id))
            .collect(),
        Err(err) => return Response::err(err.to_string()),
    };
    ptys.sort_by(|a, b| {
        let a_primary = matches!(a.info.role, PtyRole::SessionPrimary);
        let b_primary = matches!(b.info.role, PtyRole::SessionPrimary);
        b_primary.cmp(&a_primary).then_with(|| daemon_pane_id(a).cmp(&daemon_pane_id(b)))
    });

    let descriptors: Vec<Value> = ptys.iter().map(daemon_pane_descriptor).collect();
    Response::success(serde_json::json!({
        "sessionId": session_id,
        "layout": Value::Null,
        "descriptors": descriptors,
    }))
}

async fn handle_session_panes_create(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(session_id) = req.session_id.as_deref().filter(|session_id| !session_id.is_empty())
    else {
        return Response::err("session_id required");
    };
    let session = match host.session_handle.get(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    };

    let direction =
        req.args.get("direction").and_then(|direction| direction.as_str()).unwrap_or("horizontal");
    if direction != "horizontal" && direction != "vertical" {
        return Response::err("direction must be horizontal or vertical");
    }

    let profile =
        req.args.get("profile").and_then(|profile| profile.as_str()).unwrap_or("plain-shell");
    let working_dir = req
        .args
        .get("workingDir")
        .or_else(|| req.args.get("working_dir"))
        .and_then(|working_dir| working_dir.as_str())
        .unwrap_or(&session.worktree_path)
        .to_string();
    let pty_id = req
        .args
        .get("id")
        .and_then(|id| id.as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let pane_id = req
        .args
        .get("paneId")
        .or_else(|| req.args.get("pane_id"))
        .and_then(|pane_id| pane_id.as_str())
        .filter(|pane_id| !pane_id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    match host
        .pty_handle
        .spawn_shell(PtySpawnRequest {
            id: Some(pty_id.clone()),
            working_dir: Some(PathBuf::from(&working_dir)),
            session_id: Some(session_id.to_string()),
            pane_id: Some(pane_id.clone()),
            project_id: session.project_id.clone(),
            worktree_path: session.is_worktree.then(|| session.worktree_path.clone()),
            notes: parse_notes_env(&req.args),
            env: parse_pty_env_request(&req.args, identity),
            profile: Some(profile.to_string()),
            initial_size: parse_initial_size(&req.args),
            role: PtyRole::Secondary,
        })
        .await
    {
        Ok(_) => Response::success(serde_json::json!({
            "pane_id": pane_id,
            "pty_id": pty_id,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

fn daemon_pane_id(pty: &roux_runtime::pty_service::PtyRecord) -> String {
    match &pty.info.status {
        PtyStatus::RunningAttached { pane_id } => pane_id.clone(),
        _ => pty.info.id.clone(),
    }
}

fn daemon_pane_descriptor(pty: &roux_runtime::pty_service::PtyRecord) -> Value {
    let mut descriptor = serde_json::Map::new();
    descriptor.insert("id".to_string(), Value::String(daemon_pane_id(pty)));
    descriptor.insert(
        "type".to_string(),
        Value::String(if pty.command.is_some() { "command" } else { "shell" }.to_string()),
    );
    descriptor.insert("ptyId".to_string(), Value::String(pty.id.clone()));
    descriptor.insert("workingDir".to_string(), Value::String(pty.working_dir.clone()));
    if let Some(name) = &pty.info.name {
        descriptor.insert("name".to_string(), Value::String(name.clone()));
    }
    if let Some(command) = &pty.command {
        descriptor.insert("command".to_string(), Value::String(command.clone()));
    }
    if let Some(profile) = &pty.info.profile {
        descriptor.insert("profileId".to_string(), Value::String(profile.clone()));
    }
    Value::Object(descriptor)
}

async fn handle_daemon_process_start(req: Request, host: &RuntimeHost) -> Response {
    let Some(command) = req.args.get("command").and_then(|command| command.as_str()) else {
        return Response::err("command required");
    };
    let working_dir = req
        .args
        .get("workingDir")
        .or_else(|| req.args.get("working_dir"))
        .and_then(|working_dir| working_dir.as_str())
        .map(PathBuf::from);

    match host.process_handle.start(command.to_string(), working_dir).await {
        Ok(record) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon process: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_process_output(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let max_bytes = req
        .args
        .get("maxBytes")
        .or_else(|| req.args.get("max_bytes"))
        .and_then(|max_bytes| max_bytes.as_u64())
        .map(|max_bytes| max_bytes as usize)
        .unwrap_or(PROCESS_OUTPUT_DEFAULT_POLL_BYTES);

    match host.process_handle.snapshot(id, max_bytes).await {
        Ok(Some(snapshot)) => match serde_json::to_value(snapshot) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon process output: {err}")),
        },
        Ok(None) => Response::err("daemon process not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_process_list(host: &RuntimeHost) -> Response {
    match host.process_handle.list().await {
        Ok(processes) => match serde_json::to_value(processes) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon processes: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_process_kill(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    match host.process_handle.kill(id).await {
        Ok(Some(record)) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon process: {err}")),
        },
        Ok(None) => Response::err("daemon process not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_spawn_shell(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let request = match parse_pty_spawn_request(&req, host, identity).await {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    match host.pty_handle.spawn_shell(request).await {
        Ok(record) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_spawn_task(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(command) = req.args.get("command").and_then(|command| command.as_str()) else {
        return Response::err("command required");
    };
    let request = match parse_pty_spawn_request(&req, host, identity).await {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    match host.pty_handle.spawn_task(command.to_string(), request).await {
        Ok(record) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_output(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let max_bytes = req
        .args
        .get("maxBytes")
        .or_else(|| req.args.get("max_bytes"))
        .and_then(|max_bytes| max_bytes.as_u64())
        .map(|max_bytes| max_bytes as usize)
        .unwrap_or(PTY_OUTPUT_DEFAULT_POLL_BYTES);

    match host.pty_handle.snapshot(id, max_bytes).await {
        Ok(Some(snapshot)) => match serde_json::to_value(snapshot) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty output: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_list(host: &RuntimeHost) -> Response {
    match host.pty_handle.list().await {
        Ok(ptys) => match serde_json::to_value(ptys) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon ptys: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_write(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let Some(data) = req.args.get("data").and_then(|data| data.as_str()) else {
        return Response::err("data required");
    };
    match host.pty_handle.write(id, data.as_bytes().to_vec()).await {
        Ok(()) => Response::success(serde_json::json!({ "id": id, "bytes": data.len() })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_resize(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let cols = req
        .args
        .get("cols")
        .and_then(|cols| cols.as_u64())
        .and_then(|cols| u16::try_from(cols).ok())
        .unwrap_or(80);
    let rows = req
        .args
        .get("rows")
        .and_then(|rows| rows.as_u64())
        .and_then(|rows| u16::try_from(rows).ok())
        .unwrap_or(24);
    match host.pty_handle.resize(id, cols, rows).await {
        Ok(Some(record)) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_kill(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    match host.pty_handle.kill(id).await {
        Ok(Some(record)) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_detach(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    serialize_daemon_pty_metadata_result(host.pty_handle.detach(id).await)
}

async fn handle_daemon_pty_attach_pane(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let Some(pane_id) = req
        .args
        .get("paneId")
        .or_else(|| req.args.get("pane_id"))
        .and_then(|pane_id| pane_id.as_str())
    else {
        return Response::err("paneId required");
    };
    serialize_daemon_pty_metadata_result(
        host.pty_handle.attach_to_pane(id, pane_id.to_string()).await,
    )
}

async fn handle_daemon_pty_mark_read(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    serialize_daemon_pty_metadata_result(host.pty_handle.mark_read(id).await)
}

async fn handle_daemon_pty_set_name(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let name = req.args.get("name").and_then(|name| {
        if name.is_null() {
            Some(None)
        } else {
            name.as_str().map(|name| Some(name.to_string()))
        }
    });
    let Some(name) = name else {
        return Response::err("name required");
    };
    serialize_daemon_pty_metadata_result(host.pty_handle.set_name(id, name).await)
}

fn serialize_daemon_pty_metadata_result(
    result: Result<
        Option<roux_runtime::pty_service::PtyRecord>,
        roux_runtime::pty_service::PtyServiceError,
    >,
) -> Response {
    match result {
        Ok(Some(record)) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn parse_pty_spawn_request(
    req: &Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Result<PtySpawnRequest, String> {
    let working_dir = req
        .args
        .get("workingDir")
        .or_else(|| req.args.get("working_dir"))
        .and_then(|working_dir| working_dir.as_str())
        .map(PathBuf::from);
    let session_id = req
        .args
        .get("sessionId")
        .or_else(|| req.args.get("session_id"))
        .and_then(|session_id| session_id.as_str())
        .map(str::to_string)
        .or_else(|| req.session_id.clone());
    let pane_id = req
        .args
        .get("paneId")
        .or_else(|| req.args.get("pane_id"))
        .and_then(|pane_id| pane_id.as_str())
        .map(str::to_string)
        .or_else(|| req.pane_id.clone());
    let role = match req.args.get("role").and_then(|role| role.as_str()) {
        Some("sessionPrimary") | Some("session_primary") => roux_core::PtyRole::SessionPrimary,
        _ => roux_core::PtyRole::Secondary,
    };

    let mut request = PtySpawnRequest {
        id: req.args.get("id").and_then(|id| id.as_str()).map(str::to_string),
        working_dir,
        session_id,
        pane_id,
        project_id: req
            .args
            .get("projectId")
            .or_else(|| req.args.get("project_id"))
            .and_then(|project_id| project_id.as_str())
            .map(str::to_string),
        worktree_path: req
            .args
            .get("worktreePath")
            .or_else(|| req.args.get("worktree_path"))
            .and_then(|worktree_path| worktree_path.as_str())
            .map(str::to_string),
        notes: parse_notes_env(&req.args),
        env: parse_pty_env_request(&req.args, identity),
        profile: req.args.get("profile").and_then(|profile| profile.as_str()).map(str::to_string),
        initial_size: parse_initial_size(&req.args),
        role,
    };
    apply_session_spawn_bindings(&mut request, host).await?;
    Ok(request)
}

async fn apply_session_spawn_bindings(
    request: &mut PtySpawnRequest,
    host: &RuntimeHost,
) -> Result<(), String> {
    let Some(session_id) = request.session_id.as_deref() else {
        return Ok(());
    };
    let Some(session) = host.session_handle.get(session_id).await.map_err(|err| err.to_string())?
    else {
        return Ok(());
    };
    if request.project_id.is_none() {
        request.project_id = session.project_id.clone();
    }
    if request.worktree_path.is_none() && session.is_worktree {
        request.worktree_path = Some(session.worktree_path.clone());
    }
    Ok(())
}


fn parse_pty_env_request(args: &Value, identity: &DaemonIdentity) -> PtyEnvRequest {
    let current_exe = std::env::current_exe().ok();
    let cli_path = args
        .get("cliPath")
        .or_else(|| args.get("cli_path"))
        .and_then(|cli_path| cli_path.as_str())
        .map(str::to_string)
        .or_else(|| current_exe.as_ref().map(|path| path.to_string_lossy().into_owned()));
    let cli_bin_dir = args
        .get("cliBinDir")
        .or_else(|| args.get("cli_bin_dir"))
        .and_then(|cli_bin_dir| cli_bin_dir.as_str())
        .map(str::to_string)
        .or_else(|| {
            current_exe
                .as_ref()
                .and_then(|path| path.parent())
                .map(|path| path.to_string_lossy().into_owned())
        });

    PtyEnvRequest {
        user_path: args
            .get("userPath")
            .or_else(|| args.get("user_path"))
            .and_then(|user_path| user_path.as_str())
            .map(str::to_string)
            .or_else(|| std::env::var("PATH").ok()),
        socket_path: args
            .get("socketPath")
            .or_else(|| args.get("socket_path"))
            .and_then(|socket_path| socket_path.as_str())
            .map(str::to_string)
            .or_else(|| Some(identity.endpoint_display())),
        cli_bin_dir,
        cli_path,
        pane_alias: args
            .get("paneAlias")
            .or_else(|| args.get("pane_alias"))
            .and_then(|pane_alias| pane_alias.as_str())
            .map(str::to_string),
    }
}

fn parse_notes_env(args: &Value) -> Option<NotesEnvInputs> {
    let value = args.get("notesEnv").or_else(|| args.get("notes_env"))?;
    Some(NotesEnvInputs {
        vault_root: value
            .get("vaultRoot")
            .or_else(|| value.get("vault_root"))
            .and_then(|root| root.as_str())?
            .to_string(),
        session_slug: value
            .get("sessionSlug")
            .or_else(|| value.get("session_slug"))
            .and_then(|slug| slug.as_str())?
            .to_string(),
        repo_slug: value
            .get("repoSlug")
            .or_else(|| value.get("repo_slug"))
            .and_then(|slug| slug.as_str())?
            .to_string(),
        project_slug: value
            .get("projectSlug")
            .or_else(|| value.get("project_slug"))
            .and_then(|slug| slug.as_str())
            .map(str::to_string),
        context_paths: value
            .get("contextPaths")
            .or_else(|| value.get("context_paths"))
            .and_then(|paths| paths.as_array())
            .map(|paths| {
                paths.iter().filter_map(|path| path.as_str().map(str::to_string)).collect()
            })
            .unwrap_or_default(),
        project_prompt: value
            .get("projectPrompt")
            .or_else(|| value.get("project_prompt"))
            .and_then(|prompt| prompt.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn request_session_id(req: &Request) -> Option<&str> {
    req.session_id
        .as_deref()
        .or_else(|| req.args.get("sessionId").or_else(|| req.args.get("session_id"))?.as_str())
}

fn request_repo_path(req: &Request) -> Option<&str> {
    req.args
        .get("repoPath")
        .or_else(|| req.args.get("repo_path"))
        .and_then(|repo_path| repo_path.as_str())
}

fn optional_string_arg(args: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| args.get(*name))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_nullable_string_arg(args: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| args.get(*name)).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_str().map(str::trim).filter(|value| !value.is_empty()).map(str::to_string)
        }
    })
}

fn bool_arg(args: &Value, names: &[&str]) -> Option<bool> {
    names.iter().find_map(|name| args.get(*name)).and_then(|value| value.as_bool())
}

enum DaemonSessionTarget {
    Repo,
    ExistingWorktree { path: String },
    NewWorktree { branch: String, start_point: Option<String>, fetch_first: bool },
}

fn parse_daemon_session_target(args: &Value) -> DaemonSessionTarget {
    if let Some(path) = args
        .get("worktreePath")
        .or_else(|| args.get("worktree_path"))
        .and_then(|path| path.as_str())
        .filter(|path| !path.trim().is_empty())
    {
        return DaemonSessionTarget::ExistingWorktree { path: path.to_string() };
    }
    if let Some(branch) = args
        .get("branch")
        .or_else(|| args.get("worktreeBranch"))
        .or_else(|| args.get("worktree_branch"))
        .and_then(|branch| branch.as_str())
        .filter(|branch| !branch.trim().is_empty())
    {
        let start_point = args
            .get("base")
            .or_else(|| args.get("startPoint"))
            .or_else(|| args.get("start_point"))
            .and_then(|base| base.as_str())
            .filter(|base| !base.trim().is_empty())
            .map(str::to_string);
        let fetch_first = args
            .get("fetchFirst")
            .or_else(|| args.get("fetch_first"))
            .and_then(|fetch| fetch.as_bool())
            .unwrap_or(false);
        return DaemonSessionTarget::NewWorktree {
            branch: branch.to_string(),
            start_point,
            fetch_first,
        };
    }
    DaemonSessionTarget::Repo
}

async fn resolve_daemon_session_target(
    repo_path: &str,
    target: DaemonSessionTarget,
    settings: &roux_core::RouxSettings,
    hooks: AutomationHookManager,
) -> Result<(String, String, bool), String> {
    match target {
        DaemonSessionTarget::Repo => {
            let branch = get_current_branch(repo_path).unwrap_or_else(|| "main".to_string());
            Ok((repo_path.to_string(), branch, false))
        }
        DaemonSessionTarget::ExistingWorktree { path } => {
            let branch = get_current_branch(&path).unwrap_or_else(|| "main".to_string());
            Ok((path, branch, false))
        }
        DaemonSessionTarget::NewWorktree { branch, start_point, fetch_first } => {
            let wt = resolve_wt_binary(settings);
            let provider = settings.worktree_provider;
            let wt_available = wt.is_some();
            let pre_context = HookContext {
                repo_path: Some(repo_path.to_string()),
                branch: Some(branch.clone()),
                cwd: Some(repo_path.to_string()),
                ..HookContext::new(HookEvent::PreWorktreeCreate)
                    .with_provider(provider, wt_available)
            };
            hooks
                .run_blocking(HookEvent::PreWorktreeCreate, pre_context)
                .await
                .map_err(|err| err.to_string())?;
            if fetch_first {
                roux_core::fetch_origin(repo_path).map_err(|err| err.to_string())?;
            }
            let worktree_path = roux_core::create_worktree_with_provider(
                repo_path,
                &branch,
                settings.worktree_base_path.as_deref(),
                start_point.as_deref(),
                provider,
                wt.as_ref(),
            )
            .map_err(|err| err.to_string())?;
            let context = build_daemon_post_worktree_create_context(
                provider,
                wt_available,
                repo_path,
                &branch,
                &worktree_path,
            );
            hooks.spawn_background(HookEvent::PostWorktreeCreate, context);
            Ok((worktree_path, branch, true))
        }
    }
}

fn load_daemon_settings() -> roux_core::RouxSettings {
    let path = platform::settings_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<roux_core::RouxSettings>(&content).unwrap_or_default().normalized()
    } else {
        roux_core::RouxSettings::default()
    }
}

fn resolve_wt_binary(settings: &roux_core::RouxSettings) -> Option<roux_worktrunk::WtBinary> {
    let override_path =
        settings.worktrunk_binary_path.as_deref().map(str::trim).filter(|p| !p.is_empty());
    match override_path {
        Some(path) => roux_worktrunk::detect_wt(Some(path)),
        None => roux_worktrunk::detect_wt(None),
    }
}

fn get_current_branch(repo_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn list_branches(repo_path: &str) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Failed to list branches: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_string)
        .collect())
}

fn git_init(path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .map_err(|err| format!("Failed to run git init: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn is_git_repo(path: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn parse_initial_size(args: &Value) -> Option<(u16, u16)> {
    let value = args.get("initialSize").or_else(|| args.get("initial_size"))?;
    let array = value.as_array()?;
    let cols = array.first()?.as_u64().and_then(|cols| u16::try_from(cols).ok())?;
    let rows = array.get(1)?.as_u64().and_then(|rows| u16::try_from(rows).ok())?;
    Some((cols, rows))
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn daemon_auth_token(endpoint: &platform::SocketEndpoint) -> Result<Option<String>, String> {
    match endpoint {
        platform::SocketEndpoint::Unix(_) => Ok(None),
        platform::SocketEndpoint::Tcp(addr) => {
            if let Some(token) = daemon_env_auth_token() {
                return Ok(Some(token));
            }

            #[cfg(windows)]
            {
                Ok(Some(format!("{}-{}", std::process::id(), unix_now_ms())))
            }

            #[cfg(not(windows))]
            {
                Err(format!("TCP daemon bind tcp://{addr} requires ROUX_DAEMON_TOKEN"))
            }
        }
    }
}

fn daemon_env_auth_token() -> Option<String> {
    for key in ["ROUX_DAEMON_TOKEN", "ROUX_AUTH_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

async fn wait_for_shutdown_signal(mut daemon_stop_rx: watch::Receiver<bool>) -> Result<(), String> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|err| format!("failed to install SIGTERM handler: {err}"))?;

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|err| format!("failed to wait for SIGINT: {err}"))?;
            }
            _ = sigterm.recv() => {}
            _ = wait_for_daemon_stop(&mut daemon_stop_rx) => {}
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|err| format!("failed to wait for shutdown signal: {err}"))
            }
            _ = wait_for_daemon_stop(&mut daemon_stop_rx) => Ok(()),
        }
    }
}

async fn wait_for_daemon_stop(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow_and_update() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests;
