use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(windows)]
use tokio::net::TcpListener;
#[cfg(not(windows))]
use tokio::net::UnixListener;

use crate::commands::notes::{self as notes_cmd, NotesSearchQuery, NotesTarget};
use crate::platform;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
struct Request {
    command: String,
    session_id: Option<String>,
    pane_id: Option<String>,
    #[allow(dead_code)]
    auth_token: Option<String>,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn success(data: serde_json::Value) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }

    fn ok() -> Self {
        Self { ok: true, data: None, error: None }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }
}

pub fn socket_path() -> PathBuf {
    platform::socket_path()
}

pub fn start_socket_server(app: tauri::AppHandle) {
    let path = socket_path();

    tauri::async_runtime::spawn(async move {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        #[cfg(not(windows))]
        let listener = {
            let _ = fs::remove_file(&path);

            match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    rlog!("Failed to bind socket at {:?}: {}", path, e);
                    return;
                }
            }
        };

        #[cfg(windows)]
        let listener = {
            let listener = match TcpListener::bind("127.0.0.1:0").await {
                Ok(l) => l,
                Err(e) => {
                    rlog!("Failed to bind socket server on localhost: {}", e);
                    return;
                }
            };

            let addr = match listener.local_addr() {
                Ok(addr) => addr.to_string(),
                Err(e) => {
                    rlog!("Failed to resolve socket listener address: {}", e);
                    return;
                }
            };

            if let Some(parent) = platform::socket_addr_file_path().parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::write(platform::socket_addr_file_path(), &addr) {
                rlog!("Failed to write socket address file: {}", e);
                return;
            }
            let auth_token = uuid::Uuid::new_v4().to_string();
            if let Err(e) = fs::write(platform::socket_auth_token_file_path(), &auth_token) {
                rlog!("Failed to write socket auth token file: {}", e);
                return;
            }

            rlog!("Socket server listening on {}", addr);
            listener
        };

        // Set permissions to owner-only (0600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }

        #[cfg(not(windows))]
        rlog!("Socket server listening on {:?}", path);

        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    rlog!("Socket accept error: {}", e);
                    continue;
                }
            };

            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut buf_reader = BufReader::new(reader);
                let mut line = String::new();

                match buf_reader.read_line(&mut line).await {
                    Ok(0) => return,
                    Ok(_) => {}
                    Err(e) => {
                        rlog!("Socket read error: {}", e);
                        return;
                    }
                }

                let response = match serde_json::from_str::<Request>(line.trim()) {
                    Ok(req) => handle_request(req, &app).await,
                    Err(e) => Response::err(format!("Invalid request: {}", e)),
                };

                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writer.write_all(json.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
                let _ = writer.shutdown().await;
            });
        }
    });
}

async fn handle_request(req: Request, app: &tauri::AppHandle) -> Response {
    #[cfg(windows)]
    {
        let Some(expected_token) = platform::load_socket_auth_token() else {
            return Response::err("Socket auth token unavailable");
        };

        if req.auth_token.as_deref() != Some(expected_token.as_str()) {
            return Response::err("unauthorized");
        }
    }

    match req.command.as_str() {
        "split" => handle_split(req, app),
        "session-create" => handle_session_create(req, app).await,
        "shell" => handle_shell(req, app).await,
        "focus" => handle_focus(req, app),
        "run" => handle_run(req, app).await,
        "send" => handle_send(req, app).await,
        "notify" => handle_notify(req, app).await,
        "notes-read" => handle_notes_read(req, app).await,
        "notes-write" => handle_notes_write(req, app).await,
        "notes-append" => handle_notes_append(req, app).await,
        "notes-path" => handle_notes_path(req, app).await,
        "notes-search" => handle_notes_search(req, app).await,
        "notes-vault-root" => handle_notes_vault_root(app),
        "hook-show" => handle_hook_show(req, app).await,
        "hook-run" => handle_hook_run(req, app).await,
        "session-list" => handle_session_list(req, app).await,
        "session-poll" => handle_session_poll(req, app).await,
        "session-panes-list" => handle_session_panes_list(req, app).await,
        "session-panes-create" => handle_session_panes_create(req, app).await,
        "app-open" => handle_app_open(req, app).await,
        _ => Response::err(format!("unknown command: {}", req.command)),
    }
}

