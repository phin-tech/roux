use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use roux_runtime::host::RuntimeHost;

use crate::paths;

use super::load_daemon_settings;
use super::protocol::{Request, Response};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonNotesTarget {
    scope: String,
    session_id: Option<String>,
    topic: Option<String>,
    #[allow(dead_code)]
    override_slug: Option<String>,
    vault_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonNotesWriteArgs {
    target: DaemonNotesTarget,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
    vault_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonNotesAppendArgs {
    target: DaemonNotesTarget,
    content: String,
    #[serde(default)]
    timestamped: bool,
    #[serde(default)]
    tags: Vec<String>,
    vault_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonNotesPathArgs {
    target: DaemonNotesTarget,
    #[serde(default)]
    dir: bool,
    vault_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonNotesSearchQuery {
    tags: Vec<String>,
    scope: Option<String>,
    #[serde(default)]
    exact: bool,
    vault_root: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DaemonNotesRead {
    path: String,
    content: String,
}

fn serialize_response<T: Serialize>(value: T, label: &str) -> Response {
    match serde_json::to_value(value) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize {label}: {err}")),
    }
}

pub(super) async fn handle_notes_read(req: Request, host: &RuntimeHost) -> Response {
    let target: DaemonNotesTarget = match serde_json::from_value(req.args.clone()) {
        Ok(target) => target,
        Err(err) => return Response::err(format!("invalid notes-read args: {err}")),
    };
    let mut svc = build_daemon_notes_service(target.vault_root.as_deref());
    let (scope, topic, session_slug) =
        match resolve_daemon_notes_target(&mut svc, host, &target).await {
            Ok(resolved) => resolved,
            Err(err) => return Response::err(err),
        };
    let content = match svc.read_file(&scope, topic.as_deref(), &session_slug) {
        Ok(content) => content,
        Err(err) => return Response::err(err.to_string()),
    };
    let path = match svc.file_path(&scope, topic.as_deref(), &session_slug) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(err) => return Response::err(err.to_string()),
    };
    serialize_response(DaemonNotesRead { path, content }, "notes read")
}

pub(super) async fn handle_notes_write(req: Request, host: &RuntimeHost) -> Response {
    let args: DaemonNotesWriteArgs = match serde_json::from_value(req.args.clone()) {
        Ok(args) => args,
        Err(err) => return Response::err(format!("invalid notes-write args: {err}")),
    };
    let vault_root = args.vault_root.as_deref().or(args.target.vault_root.as_deref());
    let mut svc = build_daemon_notes_service(vault_root);
    let (scope, topic, session_slug) =
        match resolve_daemon_notes_target(&mut svc, host, &args.target).await {
            Ok(resolved) => resolved,
            Err(err) => return Response::err(err),
        };
    let now = notes_now_iso8601();
    match svc.write_file(&scope, topic.as_deref(), &session_slug, &args.content, &now, &args.tags) {
        Ok(()) => Response::success(serde_json::json!({})),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_notes_append(req: Request, host: &RuntimeHost) -> Response {
    let args: DaemonNotesAppendArgs = match serde_json::from_value(req.args.clone()) {
        Ok(args) => args,
        Err(err) => return Response::err(format!("invalid notes-append args: {err}")),
    };
    let vault_root = args.vault_root.as_deref().or(args.target.vault_root.as_deref());
    let mut svc = build_daemon_notes_service(vault_root);
    let (scope, topic, session_slug) =
        match resolve_daemon_notes_target(&mut svc, host, &args.target).await {
            Ok(resolved) => resolved,
            Err(err) => return Response::err(err),
        };
    let now = notes_now_iso8601();
    let ts = notes_now_short();
    let id = short_notes_entry_id();
    let include_web_anchor = load_daemon_settings().notes_include_web_anchors;
    let opts = if args.timestamped {
        roux_runtime::notes_service::AppendOpts::Timestamped {
            timestamp: &ts,
            id: &id,
            include_web_anchor,
        }
    } else {
        roux_runtime::notes_service::AppendOpts::Plain
    };
    match svc.append_file(
        &scope,
        topic.as_deref(),
        &session_slug,
        &args.content,
        opts,
        &now,
        &args.tags,
    ) {
        Ok(()) => Response::success(serde_json::json!({})),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_notes_path(req: Request, host: &RuntimeHost) -> Response {
    let args: DaemonNotesPathArgs = match serde_json::from_value(req.args.clone()) {
        Ok(args) => args,
        Err(err) => return Response::err(format!("invalid notes-path args: {err}")),
    };
    let vault_root = args.vault_root.as_deref().or(args.target.vault_root.as_deref());
    let mut svc = build_daemon_notes_service(vault_root);
    let (scope, topic, session_slug) =
        match resolve_daemon_notes_target(&mut svc, host, &args.target).await {
            Ok(resolved) => resolved,
            Err(err) => return Response::err(err),
        };
    let path = if args.dir {
        svc.dir_path(&scope, &session_slug)
    } else {
        match svc.file_path(&scope, topic.as_deref(), &session_slug) {
            Ok(path) => path,
            Err(err) => return Response::err(err.to_string()),
        }
    };
    Response::success(Value::String(path.to_string_lossy().into_owned()))
}

pub(super) async fn handle_notes_search(req: Request) -> Response {
    let query: DaemonNotesSearchQuery = match serde_json::from_value(req.args.clone()) {
        Ok(query) => query,
        Err(err) => return Response::err(format!("invalid notes-search args: {err}")),
    };
    if query.tags.is_empty() {
        return Response::err("at least one --tag is required");
    }
    let svc = build_daemon_notes_service(query.vault_root.as_deref());
    let paths: Vec<String> = svc
        .search(query.scope.as_deref(), &query.tags, query.exact)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    serialize_response(paths, "notes search")
}

pub(super) async fn handle_notes_vault_root(req: Request) -> Response {
    Response::success(Value::String(
        daemon_notes_vault_root(req.args.get("vaultRoot").and_then(|root| root.as_str()))
            .to_string_lossy()
            .into_owned(),
    ))
}

fn build_daemon_notes_service(
    root_override: Option<&str>,
) -> roux_runtime::notes_service::NotesService {
    roux_runtime::notes_service::NotesService::new(daemon_notes_vault_root(root_override))
}

fn daemon_notes_vault_root(root_override: Option<&str>) -> PathBuf {
    if let Some(root) = root_override.map(str::trim).filter(|root| !root.is_empty()) {
        return PathBuf::from(root);
    }
    let settings = load_daemon_settings();
    settings
        .notes_vault_root
        .filter(|root| !root.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(paths::default_notes_vault_root)
}

async fn resolve_daemon_notes_target(
    svc: &mut roux_runtime::notes_service::NotesService,
    host: &RuntimeHost,
    target: &DaemonNotesTarget,
) -> Result<(roux_runtime::notes_service::Scope, Option<String>, String), String> {
    let (session, project_name, remote) =
        load_daemon_notes_session_context(host, target.session_id.as_deref()).await?;
    svc.resolve_target(
        &target.scope,
        session.as_ref(),
        project_name.as_deref(),
        remote.as_deref(),
        target.topic.clone(),
    )
}

async fn load_daemon_notes_session_context(
    host: &RuntimeHost,
    session_id: Option<&str>,
) -> Result<(Option<roux_core::Session>, Option<String>, Option<String>), String> {
    let Some(session_id) = session_id else {
        return Ok((None, None, None));
    };
    let Some(session) = host.session_handle.get(session_id).await.map_err(|err| err.to_string())?
    else {
        return Ok((None, None, None));
    };
    let project_name = match session.project_id.as_deref() {
        Some(project_id) => host
            .project_handle
            .list()
            .await
            .map_err(|err| err.to_string())?
            .into_iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name),
        None => None,
    };
    let remote = daemon_git_origin_url(&session.repo_root);
    Ok((Some(session), project_name, remote))
}

fn daemon_git_origin_url(repo_root: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!url.is_empty()).then_some(url)
}

fn notes_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format_epoch_seconds_iso8601(secs)
}

fn notes_now_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format_epoch_seconds_short(secs)
}

fn short_notes_entry_id() -> String {
    uuid::Uuid::new_v4().simple().to_string().chars().take(8).collect()
}

fn format_epoch_seconds_iso8601(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn format_epoch_seconds_short(secs: u64) -> String {
    let (year, month, day, hour, minute, _second) = epoch_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

fn epoch_to_ymdhms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;
    let second = (rem % 60) as u32;
    let (year, month, day) = civil_from_days(days + 719468);
    (year, month, day, hour, minute, second)
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month, day)
}
