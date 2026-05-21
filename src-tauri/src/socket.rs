use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
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

const DEFAULT_LATEST_OUTPUT_BYTES: usize = 8 * 1024;
const MAX_LATEST_OUTPUT_BYTES: usize = 64 * 1024;

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

                let req = match serde_json::from_str::<Request>(line.trim()) {
                    Ok(req) => req,
                    Err(e) => {
                        let resp = Response::err(format!("Invalid request: {}", e));
                        let json = serde_json::to_string(&resp).unwrap_or_default();
                        let _ = writer.write_all(json.as_bytes()).await;
                        let _ = writer.write_all(b"\n").await;
                        let _ = writer.shutdown().await;
                        return;
                    }
                };

                // Streaming commands keep the writer open and push
                // newline-delimited JSON until the client disconnects.
                // One-shot commands (the default) write a single
                // Response and shut the writer down.
                if is_streaming_command(&req.command) {
                    handle_streaming_request(req, &app, buf_reader, writer).await;
                    return;
                }

                let response = handle_request(req, &app).await;
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writer.write_all(json.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
                let _ = writer.shutdown().await;
            });
        }
    });
}

/// True for commands that switch to a streaming protocol (multiple
/// newline-delimited JSON objects) instead of the default
/// request/response. Must stay in sync with `handle_streaming_request`.
fn is_streaming_command(cmd: &str) -> bool {
    matches!(cmd, "mailbox-watch")
}

/// Dispatch streaming commands. Owns the writer for the lifetime of the
/// connection; the handler returns when the client disconnects, an
/// error happens, or the watch is cancelled. Generic over reader/writer
/// types so the same code path works for Unix sockets and Windows TCP.
async fn handle_streaming_request<R, W>(
    req: Request,
    app: &tauri::AppHandle,
    reader: BufReader<R>,
    writer: W,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    #[cfg(windows)]
    {
        let Some(expected_token) = platform::load_socket_auth_token() else {
            stream_error(writer, "Socket auth token unavailable").await;
            return;
        };
        if req.auth_token.as_deref() != Some(expected_token.as_str()) {
            stream_error(writer, "unauthorized").await;
            return;
        }
    }
    match req.command.as_str() {
        "mailbox-watch" => handle_mailbox_watch(req, app, reader, writer).await,
        // Should never be reached: `is_streaming_command` is the source
        // of truth and only routes known streaming commands here.
        other => stream_error(writer, format!("unknown streaming command: {other}")).await,
    }
}

/// Write a single error frame and close. Used for auth/dispatch failures
/// before the streaming loop starts.
async fn stream_error<W>(mut writer: W, msg: impl Into<String>)
where
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    let resp = Response::err(msg);
    let json = serde_json::to_string(&resp).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
    let _ = writer.shutdown().await;
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
        "latest-output" => handle_latest_output(req, app).await,
        "notify" => handle_notify(req, app).await,
        "notes-read" => handle_notes_read(req, app).await,
        "notes-write" => handle_notes_write(req, app).await,
        "notes-append" => handle_notes_append(req, app).await,
        "notes-path" => handle_notes_path(req, app).await,
        "notes-search" => handle_notes_search(req, app).await,
        "notes-vault-root" => handle_notes_vault_root(app),
        "hook-show" => handle_hook_show(req, app).await,
        "hook-run" => handle_hook_run(req, app).await,
        "alias-set" => handle_alias_set(req, app).await,
        "alias-unset" => handle_alias_unset(req, app).await,
        "alias-claim" => handle_alias_claim(req, app).await,
        "alias-list" => handle_alias_list(req, app).await,
        "alias-get" => handle_alias_get(req, app).await,
        "alias-whoami" => handle_alias_whoami(req, app).await,
        "alias-add-member" => handle_alias_add_member(req, app).await,
        "alias-remove-member" => handle_alias_remove_member(req, app).await,
        "alias-mode" => handle_alias_mode(req, app).await,
        "mailbox-post" => handle_mailbox_post(req, app).await,
        "mailbox-peek" => handle_mailbox_peek(req, app).await,
        "mailbox-read" => handle_mailbox_read(req, app).await,
        "mailbox-ack" => handle_mailbox_ack(req, app).await,
        "mailbox-retract" => handle_mailbox_retract(req, app).await,
        "mailbox-dismiss" => handle_mailbox_dismiss(req, app).await,
        "mailbox-count" => handle_mailbox_count(req, app).await,
        "mailbox-clear" => handle_mailbox_clear(req, app).await,
        "mailbox-reply" => handle_mailbox_reply(req, app).await,
        "mailbox-sent" => handle_mailbox_sent(req, app).await,
        "bus-publish" => handle_bus_publish(req, app).await,
        "bus-tail" => handle_bus_tail(req, app).await,
        "bus-subscribe" => handle_bus_subscribe(req, app).await,
        "bus-unsubscribe" => handle_bus_unsubscribe(req, app).await,
        "bus-subscriptions" => handle_bus_subscriptions(req, app).await,
        "session-list" => handle_session_list(req, app).await,
        "session-poll" => handle_session_poll(req, app).await,
        "session-kill" => handle_session_kill(req, app).await,
        "session-rename" => handle_session_rename(req, app).await,
        "session-panes-list" => handle_session_panes_list(req, app).await,
        "session-panes-create" => handle_session_panes_create(req, app).await,
        "mcp-enabled" => handle_mcp_enabled(app),
        "app-open" => handle_app_open(req, app).await,
        _ => Response::err(format!("unknown command: {}", req.command)),
    }
}

fn handle_mcp_enabled(app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let enabled = state.settings.lock().map(|settings| settings.mcp_enabled).unwrap_or(false);
    Response::success(serde_json::json!({ "enabled": enabled }))
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

async fn handle_session_rename(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id.to_string(),
        None => return Response::err("session_id required (set $ROUX_SESSION_ID or pass --session)"),
    };
    let raw = match req.args.get("name").and_then(|n| n.as_str()) {
        Some(n) => n.to_string(),
        None => return Response::err("name required"),
    };
    // Empty / whitespace-only name clears the override (matches the
    // GUI's clearSessionNameOverride path).
    let name_override =
        if raw.trim().is_empty() { None } else { Some(raw.trim().to_string()) };

    let state: tauri::State<AppState> = app.state();
    if let Err(e) = state.session_handle.set_name_override(&session_id, name_override.clone()).await
    {
        return Response::err(format!("{}", e));
    }

    use tauri::Emitter;
    let cmd = roux_core::RouxCommand::new("session-renamed").session_id(&session_id);
    if let Err(e) = app.emit("roux-command", &cmd) {
        rlog!("Warning: failed to emit session-renamed event: {}", e);
    }

    Response::success(serde_json::json!({
        "session_id": session_id,
        "name_override": name_override,
    }))
}

