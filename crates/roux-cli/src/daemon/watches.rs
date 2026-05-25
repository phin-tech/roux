use std::fmt;

use roux_core::{CreateWatchConfig, RuntimeState, Watch};
use roux_runtime::host::RuntimeHost;
use roux_runtime::watch_runner::WatchRunner;

use super::protocol::{Request, Response};
use super::unix_now_ms;

pub(super) async fn handle_watch_list(host: &RuntimeHost) -> Response {
    match host.watch_handle.list().await {
        Ok(watches) => match serde_json::to_value(watches) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize watches: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_watch_create(req: Request, watch_runner: &WatchRunner) -> Response {
    let config = match parse_watch_config(&req) {
        Ok(config) => config,
        Err(err) => return Response::err(err.to_string()),
    };
    let watch = watch_from_config(config);
    match watch_runner.add_watch(watch).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_watch_find_or_create(
    req: Request,
    watch_runner: &WatchRunner,
) -> Response {
    let config = match parse_watch_config(&req) {
        Ok(config) => config,
        Err(err) => return Response::err(err.to_string()),
    };
    let watch = watch_from_config(config);
    match watch_runner.find_or_add_github_pr(watch).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_watch_remove(req: Request, watch_runner: &WatchRunner) -> Response {
    let Some(id) = request_watch_id(&req) else {
        return Response::err("id required");
    };
    let id = id.to_string();
    match watch_runner.remove_watch(&id).await {
        Ok(()) => Response::success(serde_json::json!({ "id": id })),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_watch_pause(req: Request, watch_runner: &WatchRunner) -> Response {
    let Some(id) = request_watch_id(&req) else {
        return Response::err("id required");
    };
    match watch_runner.pause_watch(id).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_watch_resume(req: Request, watch_runner: &WatchRunner) -> Response {
    let Some(id) = request_watch_id(&req) else {
        return Response::err("id required");
    };
    match watch_runner.resume_watch(id).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_watch_replace(req: Request, watch_runner: &WatchRunner) -> Response {
    let Some(value) = req.args.get("watch").cloned() else {
        return Response::err("watch required");
    };
    let watch: Watch = match serde_json::from_value(value) {
        Ok(watch) => watch,
        Err(err) => return Response::err(format!("invalid watch: {err}")),
    };
    match watch_runner.replace_watch(watch).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err),
    }
}

pub(super) async fn handle_watch_remove_for_session(
    req: Request,
    watch_runner: &WatchRunner,
) -> Response {
    let Some(session_id) = req
        .args
        .get("sessionId")
        .or_else(|| req.args.get("session_id"))
        .and_then(|session_id| session_id.as_str())
    else {
        return Response::err("sessionId required");
    };
    match watch_runner.remove_watches_for_session(session_id).await {
        Ok(removed) => Response::success(serde_json::json!({
            "sessionId": session_id,
            "removed": removed,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_watch_cleanup_orphans(
    host: &RuntimeHost,
    watch_runner: &WatchRunner,
) -> Response {
    let sessions = match host.session_handle.list().await {
        Ok(sessions) => sessions,
        Err(err) => return Response::err(err.to_string()),
    };
    let projects = match host.project_handle.list().await {
        Ok(projects) => projects,
        Err(err) => return Response::err(err.to_string()),
    };
    let session_ids = sessions.into_iter().map(|session| session.id).collect();
    let project_ids = projects.into_iter().map(|project| project.id).collect();
    match watch_runner.cleanup_orphans(session_ids, project_ids).await {
        Ok(removed) => Response::success(serde_json::json!({ "removed": removed })),
        Err(err) => Response::err(err),
    }
}

#[derive(Debug)]
struct ParseWatchConfigError {
    source: serde_json::Error,
}

impl fmt::Display for ParseWatchConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid watch config: {}", self.source)
    }
}

impl std::error::Error for ParseWatchConfigError {}

impl From<serde_json::Error> for ParseWatchConfigError {
    fn from(source: serde_json::Error) -> Self {
        Self { source }
    }
}

fn parse_watch_config(req: &Request) -> Result<CreateWatchConfig, ParseWatchConfigError> {
    let value = req.args.get("config").cloned().unwrap_or_else(|| req.args.clone());
    serde_json::from_value(value).map_err(ParseWatchConfigError::from)
}

fn watch_from_config(config: CreateWatchConfig) -> Watch {
    Watch {
        id: uuid::Uuid::new_v4().to_string(),
        name: config.name,
        kind: config.kind,
        mode: config.mode,
        scope: config.scope,
        runtime_state: RuntimeState::Active,
        last_result: None,
        last_checked: None,
        notify: config.notify.unwrap_or_default(),
        created_at: unix_now_ms(),
    }
}

fn request_watch_id(req: &Request) -> Option<&str> {
    req.args.get("id").and_then(|id| id.as_str())
}

fn serialize_watch(watch: Watch) -> Response {
    match serde_json::to_value(watch) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize watch: {err}")),
    }
}
