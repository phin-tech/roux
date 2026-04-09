use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use crate::AppState;

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
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("roux").join("roux.sock")
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
        "session-create" => handle_session_create(req, app),
        "shell" => handle_shell(req, app),
        "focus" => handle_focus(req, app),
        "run" => handle_run(req, app),
        "send" => handle_send(req, app),
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
    let _ = app.emit(
        "roux-command",
        serde_json::json!({
            "action": "split",
            "sessionId": session_id,
            "paneId": req.pane_id,
            "direction": direction,
        }),
    );

    Response::ok()
}

fn handle_session_create(req: Request, app: &tauri::AppHandle) -> Response {
    let state: tauri::State<AppState> = app.state();

    let name = req.args.get("name").and_then(|n| n.as_str()).unwrap_or("New Session").to_string();

    let working_dir = req.args.get("working_dir").and_then(|d| d.as_str()).map(|s| s.to_string());

    // Use the working_dir as repo_path, or fall back to current session's repo
    let repo_path = match working_dir {
        Some(ref dir) => dir.clone(),
        None => {
            // Try to get repo path from the requesting session
            match req.session_id.as_deref().and_then(|id| state.session_store.get(id)) {
                Some(session) => session.repo_root.clone(),
                None => return Response::err("working_dir or session_id required"),
            }
        }
    };

    let settings = state.settings.lock().unwrap().clone();
    let session_id = uuid::Uuid::new_v4().to_string();
    let work_dir = working_dir.unwrap_or_else(|| repo_path.clone());

    let all_flags = settings.additional_flags.clone();

    let spawn_result = state.pty_manager.spawn(
        &session_id,
        &work_dir,
        settings.default_model.as_deref(),
        &all_flags,
        None,
        settings.claude_binary_path.as_deref(),
        app.clone(),
    );

    if let Err(e) = spawn_result {
        return Response::err(format!("Failed to spawn session: {}", e));
    }

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    let is_git = crate::is_git_repo(&work_dir);
    let branch = crate::get_current_branch(&work_dir).unwrap_or_else(|| "main".to_string());

    let session = crate::session::Session {
        id: session_id.clone(),
        name,
        repo_root: repo_path,
        worktree_path: work_dir,
        branch,
        is_worktree: false,
        status: "idle".to_string(),
        model: None,
        cost: None,
        created_at: now,
        project_id: None,
        is_git_repo: is_git,
    };

    state.session_store.add(session);

    // Tell frontend about the new session
    use tauri::Emitter;
    let _ = app.emit(
        "roux-command",
        serde_json::json!({
            "action": "session-created",
            "sessionId": session_id,
        }),
    );

    Response::success(serde_json::json!({ "session_id": session_id }))
}

fn handle_shell(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id,
        None => return Response::err("session_id required"),
    };

    let state: tauri::State<AppState> = app.state();

    let working_dir = req
        .args
        .get("working_dir")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .or_else(|| state.session_store.get(session_id).map(|s| s.worktree_path.clone()));

    let working_dir = match working_dir {
        Some(dir) => dir,
        None => return Response::err("could not determine working directory"),
    };

    let pane_id = crypto_random_uuid();
    let pty_id = crypto_random_uuid();

    if let Err(e) =
        state.pty_manager.spawn_shell(&pty_id, &working_dir, Some(session_id), app.clone())
    {
        return Response::err(format!("Failed to spawn shell: {}", e));
    }

    use tauri::Emitter;
    let _ = app.emit(
        "roux-command",
        serde_json::json!({
            "action": "shell-opened",
            "sessionId": session_id,
            "paneId": pane_id,
            "ptyId": pty_id,
        }),
    );

    Response::success(serde_json::json!({ "pane_id": pane_id, "pty_id": pty_id }))
}

fn handle_focus(req: Request, app: &tauri::AppHandle) -> Response {
    use tauri::Emitter;
    let _ = app.emit(
        "roux-command",
        serde_json::json!({
            "action": "focus",
            "sessionId": req.session_id,
            "paneId": req.pane_id,
        }),
    );

    Response::ok()
}

fn handle_run(req: Request, app: &tauri::AppHandle) -> Response {
    let session_id = match req.session_id.as_deref() {
        Some(id) => id,
        None => return Response::err("session_id required"),
    };

    let command = match req.args.get("command").and_then(|c| c.as_str()) {
        Some(c) => c.to_string(),
        None => return Response::err("command argument required"),
    };

    let state: tauri::State<AppState> = app.state();

    let working_dir = req
        .args
        .get("working_dir")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .or_else(|| state.session_store.get(session_id).map(|s| s.worktree_path.clone()));

    let working_dir = match working_dir {
        Some(dir) => dir,
        None => return Response::err("could not determine working directory"),
    };

    let pane_id = format!("cmd-{}", crypto_random_uuid());
    let pty_id = format!(
        "{}-{}",
        pane_id,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
    );

    if let Err(e) =
        state.pty_manager.spawn_task(&pty_id, &command, &working_dir, Some(session_id), app.clone())
    {
        return Response::err(format!("Failed to spawn task: {}", e));
    }

    use tauri::Emitter;
    let _ = app.emit(
        "roux-command",
        serde_json::json!({
            "action": "command-opened",
            "sessionId": session_id,
            "paneId": pane_id,
            "ptyId": pty_id,
            "command": command,
            "workingDir": working_dir,
        }),
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