async fn handle_hook_show(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let repo_path = req.args.get("repo_path").and_then(|v| v.as_str());
    match state.automation_hooks.list_hooks(repo_path) {
        Ok(items) => Response::success(crate::automation_hooks::hook_list_to_value(items)),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_hook_run(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let request = match crate::automation_hooks::request_from_socket_args(req.args) {
        Ok(request) => request,
        Err(e) => return Response::err(e),
    };
    let settings = match state.settings.lock() {
        Ok(settings) => settings.clone(),
        Err(e) => return Response::err(e.to_string()),
    };
    let wt_available = crate::services::setup::resolve_wt_binary().is_some();
    let (event, context) = match crate::automation_hooks::context_from_run_request(
        request,
        Some(settings.worktree_provider),
        wt_available,
    ) {
        Ok(parts) => parts,
        Err(e) => return Response::err(e.to_string()),
    };
    let result = if event.is_blocking() {
        state.automation_hooks.run_blocking(event, context).await
    } else {
        state.automation_hooks.run_background(event, context).await
    };
    match result {
        Ok(ran) => Response::success(crate::automation_hooks::hook_run_to_value(
            crate::automation_hooks::HookRunSummary { event: event.as_str().into(), ran },
        )),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_notes_read(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let target: NotesTarget = match serde_json::from_value(req.args.clone()) {
        Ok(t) => t,
        Err(e) => return Response::err(format!("invalid notes-read args: {e}")),
    };
    match notes_cmd::do_notes_read(target, &state).await {
        Ok(r) => match serde_json::to_value(&r) {
            Ok(v) => Response::success(v),
            Err(e) => Response::err(format!("serialize notes read: {e}")),
        },
        Err(e) => Response::err(e),
    }
}

#[derive(Debug, Deserialize)]
struct NotesWriteArgs {
    target: NotesTarget,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn handle_notes_write(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let args: NotesWriteArgs = match serde_json::from_value(req.args.clone()) {
        Ok(a) => a,
        Err(e) => return Response::err(format!("invalid notes-write args: {e}")),
    };
    match notes_cmd::do_notes_write(args.target, args.content, args.tags, &state, app).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::err(e),
    }
}

#[derive(Debug, Deserialize)]
struct NotesAppendArgs {
    target: NotesTarget,
    content: String,
    #[serde(default)]
    timestamped: bool,
    #[serde(default)]
    tags: Vec<String>,
}

async fn handle_notes_append(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let args: NotesAppendArgs = match serde_json::from_value(req.args.clone()) {
        Ok(a) => a,
        Err(e) => return Response::err(format!("invalid notes-append args: {e}")),
    };
    match notes_cmd::do_notes_append(
        args.target,
        args.content,
        args.timestamped,
        args.tags,
        &state,
        app,
    )
    .await
    {
        Ok(()) => Response::ok(),
        Err(e) => Response::err(e),
    }
}

#[derive(Debug, Deserialize)]
struct NotesPathArgs {
    target: NotesTarget,
    #[serde(default)]
    dir: bool,
}

async fn handle_notes_path(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let args: NotesPathArgs = match serde_json::from_value(req.args.clone()) {
        Ok(a) => a,
        Err(e) => return Response::err(format!("invalid notes-path args: {e}")),
    };
    match notes_cmd::do_notes_path(args.target, args.dir, &state).await {
        Ok(p) => Response::success(serde_json::Value::String(p)),
        Err(e) => Response::err(e),
    }
}

async fn handle_notes_search(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let query: NotesSearchQuery = match serde_json::from_value(req.args.clone()) {
        Ok(q) => q,
        Err(e) => return Response::err(format!("invalid notes-search args: {e}")),
    };
    match notes_cmd::do_notes_search(query, &state) {
        Ok(paths) => match serde_json::to_value(&paths) {
            Ok(v) => Response::success(v),
            Err(e) => Response::err(format!("serialize notes search: {e}")),
        },
        Err(e) => Response::err(e),
    }
}

fn handle_notes_vault_root(app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    Response::success(serde_json::Value::String(notes_cmd::do_notes_vault_root(&state)))
}

async fn handle_session_list(_req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    match state.session_handle.list().await {
        Ok(sessions) => match serde_json::to_value(&sessions) {
            Ok(v) => Response::success(v),
            Err(e) => Response::err(format!("failed to serialize sessions: {}", e)),
        },
        Err(e) => Response::err(format!("{}", e)),
    }
}

async fn handle_session_poll(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id,
        None => return Response::err("session_id required"),
    };
    let state: tauri::State<AppState> = app.state();
    match state.session_handle.get(session_id).await {
        Ok(Some(s)) => match serde_json::to_value(&s) {
            Ok(v) => Response::success(v),
            Err(e) => Response::err(format!("failed to serialize session: {}", e)),
        },
        Ok(None) => Response::err("session not found"),
        Err(e) => Response::err(format!("{}", e)),
    }
}

/// Register a pending-reply oneshot channel and return its request_id + receiver.
/// Pure over the `PendingReplies` map so it's testable without an AppState.
fn register_pending_reply_in(
    map: &crate::state::PendingReplies,
) -> (String, tokio::sync::oneshot::Receiver<serde_json::Value>) {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    map.lock().unwrap().insert(request_id.clone(), tx);
    (request_id, rx)
}

fn register_pending_reply(
    app: &tauri::AppHandle,
) -> (String, tokio::sync::oneshot::Receiver<serde_json::Value>) {
    let state: tauri::State<AppState> = app.state();
    register_pending_reply_in(&state.pending_replies)
}

fn drop_pending_reply_in(map: &crate::state::PendingReplies, request_id: &str) {
    map.lock().unwrap().remove(request_id);
}

fn drop_pending_reply(app: &tauri::AppHandle, request_id: &str) {
    let state: tauri::State<AppState> = app.state();
    drop_pending_reply_in(&state.pending_replies, request_id);
}

async fn await_frontend_reply_in(
    map: &crate::state::PendingReplies,
    request_id: String,
    rx: tokio::sync::oneshot::Receiver<serde_json::Value>,
    timeout_ms: u64,
) -> Response {
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), rx).await {
        Ok(Ok(data)) => {
            // The frontend conventionally replies with `{ error: "..." }` on
            // failure so the CLI sees a non-zero exit, rather than succeeding
            // with a bogus payload. Any other shape is treated as success.
            if let Some(err) = data.get("error").and_then(|e| e.as_str()) {
                Response::err(err.to_string())
            } else {
                Response::success(data)
            }
        }
        Ok(Err(_)) => {
            drop_pending_reply_in(map, &request_id);
            Response::err("frontend dropped reply channel")
        }
        Err(_) => {
            drop_pending_reply_in(map, &request_id);
            Response::err("timed out waiting for frontend reply")
        }
    }
}

async fn await_frontend_reply(
    app: &tauri::AppHandle,
    request_id: String,
    rx: tokio::sync::oneshot::Receiver<serde_json::Value>,
    timeout_ms: u64,
) -> Response {
    let state: tauri::State<AppState> = app.state();
    // Need to clone the Arc-like Mutex handle reference but tauri::State is a ref;
    // bind explicitly so the borrow lives through the await.
    let map = &state.pending_replies;
    await_frontend_reply_in(map, request_id, rx, timeout_ms).await
}

async fn handle_session_panes_list(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id.to_string(),
        None => return Response::err("session_id required"),
    };

    // Verify the session exists before asking the frontend — otherwise a bad
    // id would return a successful empty snapshot, which is a confusing UX
    // for CLI scripts ("session not found" beats a misleading empty list).
    {
        let state: tauri::State<AppState> = app.state();
        match state.session_handle.get(&session_id).await {
            Ok(Some(_)) => {}
            Ok(None) => return Response::err("session not found"),
            Err(e) => return Response::err(format!("{}", e)),
        }
    }

    let (request_id, rx) = register_pending_reply(app);

    use tauri::Emitter;
    let cmd = roux_core::RouxCommand::new("panes-list-request")
        .session_id(&session_id)
        .request_id(&request_id);
    if let Err(e) = app.emit("roux-command", &cmd) {
        drop_pending_reply(app, &request_id);
        return Response::err(format!("failed to emit event: {}", e));
    }

    await_frontend_reply(app, request_id, rx, 2_000).await
}

async fn handle_session_panes_create(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id.to_string(),
        None => return Response::err("session_id required"),
    };

    // Verify session exists before round-tripping to the frontend.
    {
        let state: tauri::State<AppState> = app.state();
        match state.session_handle.get(&session_id).await {
            Ok(Some(_)) => {}
            Ok(None) => return Response::err("session not found"),
            Err(e) => return Response::err(format!("{}", e)),
        }
    }

    let profile =
        req.args.get("profile").and_then(|p| p.as_str()).unwrap_or("plain-shell").to_string();

    // Validate against known builtins + user profiles. Rejects typos like
    // "shell" (correct id is "plain-shell") instead of silently falling back.
    {
        let state: tauri::State<AppState> = app.state();
        let settings = state.settings.lock().unwrap().clone();
        if let Err(e) = validate_profile_id(&profile, &settings) {
            return Response::err(e);
        }
    }

    let direction =
        req.args.get("direction").and_then(|d| d.as_str()).unwrap_or("horizontal").to_string();
    if direction != "horizontal" && direction != "vertical" {
        return Response::err("direction must be horizontal or vertical");
    }
    let working_dir = req.args.get("working_dir").and_then(|d| d.as_str()).map(String::from);

    let (request_id, rx) = register_pending_reply(app);

    use tauri::Emitter;
    let mut cmd = roux_core::RouxCommand::new("pane-create")
        .session_id(&session_id)
        .direction(&direction)
        .profile_id(&profile)
        .request_id(&request_id);
    if let Some(ref wd) = working_dir {
        cmd = cmd.working_dir(wd);
    }
    if let Err(e) = app.emit("roux-command", &cmd) {
        drop_pending_reply(app, &request_id);
        return Response::err(format!("failed to emit event: {}", e));
    }

    await_frontend_reply(app, request_id, rx, 5_000).await
}

