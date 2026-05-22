use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(windows)]
use tokio::net::TcpListener;
#[cfg(not(windows))]
use tokio::net::UnixListener;

use roux_runtime::host::{RuntimeHost, RuntimeHostConfig};
use roux_runtime::process_service::PROCESS_OUTPUT_DEFAULT_POLL_BYTES;

use crate::{daemon_log::DaemonLog, paths, platform};

pub async fn run() -> Result<(), String> {
    paths::migrate_legacy_config_dir();
    let log = DaemonLog::init();

    let project_path = platform::projects_path();
    let session_path = platform::sessions_path();
    let projects = roux_runtime::project_service::load_persisted_from(&project_path);
    let sessions = roux_runtime::session_service::load_persisted_from(&session_path, &projects);
    log.write(&format!(
        "Loaded {} project(s) from {} and {} session(s) from {}",
        projects.len(),
        project_path.display(),
        sessions.len(),
        session_path.display()
    ));

    let services = RuntimeHostConfig {
        initial_sessions: sessions,
        session_persist_path: session_path,
        initial_projects: projects,
        project_persist_path: project_path,
    }
    .build();

    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity =
        DaemonIdentity::new(daemon_socket_path(), log.path().clone(), daemon_auth_token());
    let socket_server = start_socket_server(host.clone(), identity.clone(), log.clone()).await?;
    log.write(&format!("Started on {}; press Ctrl-C to stop", identity.socket.display()));

    wait_for_shutdown_signal().await?;
    log.write("Shutdown signal received");

    socket_server.shutdown();
    log.write("Socket server stopped");
    host.process_handle.shutdown().await;
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

#[derive(Debug, Clone)]
struct DaemonIdentity {
    started_at_ms: u64,
    socket: PathBuf,
    log_path: PathBuf,
    #[cfg_attr(not(windows), allow(dead_code))]
    auth_token: Option<String>,
}

impl DaemonIdentity {
    fn new(socket: PathBuf, log_path: PathBuf, auth_token: Option<String>) -> Self {
        Self { started_at_ms: unix_now_ms(), socket, log_path, auth_token }
    }

    #[cfg(test)]
    fn new_for_test(socket: impl Into<PathBuf>) -> Self {
        Self {
            started_at_ms: 1_000,
            socket: socket.into(),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Request {
    command: String,
    session_id: Option<String>,
    #[allow(dead_code)]
    pane_id: Option<String>,
    #[allow(dead_code)]
    auth_token: Option<String>,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn success(data: Value) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }
}

struct SocketServerHandle {
    join: tokio::task::JoinHandle<()>,
    cleanup: SocketCleanup,
}

impl SocketServerHandle {
    fn shutdown(self) {
        self.join.abort();
        self.cleanup.remove();
    }
}

struct SocketCleanup {
    #[cfg(not(windows))]
    socket: PathBuf,
    #[cfg(windows)]
    endpoint_file: PathBuf,
    #[cfg(windows)]
    token_file: PathBuf,
}

impl SocketCleanup {
    fn remove(self) {
        #[cfg(not(windows))]
        {
            let _ = std::fs::remove_file(self.socket);
        }
        #[cfg(windows)]
        {
            let _ = std::fs::remove_file(self.endpoint_file);
            let _ = std::fs::remove_file(self.token_file);
        }
    }
}

async fn start_socket_server(
    host: RuntimeHost,
    identity: DaemonIdentity,
    log: DaemonLog,
) -> Result<SocketServerHandle, String> {
    #[cfg(not(windows))]
    {
        let listener = bind_unix_listener(&identity.socket)?;
        log.write(&format!("Socket server listening on {}", identity.socket.display()));
        let socket = identity.socket.clone();
        let join = tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(err) => {
                        log.write(&format!("Socket accept failed: {err}"));
                        continue;
                    }
                };
                let host = host.clone();
                let identity = identity.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let mut reader = BufReader::new(reader);
                    handle_connection(&mut reader, &mut writer, &host, &identity, &log).await;
                });
            }
        });
        Ok(SocketServerHandle { join, cleanup: SocketCleanup { socket } })
    }

    #[cfg(windows)]
    {
        let listener = bind_windows_listener(&identity).await?;
        log.write(&format!("Socket server listening on {}", identity.socket.display()));
        let endpoint_file = platform::socket_addr_file_path();
        let token_file = platform::socket_auth_token_file_path();
        let join = tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(err) => {
                        log.write(&format!("Socket accept failed: {err}"));
                        continue;
                    }
                };
                let host = host.clone();
                let identity = identity.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let mut reader = BufReader::new(reader);
                    handle_connection(&mut reader, &mut writer, &host, &identity, &log).await;
                });
            }
        });
        Ok(SocketServerHandle { join, cleanup: SocketCleanup { endpoint_file, token_file } })
    }
}

