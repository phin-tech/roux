use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
struct Request {
    command: String,
    session_id: Option<String>,
    pane_id: Option<String>,
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
    crate::paths::roux_config_dir().join("roux.sock")
}

pub fn start_socket_server(app: tauri::AppHandle) {
    let path = socket_path();

    tauri::async_runtime::spawn(async move {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Remove stale socket file
        let _ = fs::remove_file(&path);

        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                rlog!("Failed to bind socket at {:?}: {}", path, e);
                return;
            }
        };

        // Set permissions to owner-only (0600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }

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
    match req.command.as_str() {
        "split" => handle_split(req, app),
        "session-create" => handle_session_create(req, app).await,
        "shell" => handle_shell(req, app).await,
        "focus" => handle_focus(req, app),
        "run" => handle_run(req, app).await,
        "send" => handle_send(req, app),
        "notify" => handle_notify(req, app).await,
        _ => Response::err(format!("unknown command: {}", req.command)),
    }
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
    let mut cmd = roux_core::RouxCommand::new("split")
        .session_id(session_id)
        .direction(direction);
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

    // Resolve repo_path: use the requesting session's repo_root, or the working_dir
    let repo_path = match req.session_id.as_deref() {
        Some(id) => match handle.get(id).await {
            Ok(Some(session)) => session.repo_root.clone(),
            _ => working_dir.clone().unwrap_or_default(),
        },
        None => working_dir.clone().unwrap_or_default(),
    };

    if repo_path.is_empty() {
        return Response::err("working_dir or session_id required");
    }

    let settings = state.settings.lock().unwrap().clone();

    // If working_dir differs from repo_path, treat it as an existing worktree
    let target = match &working_dir {
        Some(dir) if dir != &repo_path => SessionTarget::ExistingWorktree { path: dir },
        _ => SessionTarget::Repo,
    };

    let session = match svc::create_session_shell(
        &state.pty_manager,
        &state.session_handle,
        &settings,
        &repo_path,
        &name,
        target,
        None,
        app,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return Response::err(format!("{}", e)),
    };

    let session_id = session.id.clone();

    use tauri::Emitter;
    let cmd = roux_core::RouxCommand::new("session-created").session_id(&session_id);
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

    let working_dir = req
        .args
        .get("working_dir")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string());
    let working_dir = match working_dir {
        Some(dir) => dir,
        None => match handle.get(session_id).await {
            Ok(Some(s)) => s.worktree_path.clone(),
            Ok(None) => return Response::err("could not determine working directory"),
            Err(e) => return Response::err(format!("{}", e)),
        },
    };

    let pane_id = crypto_random_uuid();
    let pty_id = crypto_random_uuid();

    if let Err(e) = state.pty_manager.spawn_shell(
        &pty_id,
        &working_dir,
        Some(session_id),
        Some(&pane_id),
        None,
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

    let working_dir = req
        .args
        .get("working_dir")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string());
    let working_dir = match working_dir {
        Some(dir) => dir,
        None => match handle.get(session_id).await {
            Ok(Some(s)) => s.worktree_path.clone(),
            Ok(None) => return Response::err("could not determine working directory"),
            Err(e) => return Response::err(format!("{}", e)),
        },
    };

    let pane_id = format!("cmd-{}", crypto_random_uuid());
    let pty_id = format!(
        "{}-{}",
        pane_id,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
    );

    if let Err(e) = state.pty_manager.spawn_task(
        &pty_id,
        &command,
        &working_dir,
        Some(session_id),
        Some(&pane_id),
        app.clone(),
    ) {
        return Response::err(format!("Failed to spawn task: {}", e));
    }

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

fn handle_send(req: Request, app: &tauri::AppHandle) -> Response {
    let text = match req.args.get("text").and_then(|t| t.as_str()) {
        Some(t) => t.to_string(),
        None => return Response::err("text argument required"),
    };

    let state: tauri::State<AppState> = app.state();

    // If a specific pane/session is given, use it. Otherwise emit to frontend
    // to send to the active Claude pane.
    if let Some(session_id) = &req.session_id {
        // Write directly to the session's PTY (the main Claude pane uses session_id as pty_id)
        let target_id = req.pane_id.as_deref().unwrap_or(session_id);
        // Append \r to simulate Enter
        let data = format!("{}\r", text);
        if let Err(e) = state.pty_manager.write(target_id, data.as_bytes()) {
            return Response::err(format!("Failed to write to session: {}", e));
        }
        Response::ok()
    } else {
        Response::err("session_id required")
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_under_config() {
        let path = socket_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains(".config/roux/roux.sock"),
            "Expected .config/roux/roux.sock, got {}",
            path_str
        );
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
        assert_eq!(req.args["direction"], "horizontal");
    }

    #[test]
    fn request_default_args_is_null() {
        let json = r#"{"command": "status"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(req.args.is_null());
    }

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