fn bring_window_to_front(app: &tauri::AppHandle) {
    for window in app.webview_windows().values() {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Validate that a profile id is known — either a built-in or a user-defined
/// profile in settings. Returns `Ok(())` on match, or a descriptive error
/// listing every known id so a CLI caller can correct typos like "shell"
/// (the correct built-in id is "plain-shell").
fn validate_profile_id(id: &str, settings: &crate::settings::RouxSettings) -> Result<(), String> {
    let builtin_ids: Vec<String> =
        crate::providers::builtin_profiles(settings).into_iter().map(|p| p.id).collect();
    if builtin_ids.iter().any(|b| b == id) {
        return Ok(());
    }
    let user_ids: Vec<String> = settings.spawn_profiles.iter().map(|p| p.id.clone()).collect();
    if user_ids.iter().any(|u| u == id) {
        return Ok(());
    }
    let mut msg =
        format!("unknown profile id '{}'. Known built-ins: {}", id, builtin_ids.join(", "));
    if !user_ids.is_empty() {
        msg.push_str(&format!(". User profiles: {}", user_ids.join(", ")));
    }
    Err(msg)
}

/// Canonicalize a path to an absolute form, falling back to the input if
/// the path doesn't exist on disk. Used to compare directory arguments
/// against persisted session paths without false negatives from `.`,
/// trailing slashes, or symlink differences.
fn canonicalize_or_passthrough(path: &str) -> String {
    let p = std::path::PathBuf::from(path);
    p.canonicalize().map(|c| c.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string())
}

/// Find the first session whose worktree_path or repo_root matches `path`.
/// Compares canonicalized forms so `/foo/bar`, `/foo/bar/`, and a symlink
/// resolving to the same directory all match. Extracted for unit tests.
fn find_session_for_path(
    sessions: &[crate::session::Session],
    path: &str,
) -> Option<crate::session::Session> {
    let target = canonicalize_or_passthrough(path);
    sessions
        .iter()
        .find(|s| {
            canonicalize_or_passthrough(&s.worktree_path) == target
                || canonicalize_or_passthrough(&s.repo_root) == target
        })
        .cloned()
}

/// Derive a default session name from a directory path (basename, or
/// "New Session" if the path has no basename).
fn default_session_name_for_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "New Session".to_string())
}

async fn handle_app_open(req: Request, app: &tauri::AppHandle) -> Response {
    use crate::services::sessions::{self as svc, SessionTarget};

    let path = match req.args.get("path").and_then(|p| p.as_str()) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => return Response::err("path required"),
    };

    let state: tauri::State<AppState> = app.state();
    let handle = state.session_handle.clone();

    let sessions = match handle.list().await {
        Ok(s) => s,
        Err(e) => return Response::err(format!("{}", e)),
    };

    // Match against worktree_path or repo_root (first match wins).
    if let Some(existing) = find_session_for_path(&sessions, &path) {
        use tauri::Emitter;
        let _ = app
            .emit("roux-command", &roux_core::RouxCommand::new("focus").session_id(&existing.id));
        bring_window_to_front(app);
        return Response::success(serde_json::json!({
            "session_id": existing.id,
            "created": false,
            "focused": true,
        }));
    }

    // No match — create a new session at this path.
    let name = default_session_name_for_path(&path);

    let settings = state.settings.lock().unwrap().clone();

    let session = match svc::create_session_shell(
        &state.pty_manager,
        &state.session_handle,
        &state.project_handle,
        &settings,
        &path,
        &name,
        SessionTarget::Repo,
        None,
        None, // profile - CLI-initiated, frontend will set via profile runner
        // CLI-initiated sessions have no pane context yet.
        None,
        None, // project_id - CLI sessions are unattached
        None, // blueprint_id
        Some(&state.automation_hooks),
        app,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return Response::err(format!("{}", e)),
    };

    use tauri::Emitter;
    let _ = app.emit(
        "roux-command",
        &roux_core::RouxCommand::new("session-created").session_id(&session.id),
    );

    bring_window_to_front(app);

    Response::success(serde_json::json!({
        "session_id": session.id,
        "created": true,
        "focused": true,
    }))
}

fn handle_split(req: Request, app: &tauri::AppHandle) -> Response {
    let direction = req.args.get("direction").and_then(|d| d.as_str()).unwrap_or("horizontal");

    if direction != "horizontal" && direction != "vertical" {
        return Response::err(format!(
            "invalid direction: {}, must be horizontal or vertical",
            direction
        ));
    }

    let session_id = match req.session_id.as_deref() {
        Some(id) => id,
        None => return Response::err("session_id required"),
    };

    use tauri::Emitter;
    let mut cmd = roux_core::RouxCommand::new("split").session_id(session_id).direction(direction);
    if let Some(ref pane_id) = req.pane_id {
        cmd = cmd.pane_id(pane_id);
    }
    let _ = app.emit("roux-command", &cmd);

    Response::ok()
}

async fn handle_session_create(req: Request, app: &tauri::AppHandle) -> Response {
    use crate::services::sessions::{self as svc, SessionTarget};

    let state: tauri::State<AppState> = app.state();
    let handle = state.session_handle.clone();

    let name = req.args.get("name").and_then(|n| n.as_str()).unwrap_or("New Session").to_string();
    let working_dir = req.args.get("working_dir").and_then(|d| d.as_str()).map(|s| s.to_string());
    let worktree_branch =
        req.args.get("worktree_branch").and_then(|d| d.as_str()).map(|s| s.to_string());
    let profile = req.args.get("profile").and_then(|p| p.as_str()).unwrap_or("claude").to_string();
    let flags: Vec<String> = req
        .args
        .get("flags")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let nono_profile = req.args.get("nono_profile").and_then(|p| p.as_str()).map(|s| s.to_string());
    let nono_allow_dirs: Vec<String> = req
        .args
        .get("nono_allow_dirs")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    // Resolve repo_path: use the requesting session's repo_root, or the working_dir
    let repo_path = match req.session_id.as_deref() {
        Some(id) => match handle.get(id).await {
            Ok(Some(session)) => session.repo_root.clone(),
            _ => working_dir.clone().unwrap_or_default(),
        },
        None => working_dir.clone().unwrap_or_default(),
    };

    if repo_path.is_empty() {
        return Response::err("working_dir, worktree_branch, or session_id required");
    }

    let settings = state.settings.lock().unwrap().clone();

    if let Err(e) = validate_profile_id(&profile, &settings) {
        return Response::err(e);
    }

    // Build the session target. worktree_branch wins; else treat a distinct
    // working_dir as an existing worktree; else use the repo directly.
    let target = if let Some(branch) = worktree_branch.as_deref() {
        SessionTarget::NewWorktree { branch, start_point: None, fetch_first: false }
    } else {
        match &working_dir {
            Some(dir) if dir != &repo_path => SessionTarget::ExistingWorktree { path: dir },
            _ => SessionTarget::Repo,
        }
    };

    // `flags` were only meaningful for the legacy Claude spawn path
    // (passed directly as args to the claude binary). That path is gone;
    // flags now belong on a SpawnProfile's `startup_command` or
    // `additional_flags`. Reject them here rather than silently dropping.
    if !flags.is_empty() {
        return Response::err(
            "--flag/-f is no longer supported at session creation; bake flags into a spawn profile's startup_command instead",
        );
    }

    let nono_config = nono_profile.as_ref().map(|p| crate::pty::NonoConfig {
        profile: p.clone(),
        allow_dirs: nono_allow_dirs.clone(),
    });

    let session = match svc::create_session_shell(
        &state.pty_manager,
        &state.session_handle,
        &state.project_handle,
        &settings,
        &repo_path,
        &name,
        target,
        nono_config.as_ref(),
        Some(&profile),
        None,
        None, // project_id - CLI sessions are unattached
        None, // blueprint_id
        Some(&state.automation_hooks),
        app,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return Response::err(format!("{}", e)),
    };

    let session_id = session.id.clone();

    use tauri::Emitter;
    let mut cmd = roux_core::RouxCommand::new("session-created").session_id(&session_id);
    if profile != "claude" {
        cmd = cmd.profile_id(&profile);
    }
    if let Err(e) = app.emit("roux-command", &cmd) {
        rlog!("Warning: failed to emit session-created event: {}", e);
    }

    Response::success(serde_json::json!({ "session_id": session_id }))
}