#[cfg(not(windows))]
fn bind_unix_listener(path: &Path) -> Result<UnixListener, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create socket directory {}: {err}", parent.display()))?;
    }

    if path.exists() {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(_) => {
                return Err(format!("Roux command socket already active at {}", path.display()));
            }
            Err(_) => {
                use std::os::unix::fs::FileTypeExt;
                let metadata = std::fs::symlink_metadata(path)
                    .map_err(|err| format!("inspect socket path {}: {err}", path.display()))?;
                if !metadata.file_type().is_socket() {
                    return Err(format!("refusing to remove non-socket path {}", path.display()));
                }
                std::fs::remove_file(path)
                    .map_err(|err| format!("remove stale socket {}: {err}", path.display()))?;
            }
        }
    }

    let listener = UnixListener::bind(path)
        .map_err(|err| format!("bind daemon socket {}: {err}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(listener)
}

#[cfg(windows)]
async fn bind_windows_listener(identity: &DaemonIdentity) -> Result<TcpListener, String> {
    if let Some(parent) = platform::socket_addr_file_path().parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create socket directory {}: {err}", parent.display()))?;
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| format!("bind daemon socket on localhost: {err}"))?;
    let addr = listener
        .local_addr()
        .map_err(|err| format!("resolve daemon socket address: {err}"))?
        .to_string();
    std::fs::write(platform::socket_addr_file_path(), &addr)
        .map_err(|err| format!("write daemon socket endpoint: {err}"))?;
    let token = identity.auth_token.as_deref().unwrap_or_default();
    std::fs::write(platform::socket_auth_token_file_path(), token)
        .map_err(|err| format!("write daemon socket token: {err}"))?;
    Ok(listener)
}

async fn handle_connection<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
    log: &DaemonLog,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut line = String::new();
    let response = match reader.read_line(&mut line).await {
        Ok(0) => return,
        Ok(_) => match serde_json::from_str::<Request>(line.trim()) {
            Ok(req) => {
                let command = req.command.clone();
                let response = handle_request(req, host, identity).await;
                if response.ok {
                    log.write(&format!("Handled socket command: {command}"));
                } else {
                    let error = response.error.as_deref().unwrap_or("unknown error");
                    log.write(&format!("Socket command failed: {command}: {error}"));
                }
                response
            }
            Err(err) => {
                log.write(&format!("Invalid socket request: {err}"));
                Response::err(format!("Invalid request: {err}"))
            }
        },
        Err(err) => {
            log.write(&format!("Socket read failed: {err}"));
            Response::err(format!("Read failed: {err}"))
        }
    };

    let json = serde_json::to_string(&response).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
    let _ = writer.shutdown().await;
}

async fn handle_request(req: Request, host: &RuntimeHost, identity: &DaemonIdentity) -> Response {
    #[cfg(windows)]
    if req.auth_token.as_deref() != identity.auth_token.as_deref() {
        return Response::err("unauthorized");
    }

    match req.command.as_str() {
        "daemon-status" => handle_daemon_status(host, identity).await,
        "session-list" => handle_session_list(host).await,
        "session-poll" => handle_session_poll(req, host).await,
        "session-rename" => handle_session_rename(req, host).await,
        "project-list" => handle_project_list(host).await,
        "daemon-process-start" => handle_daemon_process_start(req, host).await,
        "daemon-process-output" => handle_daemon_process_output(req, host).await,
        "daemon-process-list" => handle_daemon_process_list(host).await,
        "daemon-process-kill" => handle_daemon_process_kill(req, host).await,
        _ => Response::err(format!("unknown daemon command: {}", req.command)),
    }
}