async fn handle_session_kill(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id.to_string(),
        None => {
            return Response::err(
                "session_id required (set $ROUX_SESSION_ID or pass --session)",
            )
        }
    };
    let state: tauri::State<AppState> = app.state();
    if let Err(e) =
        crate::commands::sessions::archive_session_with_hooks(&state, &session_id).await
    {
        return Response::err(e);
    }
    use tauri::Emitter;
    let cmd = roux_core::RouxCommand::new("session-killed").session_id(&session_id);
    if let Err(e) = app.emit("roux-command", &cmd) {
        rlog!("Warning: failed to emit session-killed event: {}", e);
    }
    Response::success(serde_json::json!({ "session_id": session_id }))
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
        None, // smol_machine_name - CLI sessions don't bind at create time
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
    let start_point =
        req.args.get("start_point").and_then(|d| d.as_str()).map(|s| s.to_string());
    let prompt = req.args.get("prompt").and_then(|d| d.as_str()).map(|s| s.to_string());
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

    // Resolve repo_path: explicit working_dir wins so a caller inside another
    // session can target a different repo. Fall back to the caller session's
    // repo_root only when working_dir is not provided.
    let repo_path = match (working_dir.clone(), req.session_id.as_deref()) {
        (Some(d), _) => d,
        (None, Some(id)) => match handle.get(id).await {
            Ok(Some(session)) => session.repo_root.clone(),
            _ => String::new(),
        },
        (None, None) => String::new(),
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
    // For new worktrees, fetch first when the start_point references an
    // origin ref so it resolves to an up-to-date commit.
    let target = if let Some(branch) = worktree_branch.as_deref() {
        let fetch_first = start_point.as_deref().is_some_and(|sp| sp.starts_with("origin/"));
        SessionTarget::NewWorktree {
            branch,
            start_point: start_point.as_deref(),
            fetch_first,
        }
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
        None, // smol_machine_name - CLI sessions don't bind at create time
        Some(&state.automation_hooks),
        app,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return Response::err(format!("{}", e)),
    };

    let session_id = session.id.clone();

    // Write the initial prompt to the primary PTY before notifying the
    // frontend so the text is in the PTY buffer when the pane attaches.
    if let Some(ref text) = prompt {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(b'\r');
        if let Err(e) = state.pty_manager.write(&session_id, &bytes) {
            rlog!("Warning: failed to write prompt to session {}: {}", session_id, e);
        }
    }

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

    // Inherit the session's smol-machine binding (if any) so CLI-spawned
    // shells land inside the same VM as the primary pane. A bound
    // session whose smolvm has been uninstalled fails loud rather than
    // silently running on the host.
    let smolvm = match session_record.as_ref().and_then(|s| s.smol_machine_name.as_deref()) {
        Some(name) if !name.trim().is_empty() => {
            match crate::services::smolvm::resolve_smolvm_binary() {
                Some(install) => Some(crate::pty::SmolvmExec {
                    binary: install.path,
                    machine_name: name.trim().to_string(),
                    guest_shell: "/bin/sh".to_string(),
                }),
                None => {
                    return Response::err(format!(
                        "session is bound to smol machine '{name}', but smolvm is not installed"
                    ));
                }
            }
        }
        _ => None,
    };

    if let Err(e) = state.pty_manager.spawn_shell(
        &pty_id,
        &working_dir,
        Some(session_id),
        Some(&pane_id),
        project_id.as_deref(),
        worktree_env.as_deref(),
        None, // notes env snapshot — wired only from session creation path
        None,
        smolvm.as_ref(),
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

    // Inherit the session's smol-machine binding so `roux run` lands
    // inside the same VM as the primary pane and shells. Mirrors the
    // smolvm hookup in `handle_shell` above; without this, a bound
    // session running `roux run <cmd>` would silently execute on the
    // host and bypass the guest's network / rootfs / process tree.
    let smolvm = match session_record.as_ref().and_then(|s| s.smol_machine_name.as_deref()) {
        Some(name) if !name.trim().is_empty() => {
            match crate::services::smolvm::resolve_smolvm_binary() {
                Some(install) => Some(crate::pty::SmolvmExec {
                    binary: install.path,
                    machine_name: name.trim().to_string(),
                    guest_shell: "/bin/sh".to_string(),
                }),
                None => {
                    return Response::err(format!(
                        "session is bound to smol machine '{name}', but smolvm is not installed"
                    ));
                }
            }
        }
        _ => None,
    };

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
        smolvm.as_ref(),
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
///
/// `pane_type` is an optional filter: when set and `pane_id` is absent,
/// resolves to the first registered pane of that type in the session
/// (e.g. "shell" targets a non-agent shell pane).
async fn resolve_send_pty_id(
    pane_handle: &crate::pane_service::PaneHandle,
    session_handle: &crate::session_service::SessionHandle,
    session_id: &str,
    pane_id: Option<&str>,
    pane_type: Option<&str>,
) -> Result<String, String> {
    if let Some(pane_id) = pane_id {
        let records = pane_handle
            .list_by_ids(vec![pane_id.to_string()])
            .await
            .map_err(|e| format!("pane lookup failed: {}", e))?;
        let record =
            records.into_iter().next().ok_or_else(|| format!("pane not found: {}", pane_id))?;

        // Defensive: reject a pane that doesn't belong to the requested
        // session. Pane IDs follow the `{session}-{suffix}` convention
        // (see services/sessions.rs), so the prefix check is sufficient.
        if !record.id.starts_with(&format!("{}-", session_id)) {
            return Err(format!("pane {} does not belong to session {}", pane_id, session_id));
        }
        return Ok(record.pty_id);
    }

    if let Some(pt) = pane_type {
        let mut records = pane_handle
            .list_by_session(session_id)
            .await
            .map_err(|e| format!("pane lookup failed: {}", e))?;
        // Stable order: sort by id so repeated calls return the same pane.
        records.sort_by(|a, b| a.id.cmp(&b.id));
        return records
            .into_iter()
            .find(|r| r.pane_type == pt)
            .map(|r| r.pty_id)
            .ok_or_else(|| format!("no '{}' pane found in session {}", pt, session_id));
    }

    match session_handle.get(session_id).await {
        // The canonical PTY for a live session has `pty_id == session.id` by
        // convention (see services/sessions.rs::create_session_shell and
        // reconnect_session_shell). `primary_pty_id` on the persisted record
        // can lag — archive() clears it and reconnect doesn't re-set it — so
        // use the persisted value when set, falling back to session.id when
        // it's None. This matches the pre-fix behavior where the handler
        // wrote to `session_id` directly.
        Ok(Some(session)) => Ok(session.primary_pty_id.unwrap_or_else(|| session_id.to_string())),
        Ok(None) => Err(format!("session not found: {}", session_id)),
        Err(e) => Err(format!("session lookup failed: {}", e)),
    }
}

/// Resolve which PTY's recent output to read. Unlike `send`, a pane-only
/// request is valid because reading is explicit and read-only.
async fn resolve_latest_output_pty_id(
    pane_handle: &crate::pane_service::PaneHandle,
    session_handle: &crate::session_service::SessionHandle,
    session_id: Option<&str>,
    pane_id: Option<&str>,
) -> Result<String, String> {
    if let Some(pane_id) = pane_id {
        let records = pane_handle
            .list_by_ids(vec![pane_id.to_string()])
            .await
            .map_err(|e| format!("pane lookup failed: {}", e))?;
        let record =
            records.into_iter().next().ok_or_else(|| format!("pane not found: {}", pane_id))?;

        if let Some(session_id) = session_id {
            if !record.id.starts_with(&format!("{}-", session_id)) {
                return Err(format!("pane {} does not belong to session {}", pane_id, session_id));
            }
        }

        return Ok(record.pty_id);
    }

    if let Some(session_id) = session_id {
        return resolve_send_pty_id(pane_handle, session_handle, session_id, None, None).await;
    }

    Err("session_id or pane_id required".to_string())
}

fn latest_output_max_bytes(args: &serde_json::Value) -> usize {
    args.get("max_bytes")
        .or_else(|| args.get("maxBytes"))
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(1, MAX_LATEST_OUTPUT_BYTES))
        .unwrap_or(DEFAULT_LATEST_OUTPUT_BYTES)
}

async fn handle_latest_output(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let max_bytes = latest_output_max_bytes(&req.args);
    let pty_id = match resolve_latest_output_pty_id(
        &state.pane_handle,
        &state.session_handle,
        req.session_id.as_deref(),
        req.pane_id.as_deref(),
    )
    .await
    {
        Ok(id) => id,
        Err(e) => return Response::err(e),
    };

    let Some(info) = state.pty_manager.get_info_direct(&pty_id) else {
        return Response::err(format!("pty not found: {}", pty_id));
    };
    let pane_id = req.pane_id.clone().or_else(|| match &info.status {
        crate::pty::PtyStatus::RunningAttached { pane_id } => Some(pane_id.clone()),
        _ => None,
    });
    let bytes = state.pty_manager.get_replay(&pty_id, max_bytes);
    Response::success(latest_output_payload(info.session_id, pane_id, pty_id, max_bytes, &bytes))
}

fn latest_output_payload(
    session_id: Option<String>,
    pane_id: Option<String>,
    pty_id: String,
    max_bytes: usize,
    bytes: &[u8],
) -> serde_json::Value {
    let mut data = serde_json::Map::new();
    data.insert("session_id".into(), optional_string_value(session_id));
    data.insert("pane_id".into(), optional_string_value(pane_id));
    data.insert("pty_id".into(), serde_json::Value::String(pty_id));
    data.insert("max_bytes".into(), serde_json::Value::Number(max_bytes.into()));
    data.insert("byte_count".into(), serde_json::Value::Number(bytes.len().into()));
    data.insert(
        "replay_bytes_base64".into(),
        serde_json::Value::String(BASE64_STANDARD.encode(bytes)),
    );
    if let Ok(text) = std::str::from_utf8(bytes) {
        data.insert("text".into(), serde_json::Value::String(text.to_string()));
    }
    serde_json::Value::Object(data)
}

fn optional_string_value(value: Option<String>) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, serde_json::Value::String)
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

    let session_id = req.session_id.as_deref().ok_or_else(|| "session_id required".to_string())?;

    let pane_type = req.args.get("pane_type").and_then(|v| v.as_str());
    let pty_id =
        resolve_send_pty_id(
            pane_handle,
            session_handle,
            session_id,
            req.pane_id.as_deref(),
            pane_type,
        )
        .await?;

    let cr = req.args.get("enter").and_then(|v| v.as_bool()).unwrap_or(true);
    let data = format_send_data(&text, cr).into_bytes();
    Ok((pty_id, data))
}

async fn handle_send(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let (pty_id, bytes) = match prepare_send(&state.pane_handle, &state.session_handle, &req).await
    {
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

// ---------------------------------------------------------------------------
// Alias handlers — bind/unbind/claim/list/get/whoami over the socket.
// All write paths fan out via Tauri's `alias-event` so the frontend can
// react without polling.
// ---------------------------------------------------------------------------

fn args_str<'a>(req: &'a Request, key: &str) -> Option<&'a str> {
    req.args.get(key).and_then(|v| v.as_str())
}