async fn handle_shell(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id,
        None => return Response::err("session_id required"),
    };

    let state: tauri::State<AppState> = app.state();
    let handle = state.session_handle.clone();

    let working_dir_arg =
        req.args.get("working_dir").and_then(|d| d.as_str()).map(|s| s.to_string());
    let session_record = match handle.get(session_id).await {
        Ok(s) => s,
        Err(e) => return Response::err(format!("{}", e)),
    };
    let working_dir = match (working_dir_arg, session_record.as_ref()) {
        (Some(dir), _) => dir,
        (None, Some(s)) => s.worktree_path.clone(),
        (None, None) => return Response::err("could not determine working directory"),
    };
    let project_id = session_record.as_ref().and_then(|s| s.project_id.clone());
    let worktree_env = session_record.as_ref().and_then(|s| {
        if s.is_worktree {
            Some(s.worktree_path.clone())
        } else {
            None
        }
    });

    let pane_id = crypto_random_uuid();
    let pty_id = crypto_random_uuid();

    if let Err(e) = state.pty_manager.spawn_shell(
        &pty_id,
        &working_dir,
        Some(session_id),
        Some(&pane_id),
        project_id.as_deref(),
        worktree_env.as_deref(),
        None, // notes env snapshot — wired only from session creation path
        None,
        None,
        crate::pty::PtyRole::Secondary,
        None, // profile — CLI-spawned, unknown
        app.clone(),
    ) {
        return Response::err(format!("Failed to spawn shell: {}", e));
    }

    use tauri::Emitter;
    let _ = app.emit(
        "roux-command",
        &roux_core::RouxCommand::new("shell-opened")
            .session_id(session_id)
            .pane_id(&pane_id)
            .pty_id(&pty_id),
    );

    Response::success(serde_json::json!({ "pane_id": pane_id, "pty_id": pty_id }))
}

fn handle_focus(req: Request, app: &tauri::AppHandle) -> Response {
    use tauri::Emitter;
    let mut cmd = roux_core::RouxCommand::new("focus");
    if let Some(ref id) = req.session_id {
        cmd = cmd.session_id(id);
    }
    if let Some(ref pane_id) = req.pane_id {
        cmd = cmd.pane_id(pane_id);
    }
    let _ = app.emit("roux-command", &cmd);

    Response::ok()
}

async fn handle_run(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id,
        None => return Response::err("session_id required"),
    };

    let command = match req.args.get("command").and_then(|c| c.as_str()) {
        Some(c) => c.to_string(),
        None => return Response::err("command argument required"),
    };

    let state: tauri::State<AppState> = app.state();
    let handle = state.session_handle.clone();

    let working_dir_arg =
        req.args.get("working_dir").and_then(|d| d.as_str()).map(|s| s.to_string());
    let session_record = match handle.get(session_id).await {
        Ok(s) => s,
        Err(e) => return Response::err(format!("{}", e)),
    };
    let working_dir = match (working_dir_arg, session_record.as_ref()) {
        (Some(dir), _) => dir,
        (None, Some(s)) => s.worktree_path.clone(),
        (None, None) => return Response::err("could not determine working directory"),
    };
    let project_id = session_record.as_ref().and_then(|s| s.project_id.clone());
    let worktree_env = session_record.as_ref().and_then(|s| {
        if s.is_worktree {
            Some(s.worktree_path.clone())
        } else {
            None
        }
    });

    let pane_id = format!("cmd-{}", crypto_random_uuid());
    let pty_id = format!(
        "{}-{}",
        pane_id,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
    );

    let pre_context = crate::automation_hooks::HookContext {
        repo_path: Some(working_dir.clone()),
        worktree_path: Some(working_dir.clone()),
        task_id: Some(pty_id.clone()),
        session_id: Some(session_id.to_string()),
        project_id: project_id.clone(),
        scope: Some("session".into()),
        cwd: Some(working_dir.clone()),
        ..crate::automation_hooks::HookContext::new(crate::automation_hooks::HookEvent::PreTaskRun)
    };
    if let Err(e) = state
        .automation_hooks
        .run_blocking(crate::automation_hooks::HookEvent::PreTaskRun, pre_context)
        .await
    {
        return Response::err(e.to_string());
    }

    if let Err(e) = state.pty_manager.spawn_task(
        &pty_id,
        &command,
        &working_dir,
        Some(session_id),
        Some(&pane_id),
        project_id.as_deref(),
        worktree_env.as_deref(),
        None, // notes env snapshot — wired only from session creation path
        None,
        crate::pty::PtyRole::Secondary,
        Some("task"),
        app.clone(),
    ) {
        return Response::err(format!("Failed to spawn task: {}", e));
    }
    let post_context = crate::automation_hooks::HookContext {
        repo_path: Some(working_dir.clone()),
        worktree_path: Some(working_dir.clone()),
        task_id: Some(pty_id.clone()),
        session_id: Some(session_id.to_string()),
        project_id,
        scope: Some("session".into()),
        cwd: Some(working_dir.clone()),
        ..crate::automation_hooks::HookContext::new(crate::automation_hooks::HookEvent::PostTaskRun)
    };
    state
        .automation_hooks
        .spawn_background(crate::automation_hooks::HookEvent::PostTaskRun, post_context);

    use tauri::Emitter;
    let _ = app.emit(
        "roux-command",
        &roux_core::RouxCommand::new("command-opened")
            .session_id(session_id)
            .pane_id(&pane_id)
            .pty_id(&pty_id)
            .command(&command)
            .working_dir(&working_dir),
    );

    Response::success(serde_json::json!({ "pane_id": pane_id, "pty_id": pty_id }))
}

/// Compose the bytes written to the PTY for a `send` request. Factored
/// out so the enter/no-enter contract can be unit-tested without a real
/// PtyManager.
fn format_send_data(text: &str, enter: bool) -> String {
    if enter {
        format!("{}\r", text)
    } else {
        text.to_string()
    }
}

/// Resolve which PTY to write to for `send`.
///
/// Frontend pane ids and pty ids live in different namespaces — the main
/// pane has pane_id `{session}-main` while its pty_id is `{session}` — so
/// when the caller passes a pane_id we have to look it up via the pane
/// service rather than treating it as a pty_id directly. When no pane_id
/// is given, fall back to the session's primary PTY.
async fn resolve_send_pty_id(
    pane_handle: &crate::pane_service::PaneHandle,
    session_handle: &crate::session_service::SessionHandle,
    session_id: &str,
    pane_id: Option<&str>,
) -> Result<String, String> {
    if let Some(pane_id) = pane_id {
        let records = pane_handle
            .list_by_ids(vec![pane_id.to_string()])
            .await
            .map_err(|e| format!("pane lookup failed: {}", e))?;
        return records
            .into_iter()
            .next()
            .map(|r| r.pty_id)
            .ok_or_else(|| format!("pane not found: {}", pane_id));
    }

    match session_handle.get(session_id).await {
        // Primary pane's pty_id is the session id by convention (see
        // services/sessions.rs::create_session_shell and reconnect_session_shell,
        // both of which spawn with `pty_id = session.id`). `primary_pty_id` on
        // the persisted record can lag — archive() clears it and reconnect
        // doesn't re-set it — so prefer the convention and fall back to the
        // recorded value only if it's set. This matches the pre-fix behavior
        // where the handler wrote to `session_id` directly.
        Ok(Some(session)) => Ok(session.primary_pty_id.unwrap_or_else(|| session_id.to_string())),
        Ok(None) => Err(format!("session not found: {}", session_id)),
        Err(e) => Err(format!("session lookup failed: {}", e)),
    }
}