async fn handle_daemon_status(host: &RuntimeHost, identity: &DaemonIdentity) -> Response {
    let session_count = host.session_handle.list().await.map(|s| s.len()).unwrap_or(0);
    let project_count = host.project_handle.list().await.map(|p| p.len()).unwrap_or(0);
    let process_count = host.process_handle.list().await.map(|p| p.len()).unwrap_or(0);
    Response::success(serde_json::json!({
        "kind": "roux-daemon",
        "pid": std::process::id(),
        "socket": identity.socket.to_string_lossy(),
        "logPath": identity.log_path.to_string_lossy(),
        "startedAtMs": identity.started_at_ms,
        "uptimeMs": unix_now_ms().saturating_sub(identity.started_at_ms),
        "sessionCount": session_count,
        "projectCount": project_count,
        "processCount": process_count,
        "capabilities": [
            "daemon-status",
            "session-list",
            "session-poll",
            "session-rename",
            "project-list",
            "daemon-process-start",
            "daemon-process-output",
            "daemon-process-list",
            "daemon-process-kill"
        ],
    }))
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

async fn handle_project_list(host: &RuntimeHost) -> Response {
    match host.project_handle.list().await {
        Ok(projects) => match serde_json::to_value(&projects) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize projects: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
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

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn daemon_auth_token() -> Option<String> {
    #[cfg(windows)]
    {
        Some(format!("{}-{}", std::process::id(), unix_now_ms()))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn daemon_socket_path() -> PathBuf {
    platform::resolve_socket_endpoint().map(PathBuf::from).unwrap_or_else(platform::socket_path)
}

async fn wait_for_shutdown_signal() -> Result<(), String> {
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
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|err| format!("failed to wait for shutdown signal: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str) -> roux_core::Session {
        roux_core::Session {
            id: id.to_string(),
            name: format!("Session {id}"),
            repo_root: "/tmp/repo".to_string(),
            worktree_path: "/tmp/repo".to_string(),
            branch: "main".to_string(),
            is_worktree: false,
            status: roux_core::SessionStatus::Disconnected,
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
            smol_machine_name: None,
        }
    }

    #[tokio::test]
    async fn daemon_status_is_daemon_only_socket_command() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);

        let response = handle_request(
            Request {
                command: "daemon-status".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::Value::Null,
            },
            &host,
            &DaemonIdentity::new_for_test("/tmp/roux.sock"),
        )
        .await;

        assert!(response.ok);
        let data = response.data.expect("status payload");
        assert_eq!(data["kind"], "roux-daemon");
        assert_eq!(data["socket"], "/tmp/roux.sock");
        assert_eq!(data["logPath"], "/tmp/roux-daemon.log");
        assert_eq!(data["processCount"], 0);
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("daemon-status")));

        host.process_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_session_rename_mutates_runtime_state() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: vec![make_session("s1")],
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);

        let response = handle_request(
            Request {
                command: "session-rename".to_string(),
                session_id: Some("s1".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "name": "Daemon owned" }),
            },
            &host,
            &DaemonIdentity::new_for_test("/tmp/roux.sock"),
        )
        .await;

        assert!(response.ok);
        let session = host.session_handle.get("s1").await.unwrap().unwrap();
        assert_eq!(session.name_override.as_deref(), Some("Daemon owned"));

        host.process_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_process_start_and_output_poll_are_daemon_owned() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);

        let start = handle_request(
            Request {
                command: "daemon-process-start".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "command": "printf daemon-owned",
                    "workingDir": dir.path(),
                }),
            },
            &host,
            &DaemonIdentity::new_for_test("/tmp/roux.sock"),
        )
        .await;
        assert!(start.ok, "start failed: {:?}", start.error);
        let process_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let mut output = None;
        for _ in 0..50 {
            let poll = handle_request(
                Request {
                    command: "daemon-process-output".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::json!({ "id": process_id, "maxBytes": 1024 }),
                },
                &host,
                &DaemonIdentity::new_for_test("/tmp/roux.sock"),
            )
            .await;
            assert!(poll.ok, "poll failed: {:?}", poll.error);
            let data = poll.data.unwrap();
            if data["output"].as_str().unwrap_or("").contains("daemon-owned") {
                output = Some(data);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let output = output.expect("daemon-owned output should be pollable");
        assert_eq!(output["record"]["id"], process_id);
        assert_eq!(output["record"]["running"], false);
        assert_eq!(output["record"]["exitCode"], 0);

        host.process_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_socket_serves_status_request() {
        use tokio::io::AsyncReadExt;

        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("roux.sock");
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let log_path = dir.path().join("roux-daemon.log");
        let server = start_socket_server(
            host.clone(),
            DaemonIdentity::new(socket_path.clone(), log_path.clone(), None),
            DaemonLog::new_for_test(log_path.clone()),
        )
        .await
        .unwrap();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        stream.write_all(br#"{"command":"daemon-status"}"#).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        let value: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["kind"], "roux-daemon");
        let expected_log_path = log_path.to_string_lossy().to_string();
        assert_eq!(value["data"]["logPath"], serde_json::Value::String(expected_log_path));

        server.shutdown();
        host.process_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }
}