fn args_bool(req: &Request, key: &str) -> Option<bool> {
    req.args.get(key).and_then(|v| v.as_bool())
}

/// Build a `ProjectFilter` from optional `project` / `global` args.
/// - `project` set → `Exact(Some(project))`
/// - `global=true` (and `project` absent) → `Exact(None)` (global-only)
/// - both absent → `Any`
fn alias_project_filter<'a>(
    project: Option<&'a str>,
    global: Option<bool>,
) -> roux_lib::aliases::ProjectFilter<'a> {
    use roux_lib::aliases::ProjectFilter;
    match (project, global) {
        (Some(p), _) => ProjectFilter::Exact(Some(p)),
        (None, Some(true)) => ProjectFilter::Exact(None),
        (None, _) => ProjectFilter::Any,
    }
}

async fn handle_alias_set(req: Request, app: &tauri::AppHandle) -> Response {
    let raw_alias = match args_str(&req, "alias") {
        Some(s) => s,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(c) => c,
        Err(e) => return Response::err(e.to_string()),
    };
    // Caller may target a specific session via args.session_id; otherwise
    // default to the calling session's id.
    let session_id = args_str(&req, "session_id")
        .map(String::from)
        .or_else(|| req.session_id.clone());
    let session_id = match session_id {
        Some(s) => s,
        None => return Response::err("session_id required (call from a session, or pass args.session_id)"),
    };
    let project_id = args_str(&req, "project_id").map(String::from);
    let force = args_bool(&req, "force").unwrap_or(false);
    // `args.pane_id` overrides `req.pane_id` (both are optional). Without
    // either, the binding stays at session-level for Phase-1 compat.
    let pane_id = args_str(&req, "pane_id")
        .map(String::from)
        .or_else(|| req.pane_id.clone());

    let state: tauri::State<AppState> = app.state();
    let bind_req = roux_lib::aliases::BindRequest {
        project_id,
        session_id: Some(session_id),
        pane_id,
        auto_claimed: false,
        force,
    };
    match state.alias_manager.bind(&canonical, bind_req, Some(app)) {
        Ok(alias) => Response::success(serde_json::to_value(alias).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_alias_unset(req: Request, app: &tauri::AppHandle) -> Response {
    let raw_alias = match args_str(&req, "alias") {
        Some(s) => s,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(c) => c,
        Err(e) => return Response::err(e.to_string()),
    };
    let project_id = args_str(&req, "project_id");
    let state: tauri::State<AppState> = app.state();
    let changed = state.alias_manager.unbind(&canonical, project_id, Some(app));
    Response::success(serde_json::json!({ "changed": changed }))
}

async fn handle_alias_claim(req: Request, app: &tauri::AppHandle) -> Response {
    let raw_alias = match args_str(&req, "alias") {
        Some(s) => s,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(c) => c,
        Err(e) => return Response::err(e.to_string()),
    };
    let session_id = match req.session_id.clone() {
        Some(s) => s,
        None => return Response::err("alias-claim must be invoked from inside a session"),
    };
    let project_id = args_str(&req, "project_id").map(String::from);
    let steal = args_bool(&req, "steal").unwrap_or(false);
    // Claim picks up the calling pane via `req.pane_id` (set by the CLI
    // from `$ROUX_PANE_ID`). `args.pane_id` lets the caller target a
    // specific pane explicitly, useful from MCP / programmatic clients.
    let pane_id = args_str(&req, "pane_id")
        .map(String::from)
        .or_else(|| req.pane_id.clone());

    let state: tauri::State<AppState> = app.state();
    let bind_req = roux_lib::aliases::BindRequest {
        project_id,
        session_id: Some(session_id),
        pane_id,
        auto_claimed: false,
        force: steal,
    };
    match state.alias_manager.bind(&canonical, bind_req, Some(app)) {
        Ok(alias) => Response::success(serde_json::to_value(alias).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_alias_list(req: Request, app: &tauri::AppHandle) -> Response {
    let project = args_str(&req, "project_id");
    let global = args_bool(&req, "global");
    let only_unbound = args_bool(&req, "only_unbound").unwrap_or(false);
    let filter = alias_project_filter(project, global);

    let state: tauri::State<AppState> = app.state();
    let aliases = state.alias_manager.list(filter, only_unbound);
    Response::success(serde_json::to_value(aliases).unwrap_or_default())
}

async fn handle_alias_get(req: Request, app: &tauri::AppHandle) -> Response {
    let raw_alias = match args_str(&req, "alias") {
        Some(s) => s,
        None => return Response::err("alias required"),
    };
    // `validate_alias_name` (not `validate_user_alias_name`) so callers can
    // resolve reserved aliases like `me`.
    let canonical = match roux_core::validate_alias_name(raw_alias) {
        Ok(c) => c,
        Err(e) => return Response::err(e.to_string()),
    };
    let project_id = args_str(&req, "project_id");

    let state: tauri::State<AppState> = app.state();
    if let Some(alias) = state.alias_manager.get(&canonical, project_id) {
        Response::success(serde_json::to_value(alias).unwrap_or_default())
    } else if project_id.is_none() {
        // Bare-alias resolution: no exact (canonical, None) entry, but the
        // alias might exist scoped to a project. Surface ambiguity to the
        // caller so it can re-issue with `--project`.
        let matches = state.alias_manager.find_all_by_name(&canonical);
        match matches.len() {
            0 => Response::err(format!("alias '{canonical}' not found")),
            1 => Response::success(serde_json::to_value(&matches[0]).unwrap_or_default()),
            _ => {
                let projects: Vec<_> =
                    matches.iter().map(|a| a.project_id.clone()).collect();
                Response::err(format!(
                    "alias '{canonical}' is ambiguous across projects {projects:?}; pass project_id"
                ))
            }
        }
    } else {
        Response::err(format!("alias '{canonical}' not found"))
    }
}

async fn handle_alias_whoami(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = args_str(&req, "session_id")
        .map(String::from)
        .or_else(|| req.session_id.clone());
    let session_id = match session_id {
        Some(s) => s,
        None => return Response::err("session_id required (call from a session, or pass args.session_id)"),
    };
    let state: tauri::State<AppState> = app.state();
    let aliases = state.alias_manager.whoami(&session_id);
    Response::success(serde_json::to_value(aliases).unwrap_or_default())
}

async fn handle_alias_add_member(req: Request, app: &tauri::AppHandle) -> Response {
    use roux_core::validate_user_alias_name;
    let alias = match args_str(&req, "alias") {
        Some(s) => match validate_user_alias_name(s) {
            Ok(canon) => canon,
            Err(e) => return Response::err(e.to_string()),
        },
        None => return Response::err("alias required"),
    };
    let pane_id = match args_str(&req, "pane_id").map(str::to_string).or_else(|| req.pane_id.clone()) {
        Some(p) => p,
        None => {
            return Response::err(
                "pane_id required (call from a pane, or pass args.pane_id)",
            )
        }
    };
    let project_id = args_str(&req, "project_id").map(str::to_string);
    let state: tauri::State<AppState> = app.state();
    match state.alias_manager.add_member(&alias, project_id.as_deref(), &pane_id, Some(app)) {
        Ok(a) => Response::success(serde_json::to_value(a).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_alias_remove_member(req: Request, app: &tauri::AppHandle) -> Response {
    use roux_core::validate_user_alias_name;
    let alias = match args_str(&req, "alias") {
        Some(s) => match validate_user_alias_name(s) {
            Ok(canon) => canon,
            Err(e) => return Response::err(e.to_string()),
        },
        None => return Response::err("alias required"),
    };
    let pane_id = match args_str(&req, "pane_id").map(str::to_string).or_else(|| req.pane_id.clone()) {
        Some(p) => p,
        None => return Response::err("pane_id required"),
    };
    let project_id = args_str(&req, "project_id").map(str::to_string);
    let state: tauri::State<AppState> = app.state();
    match state.alias_manager.remove_member(&alias, project_id.as_deref(), &pane_id, Some(app)) {
        Ok(removed) => Response::success(serde_json::json!({ "removed": removed })),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_alias_mode(req: Request, app: &tauri::AppHandle) -> Response {
    use roux_core::{validate_user_alias_name, ConsumptionMode};
    let alias = match args_str(&req, "alias") {
        Some(s) => match validate_user_alias_name(s) {
            Ok(canon) => canon,
            Err(e) => return Response::err(e.to_string()),
        },
        None => return Response::err("alias required"),
    };
    let mode = match args_str(&req, "mode") {
        Some("competing") | Some("competingConsumer") | Some("competing-consumer") => {
            ConsumptionMode::CompetingConsumer
        }
        Some("broadcast") => ConsumptionMode::Broadcast,
        Some(other) => {
            return Response::err(format!(
                "invalid mode '{other}'; expected 'competing' or 'broadcast'"
            ))
        }
        None => return Response::err("mode required"),
    };
    let project_id = args_str(&req, "project_id").map(str::to_string);
    let state: tauri::State<AppState> = app.state();
    match state.alias_manager.set_consumption_mode(
        &alias,
        project_id.as_deref(),
        mode,
        Some(app),
    ) {
        Ok(a) => Response::success(serde_json::to_value(a).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Mailbox handlers — categorical event store surface (`to`-addressed mail
// + per-recipient read/ack state). Bus and reply/sent live in a separate
// section below.
// ---------------------------------------------------------------------------

fn parse_event_kind(s: &str) -> Result<roux_core::EventKind, String> {
    use roux_core::EventKind;
    match s {
        "task" => Ok(EventKind::Task),
        "result" => Ok(EventKind::Result),
        "question" => Ok(EventKind::Question),
        "fyi" => Ok(EventKind::Fyi),
        "signal" => Ok(EventKind::Signal),
        other => Err(format!(
            "invalid kind: {other}; expected task|result|question|fyi|signal"
        )),
    }
}

/// Pick the recipient alias for receive-side commands (peek/read/ack/count/clear).
/// Caller may pass `args.alias` explicitly. Otherwise we resolve the
/// caller's identity in this priority:
///   1. Pane-bound aliases (`req.pane_id` → `find_for_pane`) — Phase 1.5.
///   2. Session-bound aliases (`req.session_id` → `whoami`) — Phase 1 compat.
///   3. Error.
///
/// Multiple matches → error and ask for disambiguation via `args.alias`.
fn resolve_recipient_alias(
    state: &tauri::State<'_, AppState>,
    req: &Request,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(a) = explicit {
        return roux_core::validate_alias_name(a).map_err(|e| e.to_string());
    }

    let mut candidates: Vec<roux_core::AgentAlias> = Vec::new();
    if let Some(pid) = req.pane_id.as_deref() {
        candidates.extend(state.alias_manager.find_for_pane(pid));
    }
    if candidates.is_empty() {
        if let Some(sid) = req.session_id.as_deref() {
            candidates.extend(state.alias_manager.whoami(sid));
        }
    }

    match candidates.len() {
        0 => Err(format!(
            "no alias bound to {context}; claim one with `roux alias claim <name>` or pass args.alias",
            context = req
                .pane_id
                .as_deref()
                .map(|p| format!("pane {p}"))
                .or_else(|| req.session_id.as_deref().map(|s| format!("session {s}")))
                .unwrap_or_else(|| "this caller".to_string())
        )),
        1 => Ok(candidates[0].alias.clone()),
        _ => {
            let names: Vec<_> = candidates.iter().map(|a| a.alias.clone()).collect();
            Err(format!(
                "caller holds multiple aliases ({names:?}); pass args.alias"
            ))
        }
    }
}

/// Default `from` for outgoing posts: prefer the calling pane's auto-claim
/// (if exactly one alias bound to `req.pane_id`), else the session's
/// primary alias, else the session_id itself, else `None`.
fn default_from(state: &tauri::State<AppState>, req: &Request) -> Option<String> {
    if let Some(pid) = req.pane_id.as_deref() {
        let pane_aliases = state.alias_manager.find_for_pane(pid);
        if pane_aliases.len() == 1 {
            return Some(pane_aliases[0].alias.clone());
        }
    }
    let session_id = req.session_id.as_deref()?;
    let mine = state.alias_manager.whoami(session_id);
    if mine.len() == 1 {
        Some(mine[0].alias.clone())
    } else {
        Some(session_id.to_string())
    }
}

fn mailbox_project_filter<'a>(
    project: Option<&'a str>,
    global: Option<bool>,
) -> roux_lib::aliases::ProjectFilter<'a> {
    use roux_lib::aliases::ProjectFilter;
    match (project, global) {
        (Some(p), _) => ProjectFilter::Exact(Some(p)),
        (None, Some(true)) => ProjectFilter::Exact(None),
        (None, _) => ProjectFilter::Any,
    }
}

async fn handle_mailbox_post(req: Request, app: &tauri::AppHandle) -> Response {
    use roux_core::EventBuilder;

    let body = match args_str(&req, "body") {
        Some(s) => s.to_string(),
        None => return Response::err("body required"),
    };

    let to_raw = args_str(&req, "to");
    let topic = args_str(&req, "topic").map(String::from);
    if to_raw.is_none() && topic.is_none() {
        return Response::err("at least one of `to` or `topic` required");
    }

    let canonical_to = match to_raw {
        Some(raw) => match roux_core::validate_alias_name(raw) {
            Ok(c) => Some(c),
            Err(e) => return Response::err(e.to_string()),
        },
        None => None,
    };

    let kind = match args_str(&req, "kind") {
        Some(s) => match parse_event_kind(s) {
            Ok(k) => k,
            Err(e) => return Response::err(e),
        },
        None => roux_core::EventKind::Task,
    };

    let state: tauri::State<AppState> = app.state();

    let from = match args_str(&req, "from") {
        Some(s) => Some(s.to_string()),
        None => default_from(&state, &req),
    };

    let project_id = args_str(&req, "project_id").map(String::from);
    let subject = args_str(&req, "subject").map(String::from);
    let correlation_id = args_str(&req, "correlation_id").map(String::from);
    let structured = req.args.get("structured").cloned();

    // Materialize an unbound alias entry if the recipient hasn't been
    // claimed yet — keeps queued mail addressed and lets a future session
    // pick it up via `roux alias claim`.
    if let Some(c) = &canonical_to {
        state.alias_manager.ensure(c, project_id.clone(), Some(app));
    }

    let mut builder = EventBuilder::new(body).kind(kind);
    if let Some(c) = canonical_to {
        builder = builder.to(c);
    }
    if let Some(t) = topic {
        builder = builder.topic(t);
    }
    if let Some(f) = from {
        builder = builder.from(f);
    }
    if let Some(p) = project_id {
        builder = builder.project_id(p);
    }
    if let Some(s) = subject {
        builder = builder.subject(s);
    }
    if let Some(c) = correlation_id {
        builder = builder.correlation_id(c);
    }
    if let Some(v) = structured {
        if !v.is_null() {
            builder = builder.structured(v);
        }
    }

    match state.mailbox_manager.post(builder, Some(app)) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_mailbox_peek(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    let unread_only = args_bool(&req, "unread").unwrap_or(false);
    let filter = mailbox_project_filter(args_str(&req, "project_id"), args_bool(&req, "global"));
    let mut events =
        state.mailbox_manager.list_for_recipient(&alias, unread_only, filter);
    if let Some(limit) = req.args.get("limit").and_then(|v| v.as_u64()) {
        events.truncate(limit as usize);
    }
    Response::success(serde_json::to_value(events).unwrap_or_default())
}

async fn handle_mailbox_read(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    let with_ack = args_bool(&req, "ack").unwrap_or(false);
    let filter = mailbox_project_filter(args_str(&req, "project_id"), args_bool(&req, "global"));
    let mut events =
        state.mailbox_manager.list_for_recipient(&alias, true, filter);
    if let Some(limit) = req.args.get("limit").and_then(|v| v.as_u64()) {
        events.truncate(limit as usize);
    }
    // Mark each returned event read (and optionally ack).
    for e in &events {
        state.mailbox_manager.mark_read(&e.id, &alias, Some(app));
        if with_ack {
            state.mailbox_manager.ack(&e.id, &alias, None, Some(app));
        }
    }
    Response::success(serde_json::to_value(events).unwrap_or_default())
}

async fn handle_mailbox_ack(req: Request, app: &tauri::AppHandle) -> Response {
    let event_id = match args_str(&req, "event_id") {
        Some(s) => s.to_string(),
        None => return Response::err("event_id required"),
    };
    let state: tauri::State<AppState> = app.state();
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    let result = args_str(&req, "result").map(String::from);
    let changed = state.mailbox_manager.ack(&event_id, &alias, result, Some(app));
    Response::success(serde_json::json!({ "changed": changed }))
}

async fn handle_mailbox_retract(req: Request, app: &tauri::AppHandle) -> Response {
    let event_id = match args_str(&req, "event_id") {
        Some(s) => s.to_string(),
        None => return Response::err("event_id required"),
    };
    let state: tauri::State<AppState> = app.state();
    // Retract is a sender-side action: caller must be the alias that
    // sent the event. Use `args.alias` if given, else the calling
    // pane's bound alias.
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    match state.mailbox_manager.retract(&event_id, &alias, Some(app)) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_mailbox_dismiss(req: Request, app: &tauri::AppHandle) -> Response {
    let event_id = match args_str(&req, "event_id") {
        Some(s) => s.to_string(),
        None => return Response::err("event_id required"),
    };
    let state: tauri::State<AppState> = app.state();
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    let changed = state.mailbox_manager.dismiss(&event_id, &alias, Some(app));
    Response::success(serde_json::json!({ "changed": changed }))
}

async fn handle_mailbox_count(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    let filter = mailbox_project_filter(args_str(&req, "project_id"), args_bool(&req, "global"));
    let count = state.mailbox_manager.unread_count(&alias, filter);
    Response::success(serde_json::json!({ "unread": count }))
}

async fn handle_mailbox_clear(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    let filter = mailbox_project_filter(args_str(&req, "project_id"), args_bool(&req, "global"));
    let removed = state.mailbox_manager.clear_read(&alias, filter, Some(app));
    Response::success(serde_json::json!({ "cleared": removed }))
}

// ---------------------------------------------------------------------------
// Reply / sent / bus surface. Reply threads via correlation_id; sent shows
// outgoing events with per-recipient state. Bus publish/tail are thin
// facades over the same store, biased toward topic-style usage.
// ---------------------------------------------------------------------------

async fn handle_mailbox_reply(req: Request, app: &tauri::AppHandle) -> Response {
    use roux_core::EventBuilder;

    let event_id = match args_str(&req, "event_id") {
        Some(s) => s.to_string(),
        None => return Response::err("event_id required"),
    };
    let body = match args_str(&req, "body") {
        Some(s) => s.to_string(),
        None => return Response::err("body required"),
    };

    let state: tauri::State<AppState> = app.state();
    let original = match state.mailbox_manager.get(&event_id) {
        Some(e) => e,
        None => return Response::err(format!("event_id not found: {event_id}")),
    };

    let recipient = match original.from.as_deref() {
        Some(r) => r.to_string(),
        None => {
            return Response::err(
                "cannot reply: original event has no `from` (anonymous sender)",
            );
        }
    };
    let canonical_to = roux_core::validate_alias_name(&recipient).ok().unwrap_or(recipient);

    // Inherit correlation_id; if the original lacks one, seed a thread
    // using the original event's id so the conversation is groupable.
    let correlation_id = original.correlation_id.clone().unwrap_or_else(|| original.id.clone());

    let from = match args_str(&req, "from") {
        Some(s) => Some(s.to_string()),
        None => default_from(&state, &req),
    };

    let kind = match args_str(&req, "kind") {
        Some(s) => match parse_event_kind(s) {
            Ok(k) => k,
            Err(e) => return Response::err(e),
        },
        None => roux_core::EventKind::Result,
    };

    let mut builder = EventBuilder::new(body)
        .to(canonical_to.clone())
        .kind(kind)
        .correlation_id(correlation_id);
    if let Some(f) = from {
        builder = builder.from(f);
    }
    if let Some(s) = args_str(&req, "subject") {
        builder = builder.subject(s.to_string());
    }
    if let Some(p) = original.project_id {
        builder = builder.project_id(p);
    }
    if let Some(v) = req.args.get("structured").cloned() {
        if !v.is_null() {
            builder = builder.structured(v);
        }
    }

    state.alias_manager.ensure(&canonical_to, builder.project_id.clone(), Some(app));

    match state.mailbox_manager.post(builder, Some(app)) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_mailbox_sent(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let sender = match args_str(&req, "sender") {
        Some(s) => s.to_string(),
        None => match default_from(&state, &req) {
            Some(s) => s,
            None => {
                return Response::err(
                    "sender required (call from a session, or pass args.sender)",
                );
            }
        },
    };
    let recipient_filter = args_str(&req, "to");
    let limit = req.args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize);
    let pairs = state.mailbox_manager.list_sent_by(&sender, recipient_filter, limit);

    // Shape the response so consumers can read both the event and the
    // recipient's read/ack state in one shot.
    let payload: Vec<serde_json::Value> = pairs
        .into_iter()
        .map(|(event, state)| {
            serde_json::json!({
                "event": event,
                "state": state,
            })
        })
        .collect();
    Response::success(serde_json::Value::Array(payload))
}

async fn handle_bus_publish(req: Request, app: &tauri::AppHandle) -> Response {
    use roux_core::EventBuilder;

    let topic = match args_str(&req, "topic") {
        Some(s) => s.to_string(),
        None => return Response::err("topic required"),
    };
    let body = match args_str(&req, "body") {
        Some(s) => s.to_string(),
        None => "".to_string(),
    };
    let structured = req.args.get("structured").cloned();
    if body.trim().is_empty() && structured.as_ref().map(|v| v.is_null()).unwrap_or(true) {
        return Response::err("body or structured payload required");
    }

    let state: tauri::State<AppState> = app.state();
    let kind = match args_str(&req, "kind") {
        Some(s) => match parse_event_kind(s) {
            Ok(k) => k,
            Err(e) => return Response::err(e),
        },
        None => roux_core::EventKind::Signal,
    };
    let from = match args_str(&req, "from") {
        Some(s) => Some(s.to_string()),
        None => default_from(&state, &req),
    };

    let mut builder = EventBuilder::new(body).topic(topic).kind(kind);
    if let Some(f) = from {
        builder = builder.from(f);
    }
    if let Some(p) = args_str(&req, "project_id") {
        builder = builder.project_id(p.to_string());
    }
    if let Some(s) = args_str(&req, "subject") {
        builder = builder.subject(s.to_string());
    }
    if let Some(v) = structured {
        if !v.is_null() {
            builder = builder.structured(v);
        }
    }

    match state.mailbox_manager.post(builder, Some(app)) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_bus_tail(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let limit = req.args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize);
    let filter = mailbox_project_filter(args_str(&req, "project_id"), args_bool(&req, "global"));

    let events = match args_str(&req, "topic") {
        Some(t) => {
            let mut events = state.mailbox_manager.list_for_topic(t, filter);
            if let Some(n) = limit {
                events.truncate(n);
            }
            events
        }
        None => state.mailbox_manager.list_all(filter, limit),
    };
    Response::success(serde_json::to_value(events).unwrap_or_default())
}

/// Resolve the alias to subscribe under: explicit `--alias` wins, else
/// the calling pane's bound alias. We require *something* to bind to
/// — anonymous subscriptions can't deliver mail anywhere. When the
/// pane holds multiple aliases (manual + auto-claim, or several manual
/// claims) we refuse to guess and return a disambiguation error;
/// silently picking by `created_at` would route deliveries to the
/// wrong inbox.
fn resolve_subscriber_alias(req: &Request, app: &tauri::AppHandle) -> Result<String, String> {
    if let Some(a) = args_str(req, "alias") {
        return Ok(a.to_string());
    }
    let state: tauri::State<AppState> = app.state();
    let pane_id = req.pane_id.as_deref().ok_or_else(|| {
        "no --alias given and no pane context available; pass --alias <name>".to_string()
    })?;
    let held = state.alias_manager.find_for_pane(pane_id);
    match held.len() {
        0 => Err(
            "no --alias given and the calling pane holds no alias; pass --alias <name>"
                .to_string(),
        ),
        1 => Ok(held[0].alias.clone()),
        _ => {
            let names: Vec<_> = held.iter().map(|a| a.alias.as_str()).collect();
            Err(format!(
                "no --alias given and the calling pane holds multiple aliases ({names:?}); pass --alias <name>"
            ))
        }
    }
}

async fn handle_bus_subscribe(req: Request, app: &tauri::AppHandle) -> Response {
    let pattern = match args_str(&req, "pattern") {
        Some(p) if !p.trim().is_empty() => p.to_string(),
        _ => return Response::err("pattern required"),
    };
    let alias = match resolve_subscriber_alias(&req, app) {
        Ok(a) => a,
        Err(e) => return Response::err(e),
    };
    let project_id = args_str(&req, "project_id").map(str::to_string);

    let state: tauri::State<AppState> = app.state();
    match state.subscription_manager.subscribe(&alias, &pattern, project_id, Some(app)) {
        Ok(s) => Response::success(serde_json::to_value(s).unwrap_or_default()),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_bus_unsubscribe(req: Request, app: &tauri::AppHandle) -> Response {
    let id = match args_str(&req, "id") {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Response::err("subscription id required"),
    };
    let state: tauri::State<AppState> = app.state();
    match state.subscription_manager.unsubscribe(&id, Some(app)) {
        Ok(removed) => Response::success(serde_json::json!({ "removed": removed })),
        Err(e) => Response::err(e.to_string()),
    }
}

async fn handle_bus_subscriptions(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();
    let filter = mailbox_project_filter(args_str(&req, "project_id"), args_bool(&req, "global"));
    let subs = match args_str(&req, "alias") {
        Some(a) => state.subscription_manager.for_alias(a, filter),
        None => state.subscription_manager.list(filter),
    };
    Response::success(serde_json::to_value(subs).unwrap_or_default())
}

/// Long-lived `mailbox watch` stream. The client connects, receives a
/// `ready` line, then reads newline-delimited JSON frames until it
/// disconnects:
///
/// - `{"type":"ready"}` — handshake; safe to start consuming.
/// - `{"type":"event","event":{...}}` — a fresh event (initial backlog
///   or live delivery). When `args.ack=true` the watch handler also
///   calls `mark_read+ack` on the recipient before forwarding.
/// - `{"type":"error","error":"..."}` — terminal error; stream ends.
///
/// The reader side of the stream is consulted only for client
/// disconnect detection — any line the client sends terminates the
/// watch (treated as "client done"). Real bidirectional control is
/// future work.
async fn handle_mailbox_watch<R, W>(
    req: Request,
    app: &tauri::AppHandle,
    reader: BufReader<R>,
    writer: W,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let state: tauri::State<AppState> = app.state();
    let alias = match resolve_recipient_alias(&state, &req, args_str(&req, "alias")) {
        Ok(a) => a,
        Err(e) => return stream_error(writer, e).await,
    };
    let project_filter =
        mailbox_project_filter(args_str(&req, "project_id"), args_bool(&req, "global"));
    let ack = args_bool(&req, "ack").unwrap_or(false);
    let send_backlog = args_bool(&req, "backlog").unwrap_or(true);

    watch_stream_loop(
        &state.mailbox_manager,
        &alias,
        project_filter,
        ack,
        send_backlog,
        reader,
        writer,
    )
    .await;
}

/// Streaming watch loop, factored out of `handle_mailbox_watch` so it
/// can be exercised in unit tests without a Tauri AppState. Owns the
/// reader/writer for the lifetime of the watch and returns when the
/// client disconnects, the broadcast closes, or a write fails.
///
/// The `MailboxManager` already carries the `SubscriptionManager` (via
/// `with_subscriptions`); subscription matches reach this loop through
/// the broadcast channel as `MailboxEvent::TopicDelivered` frames, so
/// the loop doesn't need a separate handle on subscriptions.
pub(crate) async fn watch_stream_loop<R, W>(
    mailbox: &roux_lib::mailbox::MailboxManager,
    alias: &str,
    project_filter: roux_lib::aliases::ProjectFilter<'_>,
    ack: bool,
    send_backlog: bool,
    mut reader: BufReader<R>,
    mut writer: W,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // Three states, not two: `None` = no filter (Any); `Some(None)` =
    // global-only (event.project_id must be None); `Some(Some(p))` =
    // exact project match. Collapsing `Exact(None)` to `None` would
    // make `--global` watchers receive cross-project events on the
    // live stream while the backlog (which goes through
    // `list_for_recipient(... Exact(None))`) correctly filtered them.
    let project_scope: Option<Option<String>> = match project_filter {
        roux_lib::aliases::ProjectFilter::Any => None,
        roux_lib::aliases::ProjectFilter::Exact(None) => Some(None),
        roux_lib::aliases::ProjectFilter::Exact(Some(p)) => Some(Some(p.to_string())),
    };

    // Subscribe BEFORE replaying the backlog so we don't drop events
    // posted during the handshake. The trade-off is that an event
    // posted between `subscribe_events()` and `list_for_recipient()`
    // can appear in both — `forwarded_ids` below dedupes those.
    let mut rx = mailbox.subscribe_events();

    if write_frame(&mut writer, &serde_json::json!({"type": "ready"})).await.is_err() {
        return;
    }

    // Track event IDs we've already forwarded so the same event can't
    // arrive twice via:
    // (a) backlog list + live broadcast race, or
    // (b) `Posted` arm + `TopicDelivered` arm for the same subscribed
    //     event (both broadcast on every post — the watcher must
    //     deliver only once).
    let mut forwarded_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    if send_backlog {
        let backlog = mailbox.list_for_recipient(alias, true, project_filter);
        for event in backlog {
            forwarded_ids.insert(event.id.clone());
            if !forward_event(mailbox, &mut writer, &event, alias, ack).await {
                return;
            }
        }
    }

    let mut line = String::new();
    loop {
        line.clear();
        tokio::select! {
            recv = rx.recv() => {
                match recv {
                    // Direct-mail-only on this arm: subscription events
                    // are forwarded via TopicDelivered below. Without
                    // this restriction, a subscribed alias would see
                    // each match twice (once here, once on TopicDelivered).
                    Ok(roux_core::MailboxEvent::Posted { event }) => {
                        if event.to.as_deref() != Some(alias) {
                            continue;
                        }
                        if !event_in_scope(&event, project_scope.as_ref()) {
                            continue;
                        }
                        if !forwarded_ids.insert(event.id.clone()) {
                            continue;
                        }
                        if !forward_event(mailbox, &mut writer, &event, alias, ack).await {
                            return;
                        }
                    }
                    Ok(roux_core::MailboxEvent::TopicDelivered { event_id, recipient, .. }) => {
                        if recipient != alias {
                            continue;
                        }
                        let Some(event) = mailbox.get(&event_id) else { continue };
                        if !event_in_scope(&event, project_scope.as_ref()) {
                            continue;
                        }
                        if !forwarded_ids.insert(event.id.clone()) {
                            continue;
                        }
                        if !forward_event(mailbox, &mut writer, &event, alias, ack).await {
                            return;
                        }
                    }
                    // Read/Acked/Cleared aren't watch payloads.
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let warn = serde_json::json!({
                            "type": "warning",
                            "message": format!("dropped {n} buffered events; consumer fell behind"),
                        });
                        if write_frame(&mut writer, &warn).await.is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
            client = reader.read_line(&mut line) => {
                match client {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        // Any client write terminates the watch. Reserved
                        // for future control frames (e.g. dynamic ack).
                        return;
                    }
                }
            }
        }
    }
}

/// True when the event is visible to `alias` under the requested
/// project scope. Visibility = direct mail OR matching subscription.
/// True when `event` falls inside the requested project scope.
/// `scope`:
/// - `None` ⇒ no filter (Any).
/// - `Some(None)` ⇒ global-only; event must also be unscoped.
/// - `Some(Some(p))` ⇒ event must be scoped to project `p`.
fn event_in_scope(event: &roux_core::Event, scope: Option<&Option<String>>) -> bool {
    match scope {
        None => true,
        Some(None) => event.project_id.is_none(),
        Some(Some(p)) => event.project_id.as_deref() == Some(p.as_str()),
    }
}

/// Write one event frame, optionally acking. Returns false on write
/// error so the caller can exit the loop.
async fn forward_event<W>(
    mailbox: &roux_lib::mailbox::MailboxManager,
    writer: &mut W,
    event: &roux_core::Event,
    alias: &str,
    ack: bool,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    // Write the frame first, then ack — if the socket write fails
    // (client disconnected mid-stream), we must NOT stamp the event
    // "watched" since the recipient's agent never actually saw it.
    // Acking before delivery confirmation would corrupt the sender's
    // `mailbox sent` view with a false delivery record.
    let frame = serde_json::json!({ "type": "event", "event": event });
    let delivered = write_frame(writer, &frame).await.is_ok();
    if delivered && ack {
        mailbox.mark_read(&event.id, alias, None);
        mailbox.ack(&event.id, alias, Some("watched".into()), None);
    }
    delivered
}

async fn write_frame<W>(writer: &mut W, value: &serde_json::Value) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    let json = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
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
    fn request_latest_output_deserializes() {
        let json = r#"{
            "command": "latest-output",
            "session_id": "sid-1",
            "pane_id": "sid-1-main",
            "args": {"max_bytes": 4096}
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "latest-output");
        assert_eq!(req.session_id.as_deref(), Some("sid-1"));
        assert_eq!(req.pane_id.as_deref(), Some("sid-1-main"));
        assert_eq!(req.args["max_bytes"], 4096);
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

    #[test]
    fn request_session_create_with_start_point_deserializes() {
        let json = r#"{
            "command": "session-create",
            "args": {
                "working_dir": "/path/to/repo-b",
                "worktree_branch": "feat/x",
                "start_point": "origin/main"
            }
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.args["working_dir"], "/path/to/repo-b");
        assert_eq!(req.args["worktree_branch"], "feat/x");
        assert_eq!(req.args["start_point"], "origin/main");
    }

    #[test]
    fn request_session_rename_deserializes() {
        let json = r#"{
            "command": "session-rename",
            "session_id": "sid-1",
            "args": { "name": "renamed" }
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "session-rename");
        assert_eq!(req.session_id.as_deref(), Some("sid-1"));
        assert_eq!(req.args["name"], "renamed");
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
            session_id: None,
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
            pinned_pr_url: None,
            smol_machine_name: None,
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

        let pty_id =
            resolve_send_pty_id(&panes, &sessions, "sid-1", Some("sid-1-main"), None).await.unwrap();
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

        let pty_id = resolve_send_pty_id(&panes, &sessions, "sid-1", None, None).await.unwrap();
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

        let err =
            resolve_send_pty_id(&panes, &sessions, "sid-2", Some("sid-1-main"), None).await.unwrap_err();
        assert!(err.contains("pane not found"), "got: {}", err);
    }

    #[tokio::test]
    async fn resolve_send_pty_id_cross_session_pane_errors() {
        // The pane exists but belongs to a different session. The resolver
        // must reject it rather than silently routing to the wrong PTY.
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = crate::session_service::spawn_with_path(
            vec![
                session_with_pty("sid-A", Some("sid-A")),
                session_with_pty("sid-B", Some("sid-B")),
            ],
            dir.path().join("sessions.json"),
        );
        panes.upsert(pane_record("sid-A-main", "sid-A")).await.unwrap();
        panes.upsert(pane_record("sid-B-main", "sid-B")).await.unwrap();

        let err =
            resolve_send_pty_id(&panes, &sessions, "sid-B", Some("sid-A-main"), None).await.unwrap_err();
        assert!(err.contains("does not belong to session"), "got: {}", err);
    }

    #[tokio::test]
    async fn resolve_send_pty_id_unknown_session_errors() {
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));

        let err = resolve_send_pty_id(&panes, &sessions, "missing-sid", None, None).await.unwrap_err();
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

        let pty_id = resolve_send_pty_id(&panes, &sessions, "sid-1", None, None).await.unwrap();
        assert_eq!(pty_id, "sid-1");
    }

    #[tokio::test]
    async fn resolve_send_pty_id_pane_type_picks_first_matching_pane() {
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));
        // Claude pane (the main PTY) + two shell panes.
        let mut claude = pane_record("sid-1-main", "sid-1");
        claude.pane_type = "claude".into();
        let mut shell_a = pane_record("sid-1-leaf-a", "pty-shell-a");
        shell_a.pane_type = "shell".into();
        let mut shell_b = pane_record("sid-1-leaf-b", "pty-shell-b");
        shell_b.pane_type = "shell".into();
        panes.upsert(claude).await.unwrap();
        panes.upsert(shell_a).await.unwrap();
        panes.upsert(shell_b).await.unwrap();

        // Asking for "shell" returns the lexically-first shell pane.
        let pty_id =
            resolve_send_pty_id(&panes, &sessions, "sid-1", None, Some("shell")).await.unwrap();
        assert_eq!(pty_id, "pty-shell-a");

        // Asking for a type with no match errors cleanly.
        let err =
            resolve_send_pty_id(&panes, &sessions, "sid-1", None, Some("command")).await.unwrap_err();
        assert!(err.contains("no 'command' pane found"), "got: {}", err);
    }

    #[test]
    fn request_session_kill_deserializes() {
        let json =
            r#"{"command": "session-kill", "session_id": "sid-1"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "session-kill");
        assert_eq!(req.session_id.as_deref(), Some("sid-1"));
    }

    #[test]
    fn request_send_with_pane_type_deserializes() {
        let json = r#"{
            "command": "send",
            "session_id": "sid-1",
            "args": { "text": "ls", "enter": true, "pane_type": "shell" }
        }"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.args["pane_type"], "shell");
        assert_eq!(req.args["text"], "ls");
    }

    #[tokio::test]
    async fn resolve_latest_output_pty_id_accepts_pane_without_session() {
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));
        panes.upsert(pane_record("sid-1-main", "sid-1")).await.unwrap();

        let pty_id = resolve_latest_output_pty_id(&panes, &sessions, None, Some("sid-1-main"))
            .await
            .unwrap();
        assert_eq!(pty_id, "sid-1");
    }

    #[tokio::test]
    async fn resolve_latest_output_pty_id_accepts_session_without_pane() {
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) = crate::session_service::spawn_with_path(
            vec![session_with_pty("sid-1", Some("sid-1"))],
            dir.path().join("sessions.json"),
        );

        let pty_id =
            resolve_latest_output_pty_id(&panes, &sessions, Some("sid-1"), None).await.unwrap();
        assert_eq!(pty_id, "sid-1");
    }

    #[tokio::test]
    async fn resolve_latest_output_pty_id_requires_pane_or_session() {
        let (panes, _pjoin) = crate::pane_service::spawn();
        let dir = tempfile::tempdir().unwrap();
        let (sessions, _sjoin) =
            crate::session_service::spawn_with_path(vec![], dir.path().join("sessions.json"));

        let err = resolve_latest_output_pty_id(&panes, &sessions, None, None).await.unwrap_err();
        assert_eq!(err, "session_id or pane_id required");
    }

    #[test]
    fn latest_output_payload_includes_utf8_text_and_exact_bytes() {
        let payload = latest_output_payload(
            Some("sid-1".into()),
            Some("sid-1-main".into()),
            "pty-1".into(),
            4096,
            b"hello",
        );

        assert_eq!(payload["session_id"], "sid-1");
        assert_eq!(payload["pane_id"], "sid-1-main");
        assert_eq!(payload["pty_id"], "pty-1");
        assert_eq!(payload["max_bytes"], 4096);
        assert_eq!(payload["byte_count"], 5);
        assert_eq!(payload["replay_bytes_base64"], BASE64_STANDARD.encode(b"hello"));
        assert_eq!(payload["text"], "hello");
    }

    #[test]
    fn latest_output_payload_omits_text_for_non_utf8_bytes() {
        let bytes = [0xff, b'a', 0xfe];
        let payload =
            latest_output_payload(Some("sid-1".into()), None, "pty-1".into(), 4096, &bytes);

        assert_eq!(payload["pane_id"], serde_json::Value::Null);
        assert_eq!(payload["byte_count"], 3);
        assert_eq!(payload["replay_bytes_base64"], BASE64_STANDARD.encode(bytes));
        assert!(!payload.as_object().unwrap().contains_key("text"));
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
            pinned_pr_url: None,
            smol_machine_name: None,
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

    // ── mailbox-watch streaming ─────────────────────────────────────

    /// Build an in-memory MailboxManager + SubscriptionManager pair for
    /// streaming tests. Both `in_memory()` ctors are gated to the
    /// owning module's tests, so we go through `load_from` with a
    /// tempdir — same pattern other cross-module tests use.
    fn watch_test_managers() -> (
        tempfile::TempDir,
        roux_lib::mailbox::MailboxManager,
        roux_lib::subscriptions::SubscriptionManager,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("events.jsonl");
        let state = dir.path().join("read_state.json");
        let subs_path = dir.path().join("subscriptions.json");
        let subs = roux_lib::subscriptions::SubscriptionManager::load_from(subs_path);
        let mgr = roux_lib::mailbox::MailboxManager::load_from(events, state)
            .with_subscriptions(subs.clone());
        (dir, mgr, subs)
    }

    /// Helper: spawn the watch loop against a tokio duplex pipe, return
    /// the client side so the test can read frames and close on demand.
    /// `subs` is kept on the signature so callers can wire subscriptions
    /// into the mailbox before spawning; the loop itself reads them via
    /// the broadcast channel, not directly.
    #[allow(clippy::needless_pass_by_value)]
    fn spawn_watch_for_test(
        mailbox: roux_lib::mailbox::MailboxManager,
        _subs: roux_lib::subscriptions::SubscriptionManager,
        alias: &'static str,
        ack: bool,
        send_backlog: bool,
    ) -> tokio::io::DuplexStream {
        spawn_watch_with_filter(
            mailbox,
            alias,
            roux_lib::aliases::ProjectFilter::Any,
            ack,
            send_backlog,
        )
    }

    fn spawn_watch_with_filter(
        mailbox: roux_lib::mailbox::MailboxManager,
        alias: &'static str,
        filter: roux_lib::aliases::ProjectFilter<'static>,
        ack: bool,
        send_backlog: bool,
    ) -> tokio::io::DuplexStream {
        let (server_side, client_side) = tokio::io::duplex(8192);
        let (server_read, server_write) = tokio::io::split(server_side);
        let buf_reader = BufReader::new(server_read);
        tokio::spawn(async move {
            watch_stream_loop(
                &mailbox, alias, filter, ack, send_backlog, buf_reader, server_write,
            )
            .await;
        });
        client_side
    }

    /// Read newline-delimited frames from the watch socket until either
    /// `count` frames are collected or `timeout` elapses. Returns
    /// whatever was read.
    async fn read_frames(
        client: &mut tokio::io::DuplexStream,
        count: usize,
        timeout: std::time::Duration,
    ) -> Vec<serde_json::Value> {
        let (read, _write) = tokio::io::split(client);
        let mut reader = BufReader::new(read);
        let deadline = tokio::time::Instant::now() + timeout;
        let mut frames: Vec<serde_json::Value> = Vec::new();
        while frames.len() < count {
            let mut line = String::new();
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, reader.read_line(&mut line)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(_)) => {
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        frames.push(v);
                    }
                }
                Ok(Err(_)) | Err(_) => break,
            }
        }
        frames
    }

    #[tokio::test]
    async fn watch_emits_ready_then_streams_addressed_event() {
        let (_dir, mgr, subs) = watch_test_managers();
        let mut client = spawn_watch_for_test(mgr.clone(), subs, "auditor", false, true);

        // Give the watch loop a moment to write `ready` before we post.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Post an event addressed to auditor. Must come AFTER `ready`
        // so we hit the live-loop path, not the backlog path.
        let event = mgr
            .post(
                roux_core::EventBuilder::new("hi")
                    .to("auditor")
                    .from("builder")
                    .kind(roux_core::EventKind::Task),
                None,
            )
            .unwrap();

        let frames = read_frames(&mut client, 2, std::time::Duration::from_millis(500)).await;
        assert!(!frames.is_empty(), "expected at least the ready frame");
        assert_eq!(frames[0]["type"], "ready");
        // Find the event frame (might be at index 1).
        let event_frame = frames.iter().find(|f| f["type"] == "event").unwrap();
        assert_eq!(event_frame["event"]["id"], event.id);
    }

    #[tokio::test]
    async fn watch_replays_unread_backlog_first() {
        let (_dir, mgr, subs) = watch_test_managers();
        // Post BEFORE starting the watcher → the event lives in backlog.
        let event = mgr
            .post(
                roux_core::EventBuilder::new("queued")
                    .to("auditor")
                    .from("builder"),
                None,
            )
            .unwrap();

        let mut client = spawn_watch_for_test(mgr, subs, "auditor", false, true);

        let frames = read_frames(&mut client, 2, std::time::Duration::from_millis(500)).await;
        let event_frame = frames.iter().find(|f| f["type"] == "event").unwrap();
        assert_eq!(event_frame["event"]["id"], event.id);
    }

    #[tokio::test]
    async fn watch_no_backlog_skips_existing_events() {
        let (_dir, mgr, subs) = watch_test_managers();
        mgr.post(
            roux_core::EventBuilder::new("queued")
                .to("auditor")
                .from("builder"),
            None,
        )
        .unwrap();

        let mut client =
            spawn_watch_for_test(mgr.clone(), subs, "auditor", false, /* send_backlog */ false);

        let frames = read_frames(&mut client, 1, std::time::Duration::from_millis(150)).await;
        // Only `ready` — no event (queued was unread but backlog suppressed).
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["type"], "ready");
    }

    #[tokio::test]
    async fn watch_filters_other_recipients_from_live_stream() {
        let (_dir, mgr, subs) = watch_test_managers();
        let mut client = spawn_watch_for_test(mgr.clone(), subs, "auditor", false, true);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Post one to a different alias, one to auditor.
        mgr.post(
            roux_core::EventBuilder::new("not for you")
                .to("reviewer")
                .from("builder"),
            None,
        )
        .unwrap();
        let mine = mgr
            .post(
                roux_core::EventBuilder::new("for you")
                    .to("auditor")
                    .from("builder"),
                None,
            )
            .unwrap();

        let frames = read_frames(&mut client, 3, std::time::Duration::from_millis(500)).await;
        let event_ids: Vec<_> = frames
            .iter()
            .filter(|f| f["type"] == "event")
            .map(|f| f["event"]["id"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(event_ids, vec![mine.id], "only auditor-addressed events propagate");
    }

    #[tokio::test]
    async fn watch_streams_subscribed_topic_events() {
        let (_dir, mgr, subs) = watch_test_managers();
        subs.subscribe("auditor", "**.completed", None, None).unwrap();

        let mut client = spawn_watch_for_test(mgr.clone(), subs, "auditor", false, true);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let event = mgr
            .post(
                roux_core::EventBuilder::new("green")
                    .topic("repo-a.build.completed")
                    .from("builder")
                    .kind(roux_core::EventKind::Signal),
                None,
            )
            .unwrap();

        // Read enough frames to catch any duplicate. Pre-fix the loop
        // emitted the same event twice (once via `Posted`, once via
        // `TopicDelivered`); requesting 3 frames and asserting exactly
        // one event row catches the regression.
        let frames = read_frames(&mut client, 3, std::time::Duration::from_millis(300)).await;
        let event_frames: Vec<_> = frames.iter().filter(|f| f["type"] == "event").collect();
        assert_eq!(
            event_frames.len(),
            1,
            "subscribed topic event must be delivered exactly once: {frames:?}"
        );
        assert_eq!(event_frames[0]["event"]["id"], event.id);
    }

    /// Regression for the TOCTOU between `subscribe_events()` and the
    /// backlog `list_for_recipient` call. An event posted in that
    /// window can appear in both, so the watcher needs an in-loop
    /// dedup guard.
    #[tokio::test]
    async fn watch_dedupes_event_seen_in_backlog_and_live_stream() {
        let (_dir, mgr, subs) = watch_test_managers();

        // Post first so the event is unread → goes into the backlog.
        let event = mgr
            .post(
                roux_core::EventBuilder::new("queued")
                    .to("auditor")
                    .from("builder"),
                None,
            )
            .unwrap();

        // The watcher's broadcast subscribe happens inside the spawn,
        // so the broadcast for `event` already fired. The backlog read
        // will surface the same event. Without dedup the watcher would
        // forward it twice.
        let mut client = spawn_watch_for_test(mgr, subs, "auditor", false, true);
        let frames = read_frames(&mut client, 3, std::time::Duration::from_millis(300)).await;
        let event_frames: Vec<_> = frames.iter().filter(|f| f["type"] == "event").collect();
        assert_eq!(
            event_frames.len(),
            1,
            "backlog event must not also be delivered through the live stream: {frames:?}"
        );
        assert_eq!(event_frames[0]["event"]["id"], event.id);
    }

    /// Regression for the `--global` filter being silently dropped on
    /// the live stream. Pre-fix the live arm collapsed `Exact(None)` to
    /// `None` (no filter), so a `--global` watcher would receive
    /// project-scoped events even though the backlog correctly hid
    /// them. The two paths must agree.
    #[tokio::test]
    async fn watch_global_filter_excludes_project_scoped_live_events() {
        let (_dir, mgr, _subs) = watch_test_managers();
        let mut client = spawn_watch_with_filter(
            mgr.clone(),
            "auditor",
            roux_lib::aliases::ProjectFilter::Exact(None),
            false,
            true,
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Project-scoped event must NOT propagate to a global watcher.
        mgr.post(
            roux_core::EventBuilder::new("p1")
                .to("auditor")
                .from("builder")
                .project_id("p1"),
            None,
        )
        .unwrap();
        // Global event MUST propagate.
        let global = mgr
            .post(
                roux_core::EventBuilder::new("g")
                    .to("auditor")
                    .from("builder"),
                None,
            )
            .unwrap();

        let frames = read_frames(&mut client, 3, std::time::Duration::from_millis(300)).await;
        let event_ids: Vec<_> = frames
            .iter()
            .filter(|f| f["type"] == "event")
            .map(|f| f["event"]["id"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(
            event_ids,
            vec![global.id],
            "global watcher must skip project-scoped events: {frames:?}"
        );
    }

    #[tokio::test]
    async fn watch_with_ack_marks_event_read_and_acked() {
        let (_dir, mgr, subs) = watch_test_managers();
        let event = mgr
            .post(
                roux_core::EventBuilder::new("queued")
                    .to("auditor")
                    .from("builder"),
                None,
            )
            .unwrap();

        let mut client =
            spawn_watch_for_test(mgr.clone(), subs, "auditor", /* ack */ true, true);

        // Read until we've seen the event frame, then drop the client to
        // close the watch and let the ack persist before we assert.
        let _ = read_frames(&mut client, 2, std::time::Duration::from_millis(500)).await;
        drop(client);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let state = mgr.read_state(&event.id, "auditor").unwrap();
        assert!(state.is_read(), "watch --ack must mark read");
        assert!(state.is_acked(), "watch --ack must ack");
        assert_eq!(state.ack_result.as_deref(), Some("watched"));
    }

    /// Regression: don't ack on undelivered events. Pre-fix
    /// `forward_event` called `mark_read`/`ack` BEFORE the socket write,
    /// so a client that dropped mid-stream still ended up with the
    /// event stamped "watched" — a false delivery record visible to the
    /// sender. Now we write first and only ack on successful delivery.
    #[tokio::test]
    async fn forward_event_does_not_ack_when_write_fails() {
        let (_dir, mgr, _subs) = watch_test_managers();
        let event = mgr
            .post(
                roux_core::EventBuilder::new("queued")
                    .to("auditor")
                    .from("builder"),
                None,
            )
            .unwrap();

        // tokio::io::sink() with a custom Empty wouldn't fail — instead
        // we close the duplex stream by dropping the client side, then
        // call forward_event against the dead writer.
        let (server_side, client_side) = tokio::io::duplex(8);
        drop(client_side);
        let (_r, mut w) = tokio::io::split(server_side);
        let delivered = forward_event(&mgr, &mut w, &event, "auditor", true).await;
        assert!(!delivered, "write to a closed pipe must report failure");

        let state = mgr.read_state(&event.id, "auditor");
        assert!(
            state.is_none() || !state.unwrap().is_acked(),
            "failed delivery must not leave an acked ReadState behind",
        );
    }
}