/// Pure routing+formatting half of `handle_send`. Resolves the request to a
/// `(pty_id, bytes_to_write)` pair without touching the PtyManager, so it can
/// be exercised in headless tests without a real Tauri app or a live PTY.
async fn prepare_send(
    pane_handle: &crate::pane_service::PaneHandle,
    session_handle: &crate::session_service::SessionHandle,
    req: &Request,
) -> Result<(String, Vec<u8>), String> {
    let text = req
        .args
        .get("text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "text argument required".to_string())?
        .to_string();

    let session_id =
        req.session_id.as_deref().ok_or_else(|| "session_id required".to_string())?;

    let pty_id =
        resolve_send_pty_id(pane_handle, session_handle, session_id, req.pane_id.as_deref())
            .await?;

    let cr = req.args.get("enter").and_then(|v| v.as_bool()).unwrap_or(true);
    let data = format_send_data(&text, cr).into_bytes();
    Ok((pty_id, data))
}

async fn handle_send(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let (pty_id, bytes) =
        match prepare_send(&state.pane_handle, &state.session_handle, &req).await {
            Ok(pair) => pair,
            Err(e) => return Response::err(e),
        };
    if let Err(e) = state.pty_manager.write(&pty_id, &bytes) {
        return Response::err(format!("Failed to write to session: {}", e));
    }
    Response::ok()
}

async fn handle_notify(req: Request, app: &tauri::AppHandle) -> Response {
    use roux_core::{
        ActionKind, NotificationAction, NotificationLevel, NotificationRequest as NReq,
        NotificationSource,
    };

    let payload = match req.args.get("payload") {
        Some(p) => p.clone(),
        None => return Response::err("payload required"),
    };

    // Required
    let title = match payload.get("title").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Response::err("payload.title required"),
    };

    // Severity — default to info.
    let level = match payload.get("level").and_then(|l| l.as_str()).unwrap_or("info") {
        "info" => NotificationLevel::Info,
        "success" => NotificationLevel::Success,
        "attention" => NotificationLevel::Attention,
        "warning" => NotificationLevel::Warning,
        "error" => NotificationLevel::Error,
        other => {
            return Response::err(format!(
                "invalid level: {}, expected info|success|attention|warning|error",
                other
            ));
        }
    };

    let subtitle = payload.get("subtitle").and_then(|s| s.as_str()).map(String::from);
    let body = payload.get("body").and_then(|s| s.as_str()).map(String::from);

    // Session resolution:
    //   1. explicit sessionId in payload
    //   2. --cwd lookup against session list
    //   3. None (global)
    let state: tauri::State<AppState> = app.state();
    let session_id = if let Some(sid) = payload.get("sessionId").and_then(|s| s.as_str()) {
        Some(sid.to_string())
    } else if let Some(cwd) = req.args.get("cwd").and_then(|c| c.as_str()) {
        match state.session_handle.list().await {
            Ok(sessions) => sessions
                .into_iter()
                .find(|s| s.worktree_path == cwd || s.repo_root == cwd)
                .map(|s| s.id),
            Err(_) => None,
        }
    } else {
        None
    };

    // Default actions: Focus (if session resolved) + Dismiss.
    let mut actions: Vec<NotificationAction> = Vec::new();
    if let Some(ref sid) = session_id {
        actions.push(NotificationAction {
            id: "focus".into(),
            label: "Focus session".into(),
            kind: ActionKind::FocusSession { session_id: sid.clone() },
            primary: true,
        });
    }
    actions.push(NotificationAction {
        id: "dismiss".into(),
        label: "Dismiss".into(),
        kind: ActionKind::Dismiss,
        primary: actions.is_empty(),
    });

    let notification = state.notification_manager.push(
        NReq {
            level,
            source: NotificationSource::Cli,
            title,
            subtitle,
            body,
            session_id,
            actions,
            dedup_key: None,
        },
        Some(app),
    );

    Response::success(serde_json::json!({ "id": notification.id }))
}

fn crypto_random_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Clean up the socket file on shutdown.
pub fn cleanup_socket() {
    let path = socket_path();
    let _ = fs::remove_file(path);
    #[cfg(windows)]
    {
        let _ = fs::remove_file(platform::socket_addr_file_path());
        let _ = fs::remove_file(platform::socket_auth_token_file_path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_under_config() {
        let path = socket_path();
        assert_eq!(path, platform::socket_path());
    }

    #[test]
    fn response_ok_serializes() {
        let resp = Response::ok();
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
        assert!(json.get("data").is_none());
        assert!(json.get("error").is_none());
    }

    #[test]
    fn response_success_serializes() {
        let resp = Response::success(serde_json::json!({"pane_id": "abc"}));
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["data"]["pane_id"], "abc");
        assert!(json.get("error").is_none());
    }

    #[test]
    fn response_err_serializes() {
        let resp = Response::err("something broke");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"], "something broke");
        assert!(json.get("data").is_none());
    }

    #[test]
    fn request_deserializes_minimal() {
        let json = r#"{"command": "split"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "split");
        assert!(req.session_id.is_none());
        assert!(req.pane_id.is_none());
        assert!(req.auth_token.is_none());
    }

    #[test]
    fn request_deserializes_full() {
        let json = r#"{
            "command": "split",
            "session_id": "abc123",
            "pane_id": "pane-1",
            "args": {"direction": "horizontal"}
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "split");
        assert_eq!(req.session_id.as_deref(), Some("abc123"));
        assert_eq!(req.pane_id.as_deref(), Some("pane-1"));
        assert!(req.auth_token.is_none());
        assert_eq!(req.args["direction"], "horizontal");
    }

    #[test]
    fn request_default_args_is_null() {
        let json = r#"{"command": "status"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(req.args.is_null());
    }

    // ── format_send_data ─────────────────────────────────────

    #[test]
    fn format_send_data_appends_cr_when_enter_true() {
        assert_eq!(format_send_data("hi", true), "hi\r");
    }

    #[test]
    fn format_send_data_returns_raw_when_enter_false() {
        assert_eq!(format_send_data("hi", false), "hi");
    }

    #[test]
    fn format_send_data_preserves_embedded_newlines() {
        // Embedded newlines must survive verbatim; the flag only controls
        // the *trailing* Enter.
        assert_eq!(format_send_data("a\nb", true), "a\nb\r");
        assert_eq!(format_send_data("a\nb", false), "a\nb");
    }

    #[test]
    fn format_send_data_empty_text_still_appends_cr() {
        assert_eq!(format_send_data("", true), "\r");
        assert_eq!(format_send_data("", false), "");
    }

    // ── Request deserialization for new commands ─────────────

    #[test]
    fn request_send_with_enter_false_deserializes() {
        let json = r#"{
            "command": "send",
            "session_id": "s1",
            "args": {"text": "hi", "enter": false}
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "send");
        assert_eq!(req.args["text"], "hi");
        assert_eq!(req.args["enter"], false);
    }

    #[test]
    fn request_send_without_enter_defaults_to_true_at_handler() {
        // Request itself doesn't default enter; the handler does. Document
        // that absence of the key is equivalent to true via the read pattern.
        let json = r#"{"command": "send", "session_id": "s1", "args": {"text": "x"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        let cr = req.args.get("enter").and_then(|v| v.as_bool()).unwrap_or(true);
        assert!(cr);
    }

    #[test]
    fn request_app_open_deserializes() {
        let json = r#"{"command": "app-open", "args": {"path": "/tmp/x"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "app-open");
        assert_eq!(req.args["path"], "/tmp/x");
    }

    #[test]
    fn request_session_list_deserializes_without_args() {
        let json = r#"{"command": "session-list"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "session-list");
        assert!(req.session_id.is_none());
        assert!(req.args.is_null());
    }

    #[test]
    fn request_session_poll_requires_session_id() {
        let json = r#"{"command": "session-poll", "session_id": "sid-abc"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "session-poll");
        assert_eq!(req.session_id.as_deref(), Some("sid-abc"));
    }

    #[test]
    fn request_session_panes_list_deserializes() {
        let json = r#"{"command": "session-panes-list", "session_id": "sid-1"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "session-panes-list");
        assert_eq!(req.session_id.as_deref(), Some("sid-1"));
    }

    #[test]
    fn request_session_panes_create_with_profile_deserializes() {
        let json = r#"{
            "command": "session-panes-create",
            "session_id": "sid-1",
            "args": {"profile": "shell", "direction": "vertical", "working_dir": "/tmp"}
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.args["profile"], "shell");
        assert_eq!(req.args["direction"], "vertical");
        assert_eq!(req.args["working_dir"], "/tmp");
    }

    #[test]
    fn request_session_create_with_full_args_deserializes() {
        let json = r#"{
            "command": "session-create",
            "args": {
                "name": "feat-x",
                "worktree_branch": "feat/x",
                "profile": "claude",
                "flags": ["--debug", "--model=opus"],
                "nono_profile": "strict",
                "nono_allow_dirs": ["~/work", "/tmp"]
            }
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.args["name"], "feat-x");
        assert_eq!(req.args["worktree_branch"], "feat/x");
        assert_eq!(req.args["profile"], "claude");
        assert_eq!(req.args["flags"].as_array().unwrap().len(), 2);
        assert_eq!(req.args["nono_allow_dirs"].as_array().unwrap().len(), 2);
    }

    // ── resolve_send_pty_id ──────────────────────────────────

    fn pane_record(id: &str, pty_id: &str) -> crate::pane_service::PaneRecord {
        crate::pane_service::PaneRecord {
            id: id.into(),
            pane_type: "claude".into(),
            pty_id: pty_id.into(),
            name: None,
            working_dir: None,
            command: None,
            doc_path: None,
            spawn_profile_ref: None,
            provider: None,
            provider_session_id: None,
            nono_profile: None,
            nono_allow_dirs: None,
            notes_scope: None,
            notes_view_mode: None,
        }
    }

    fn session_with_pty(id: &str, primary_pty_id: Option<&str>) -> crate::session::Session {
        crate::session::Session {
            id: id.into(),
            name: format!("Session {}", id),
            repo_root: "/tmp/repo".into(),
            worktree_path: "/tmp/repo".into(),
            branch: "main".into(),
            is_worktree: false,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: None,
            is_git_repo: false,
            name_override: None,
            primary_pty_id: primary_pty_id.map(String::from),
            archived: false,
            ended_at: None,
            blueprint_id: None,
        }
    }

    #[tokio::test]
    async fn resolve_send_pty_id_pane_lookup_returns_pty_id() {
        // The main pane's frontend id is `{session}-main` while its pty_id
        // is the session id itself. The handler must translate, not pass the
        // pane id straight to pty_manager.
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));
        panes.upsert(pane_record("sid-1-main", "sid-1")).await.unwrap();

        let pty_id = resolve_send_pty_id(&panes, &sessions, "sid-1", Some("sid-1-main"))
            .await
            .unwrap();
        assert_eq!(pty_id, "sid-1");
    }

    #[tokio::test]
    async fn resolve_send_pty_id_no_pane_uses_session_primary_pty() {
        // The common path: caller passes only --session. The session's primary
        // PTY is the canonical write target.
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = crate::session_service::spawn_with_path(
            vec![session_with_pty("sid-1", Some("sid-1"))],
            dir.path().join("sessions.json"),
        );

        let pty_id = resolve_send_pty_id(&panes, &sessions, "sid-1", None).await.unwrap();
        assert_eq!(pty_id, "sid-1");
    }

    #[tokio::test]
    async fn resolve_send_pty_id_unknown_pane_errors() {
        // Issue #127's failure mode pre-CLI-fix: a pane id from a different
        // session leaks through. With the backend fix, unknown panes now
        // surface a clean "pane not found" rather than misrouting to a
        // nonexistent pty.
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = crate::session_service::spawn_with_path(
            vec![session_with_pty("sid-2", Some("sid-2"))],
            dir.path().join("sessions.json"),
        );

        let err = resolve_send_pty_id(&panes, &sessions, "sid-2", Some("sid-1-main"))
            .await
            .unwrap_err();
        assert!(err.contains("pane not found"), "got: {}", err);
    }

    #[tokio::test]
    async fn resolve_send_pty_id_unknown_session_errors() {
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));

        let err = resolve_send_pty_id(&panes, &sessions, "missing-sid", None).await.unwrap_err();
        assert!(err.contains("session not found"), "got: {}", err);
    }

    #[tokio::test]
    async fn resolve_send_pty_id_falls_back_to_session_id_when_primary_pty_unset() {
        // Regression coverage for the restore/reconnect path: archive() clears
        // primary_pty_id and reconnect_session_shell doesn't re-set it, but
        // the live PTY is still registered under `pty_id == session.id` by
        // convention. The resolver must use that convention when the
        // persisted field is None, otherwise sends to restored sessions break.
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = crate::session_service::spawn_with_path(
            vec![session_with_pty("sid-1", None)],
            dir.path().join("sessions.json"),
        );

        let pty_id = resolve_send_pty_id(&panes, &sessions, "sid-1", None).await.unwrap();
        assert_eq!(pty_id, "sid-1");
    }

    // ── prepare_send (issue #127 regression coverage) ────────

    fn send_request(session_id: &str, pane_id: Option<&str>, text: &str, enter: bool) -> Request {
        let mut args = serde_json::Map::new();
        args.insert("text".into(), serde_json::Value::String(text.into()));
        args.insert("enter".into(), serde_json::Value::Bool(enter));
        Request {
            command: "send".into(),
            session_id: Some(session_id.into()),
            pane_id: pane_id.map(String::from),
            auth_token: None,
            args: serde_json::Value::Object(args),
        }
    }

    /// End-to-end exercise of the routing+formatting path on the bug-report
    /// scenarios. These are the regression tests that would fail if either
    /// half of the fix (CLI env handling, backend pane→pty translation) gets
    /// reverted, without needing a running Tauri app or a live PTY.
    #[tokio::test]
    async fn prepare_send_main_pane_id_resolves_to_session_pty() {
        // The main pane has frontend id `{session}-main` but its pty_id is
        // `{session}`. Pre-fix, handle_send wrote to `{session}-main` and
        // failed; now prepare_send must hand back `{session}` plus the
        // formatted bytes.
        let (panes, _pj) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sj) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));
        panes.upsert(pane_record("sid-1-main", "sid-1")).await.unwrap();

        let req = send_request("sid-1", Some("sid-1-main"), "hello", true);
        let (pty_id, bytes) = prepare_send(&panes, &sessions, &req).await.unwrap();
        assert_eq!(pty_id, "sid-1");
        assert_eq!(bytes, b"hello\r");
    }

    #[tokio::test]
    async fn prepare_send_no_pane_uses_session_primary_pty() {
        // The common in-session case: caller has no --pane.
        let (panes, _pj) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sj) = crate::session_service::spawn_with_path(
            vec![session_with_pty("sid-1", Some("sid-1"))],
            dir.path().join("sessions.json"),
        );

        let req = send_request("sid-1", None, "hi", true);
        let (pty_id, bytes) = prepare_send(&panes, &sessions, &req).await.unwrap();
        assert_eq!(pty_id, "sid-1");
        assert_eq!(bytes, b"hi\r");
    }

    #[tokio::test]
    async fn prepare_send_no_enter_omits_carriage_return() {
        let (panes, _pj) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sj) = crate::session_service::spawn_with_path(
            vec![session_with_pty("sid-1", Some("sid-1"))],
            dir.path().join("sessions.json"),
        );

        let req = send_request("sid-1", None, "raw", false);
        let (_pty_id, bytes) = prepare_send(&panes, &sessions, &req).await.unwrap();
        assert_eq!(bytes, b"raw");
    }

    #[tokio::test]
    async fn prepare_send_cross_session_pane_lookup_returns_target_pty() {
        // The exact issue #127 path AFTER the CLI fix: caller in session A
        // sends to session B with --session B. Both sessions exist; B's main
        // pane is registered. The result must route to B's pty_id, not A's.
        let (panes, _pj) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sj) = crate::session_service::spawn_with_path(
            vec![
                session_with_pty("sid-A", Some("sid-A")),
                session_with_pty("sid-B", Some("sid-B")),
            ],
            dir.path().join("sessions.json"),
        );
        panes.upsert(pane_record("sid-A-main", "sid-A")).await.unwrap();
        panes.upsert(pane_record("sid-B-main", "sid-B")).await.unwrap();

        // Caller targets B explicitly. With the CLI fix in place the env
        // pane is dropped, so pane_id is None — backend falls back to B's
        // primary_pty_id.
        let req = send_request("sid-B", None, "for B", true);
        let (pty_id, _) = prepare_send(&panes, &sessions, &req).await.unwrap();
        assert_eq!(pty_id, "sid-B");
    }

    #[tokio::test]
    async fn prepare_send_stale_cross_session_pane_id_errors_cleanly() {
        // The pre-CLI-fix failure mode: the caller's env pane (`sid-A-main`)
        // leaks into a request targeting B. Backend must surface a clean
        // "pane not found" rather than silently writing to A's pty (which
        // is what the OLD bug did via the pane_id == pty_id assumption).
        let (panes, _pj) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sj) = crate::session_service::spawn_with_path(
            vec![session_with_pty("sid-B", Some("sid-B"))],
            dir.path().join("sessions.json"),
        );
        // Only B's pane is registered. A doesn't exist on this Roux instance.
        panes.upsert(pane_record("sid-B-main", "sid-B")).await.unwrap();

        let req = send_request("sid-B", Some("sid-A-main"), "leaked", true);
        let err = prepare_send(&panes, &sessions, &req).await.unwrap_err();
        assert!(err.contains("pane not found"), "got: {}", err);
    }

    #[tokio::test]
    async fn prepare_send_missing_text_errors() {
        let (panes, _pj) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sj) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));
        let req = Request {
            command: "send".into(),
            session_id: Some("sid".into()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        };
        let err = prepare_send(&panes, &sessions, &req).await.unwrap_err();
        assert_eq!(err, "text argument required");
    }

    #[tokio::test]
    async fn prepare_send_missing_session_id_errors() {
        let (panes, _pj) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sj) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));
        let req = Request {
            command: "send".into(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({"text": "hi"}),
        };
        let err = prepare_send(&panes, &sessions, &req).await.unwrap_err();
        assert_eq!(err, "session_id required");
    }

    // ── Pending-reply round-trip primitives ──────────────────

    // ── Frontend error replies propagate as errors, not successes ───

    #[tokio::test]
    async fn pending_reply_with_error_field_surfaces_as_err() {
        // The frontend conventionally replies with `{ error: "..." }` on
        // failure. The CLI caller must see exit 1, not exit 0 + an error blob.
        let map: crate::state::PendingReplies =
            std::sync::Mutex::new(std::collections::HashMap::new());
        let (rid, rx) = register_pending_reply_in(&map);
        let tx = map.lock().unwrap().remove(&rid).unwrap();
        tx.send(serde_json::json!({"error": "pane-create failed"})).unwrap();

        let resp = await_frontend_reply_in(&map, rid, rx, 500).await;
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"], "pane-create failed");
    }

    #[tokio::test]
    async fn pending_reply_without_error_field_is_success() {
        // A reply that happens to contain other fields but no `error` key
        // remains a success — e.g. a pane snapshot with an `errors` array.
        let map: crate::state::PendingReplies =
            std::sync::Mutex::new(std::collections::HashMap::new());
        let (rid, rx) = register_pending_reply_in(&map);
        let tx = map.lock().unwrap().remove(&rid).unwrap();
        tx.send(serde_json::json!({"descriptors": []})).unwrap();

        let resp = await_frontend_reply_in(&map, rid, rx, 500).await;
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
    }

    // ── Profile id validation ────────────────────────────────

    #[test]
    fn validate_profile_id_accepts_claude_builtin() {
        let settings = crate::settings::RouxSettings::default();
        assert!(validate_profile_id("claude", &settings).is_ok());
    }

    #[test]
    fn validate_profile_id_accepts_plain_shell_builtin() {
        let settings = crate::settings::RouxSettings::default();
        assert!(validate_profile_id("plain-shell", &settings).is_ok());
    }

    #[test]
    fn validate_profile_id_rejects_unknown_id_with_helpful_message() {
        let settings = crate::settings::RouxSettings::default();
        let err = validate_profile_id("shell", &settings).unwrap_err();
        // Common typo: "shell" instead of "plain-shell". The message should
        // point callers at the known builtins.
        assert!(err.contains("shell"));
        assert!(err.contains("plain-shell"), "error must list builtins: {}", err);
    }

    #[test]
    fn validate_profile_id_accepts_user_profile() {
        let mut settings = crate::settings::RouxSettings::default();
        settings.spawn_profiles.push(roux_core::SpawnProfile {
            id: "my-profile".into(),
            name: "Mine".into(),
            setup_command: None,
            startup_command: None,
            startup_behavior: None,
            env: None,
            cwd_override: None,
            icon: None,
            provider: None,
            nono_profile: None,
            nono_allow_dirs: None,
            source: roux_core::ProfileSource::User,
        });
        assert!(validate_profile_id("my-profile", &settings).is_ok());
    }

    #[test]
    fn validate_profile_id_error_lists_user_profiles_when_present() {
        let mut settings = crate::settings::RouxSettings::default();
        settings.spawn_profiles.push(roux_core::SpawnProfile {
            id: "my-profile".into(),
            name: "Mine".into(),
            setup_command: None,
            startup_command: None,
            startup_behavior: None,
            env: None,
            cwd_override: None,
            icon: None,
            provider: None,
            nono_profile: None,
            nono_allow_dirs: None,
            source: roux_core::ProfileSource::User,
        });
        let err = validate_profile_id("typo", &settings).unwrap_err();
        assert!(err.contains("User profiles"), "err should mention user profiles: {}", err);
        assert!(err.contains("my-profile"), "err should list user profile id: {}", err);
    }

    #[test]
    fn validate_profile_id_error_omits_user_section_when_no_user_profiles() {
        let settings = crate::settings::RouxSettings::default();
        let err = validate_profile_id("typo", &settings).unwrap_err();
        assert!(
            !err.contains("User profiles"),
            "err should not mention user profiles when empty: {}",
            err
        );
    }

    // ── Canonicalized path matching ──────────────────────────

    #[test]
    fn canonicalize_or_passthrough_returns_input_for_missing_path() {
        // Non-existent path — cannot canonicalize, so the input is returned verbatim.
        let got = canonicalize_or_passthrough("/nonexistent/roux-test-path");
        assert_eq!(got, "/nonexistent/roux-test-path");
    }

    #[test]
    fn canonicalize_or_passthrough_resolves_dot() {
        let got = canonicalize_or_passthrough("/tmp/./");
        // Whether it exists depends on platform, but on all supported ones /tmp resolves.
        // Accept either /tmp or /private/tmp (macOS symlink) or the passthrough input.
        assert!(got == "/tmp" || got == "/private/tmp" || got.contains("tmp"));
    }

    #[tokio::test]
    async fn pending_reply_resolves_when_frontend_replies() {
        let map: crate::state::PendingReplies =
            std::sync::Mutex::new(std::collections::HashMap::new());
        let (rid, rx) = register_pending_reply_in(&map);

        // Pull the sender back out and deliver a reply, as `submit_roux_reply`
        // would do from the frontend side.
        let tx = map.lock().unwrap().remove(&rid).unwrap();
        let payload = serde_json::json!({"pane_id": "p", "pty_id": "t"});
        tx.send(payload.clone()).unwrap();

        let resp = await_frontend_reply_in(&map, rid, rx, 500).await;
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["data"]["pane_id"], "p");
        assert_eq!(json["data"]["pty_id"], "t");
    }

    #[tokio::test]
    async fn pending_reply_times_out_and_cleans_up_map() {
        let map: crate::state::PendingReplies =
            std::sync::Mutex::new(std::collections::HashMap::new());
        let (rid, rx) = register_pending_reply_in(&map);
        assert_eq!(map.lock().unwrap().len(), 1);

        let resp = await_frontend_reply_in(&map, rid.clone(), rx, 50).await;
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], false);
        assert!(json["error"].as_str().unwrap().contains("timed out"));

        // Timed-out entry must not leak.
        assert!(map.lock().unwrap().is_empty(), "timeout must drop the entry");
    }

    #[tokio::test]
    async fn pending_reply_reports_dropped_channel() {
        let map: crate::state::PendingReplies =
            std::sync::Mutex::new(std::collections::HashMap::new());
        let (rid, rx) = register_pending_reply_in(&map);

        // Drop the sender without sending — simulates a crashed frontend
        // handler that never called submit_roux_reply.
        let _ = map.lock().unwrap().remove(&rid);
        // tx is dropped here when the HashMap entry goes out of scope.

        let resp = await_frontend_reply_in(&map, rid, rx, 500).await;
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], false);
        assert!(json["error"].as_str().unwrap().contains("dropped"));
    }

    #[test]
    fn register_pending_reply_in_generates_unique_ids() {
        let map: crate::state::PendingReplies =
            std::sync::Mutex::new(std::collections::HashMap::new());
        let (r1, _rx1) = register_pending_reply_in(&map);
        let (r2, _rx2) = register_pending_reply_in(&map);
        assert_ne!(r1, r2);
        assert_eq!(map.lock().unwrap().len(), 2);
    }

    // ── app-open session matching ────────────────────────────

    fn make_session(id: &str, repo: &str, worktree: &str) -> crate::session::Session {
        crate::session::Session {
            id: id.to_string(),
            name: id.to_string(),
            repo_root: repo.to_string(),
            worktree_path: worktree.to_string(),
            branch: "main".to_string(),
            is_worktree: repo != worktree,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: None,
            is_git_repo: true,
            name_override: None,
            primary_pty_id: None,
            archived: false,
            ended_at: None,
            blueprint_id: None,
        }
    }

    #[test]
    fn find_session_matches_on_worktree_path() {
        let sessions =
            vec![make_session("a", "/repo", "/repo"), make_session("b", "/repo", "/wt/feat")];
        let got = find_session_for_path(&sessions, "/wt/feat").unwrap();
        assert_eq!(got.id, "b");
    }

    #[test]
    fn find_session_matches_on_repo_root_when_no_worktree_match() {
        let sessions = vec![make_session("a", "/repo", "/repo")];
        let got = find_session_for_path(&sessions, "/repo").unwrap();
        assert_eq!(got.id, "a");
    }

    #[test]
    fn find_session_returns_none_when_no_match() {
        let sessions = vec![make_session("a", "/repo", "/wt/feat")];
        assert!(find_session_for_path(&sessions, "/other").is_none());
    }

    #[test]
    fn find_session_returns_first_on_multiple_matches() {
        let sessions =
            vec![make_session("a", "/repo", "/repo"), make_session("b", "/repo", "/repo")];
        let got = find_session_for_path(&sessions, "/repo").unwrap();
        assert_eq!(got.id, "a");
    }

    #[test]
    fn default_session_name_uses_basename() {
        assert_eq!(default_session_name_for_path("/tmp/my-repo"), "my-repo");
    }

    #[test]
    fn default_session_name_handles_trailing_slash() {
        assert_eq!(default_session_name_for_path("/tmp/my-repo/"), "my-repo");
    }

    #[test]
    fn default_session_name_fallback_for_root() {
        // "/" has no file_name on Unix.
        let got = default_session_name_for_path("/");
        assert!(got == "New Session" || got == "/");
    }

    // ── RouxCommand builder / serde ──────────────────────────

    #[test]
    fn roux_command_profile_id_and_request_id_builders() {
        let cmd = roux_core::RouxCommand::new("pane-create")
            .session_id("s")
            .profile_id("claude")
            .request_id("req-1")
            .direction("horizontal");
        assert_eq!(cmd.profile_id.as_deref(), Some("claude"));
        assert_eq!(cmd.request_id.as_deref(), Some("req-1"));
        assert_eq!(cmd.direction.as_deref(), Some("horizontal"));
    }

    #[test]
    fn roux_command_serializes_camel_case_and_skips_none() {
        let cmd = roux_core::RouxCommand::new("focus").session_id("sid-1");
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["action"], "focus");
        assert_eq!(json["sessionId"], "sid-1");
        assert!(json.get("profileId").is_none());
        assert!(json.get("requestId").is_none());
        assert!(json.get("paneId").is_none());
    }

    #[test]
    fn roux_command_serializes_profile_and_request_id_as_camel_case() {
        let cmd = roux_core::RouxCommand::new("pane-create").profile_id("shell").request_id("r1");
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["profileId"], "shell");
        assert_eq!(json["requestId"], "r1");
    }

    #[test]
    fn drop_pending_reply_in_removes_entry() {
        let map: crate::state::PendingReplies =
            std::sync::Mutex::new(std::collections::HashMap::new());
        let (rid, _rx) = register_pending_reply_in(&map);
        assert_eq!(map.lock().unwrap().len(), 1);
        drop_pending_reply_in(&map, &rid);
        assert!(map.lock().unwrap().is_empty());
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn socket_roundtrip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::{UnixListener, UnixStream};

        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();

        // Spawn a simple echo server that reads a line and responds with a fixed response
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut buf_reader = BufReader::new(reader);
            let mut line = String::new();
            buf_reader.read_line(&mut line).await.unwrap();

            // Parse and verify it's a valid request
            let req: Request = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(req.command, "split");

            let resp = Response::ok();
            let json = serde_json::to_string(&resp).unwrap();
            writer.write_all(json.as_bytes()).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
            writer.shutdown().await.unwrap();
        });

        // Client side
        let mut stream = UnixStream::connect(&sock_path).await.unwrap();
        let request = serde_json::json!({
            "command": "split",
            "session_id": "test-session",
            "args": {"direction": "horizontal"}
        });
        let mut msg = serde_json::to_string(&request).unwrap();
        msg.push('\n');
        stream.write_all(msg.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();

        let (reader, _) = stream.into_split();
        let mut buf_reader = BufReader::new(reader);
        let mut response_line = String::new();
        buf_reader.read_line(&mut response_line).await.unwrap();

        let resp: serde_json::Value = serde_json::from_str(response_line.trim()).unwrap();
        assert_eq!(resp["ok"], true);

        server.await.unwrap();
    }
}
