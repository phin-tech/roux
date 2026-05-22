use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::ipc::{Channel, Response as IpcResponse};
use tauri::{AppHandle, Emitter};

use roux_core::{Project, Session, SessionExitPayload, SessionExitReason};
use roux_runtime::process_service::{ProcessRecord, ProcessSnapshot};
use roux_runtime::pty_service::{PtyRecord, PtySnapshot};
use roux_runtime::terminal_env::NotesEnvInputs;

use crate::platform;

const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonStatus {
    pub(crate) kind: String,
    pub(crate) pid: u32,
    pub(crate) socket: String,
    #[serde(default)]
    pub(crate) log_path: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) uptime_ms: u64,
    pub(crate) session_count: usize,
    pub(crate) project_count: usize,
    #[serde(default)]
    pub(crate) process_count: usize,
    #[serde(default)]
    pub(crate) pty_count: usize,
    pub(crate) capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonClient {
    status: DaemonStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonCreateSessionShellRequest {
    pub(crate) id: String,
    pub(crate) repo_path: String,
    pub(crate) name: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) base: Option<String>,
    pub(crate) fetch_first: bool,
    pub(crate) profile: Option<String>,
    pub(crate) initial_size: Option<(u16, u16)>,
    pub(crate) project_id: Option<String>,
    pub(crate) blueprint_id: Option<String>,
    pub(crate) smol_machine_name: Option<String>,
    pub(crate) notes: Option<NotesEnvInputs>,
}

impl DaemonClient {
    pub(crate) fn detect() -> Option<Self> {
        let data =
            send_command_blocking(serde_json::json!({ "command": "daemon-status" }), PROBE_TIMEOUT)
                .ok()?;
        let status: DaemonStatus = serde_json::from_value(data).ok()?;
        if status.kind == "roux-daemon" {
            Some(Self { status })
        } else {
            None
        }
    }

    pub(crate) fn ensure_local() -> Option<Self> {
        if let Some(client) = Self::detect() {
            return Some(client);
        }

        match launch_local_daemon() {
            Ok(started) => {
                rlog!("Started roux daemon pid={} from {}", started.pid, started.binary.display());
            }
            Err(err) => {
                rlog!("Unable to start roux daemon; desktop will self-host runtime state: {err}");
                return None;
            }
        }

        match wait_for_daemon(STARTUP_TIMEOUT, STARTUP_POLL_INTERVAL) {
            Some(client) => Some(client),
            None => {
                rlog!(
                    "Started roux daemon but it did not become ready within {}ms; desktop will self-host runtime state",
                    STARTUP_TIMEOUT.as_millis()
                );
                None
            }
        }
    }

    pub(crate) fn status(&self) -> &DaemonStatus {
        &self.status
    }

    pub(crate) async fn refresh_status(&self) -> Result<DaemonStatus, String> {
        let value = send_command_async(serde_json::json!({ "command": "daemon-status" })).await?;
        let status: DaemonStatus =
            serde_json::from_value(value).map_err(|err| format!("decode daemon-status: {err}"))?;
        if status.kind == "roux-daemon" {
            Ok(status)
        } else {
            Err(format!("unexpected daemon kind: {}", status.kind))
        }
    }

    pub(crate) async fn list_sessions(&self) -> Result<Vec<Session>, String> {
        let value = send_command_async(serde_json::json!({ "command": "session-list" })).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon session-list: {err}"))
    }

    pub(crate) async fn list_projects(&self) -> Result<Vec<Project>, String> {
        let value = send_command_async(serde_json::json!({ "command": "project-list" })).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon project-list: {err}"))
    }

    pub(crate) async fn set_session_name_override(
        &self,
        session_id: String,
        name_override: Option<String>,
    ) -> Result<(), String> {
        let name = name_override.unwrap_or_default();
        let _ = send_command_async(serde_json::json!({
            "command": "session-rename",
            "session_id": session_id,
            "args": { "name": name },
        }))
        .await?;
        Ok(())
    }

    pub(crate) async fn create_session_shell(
        &self,
        request: DaemonCreateSessionShellRequest,
    ) -> Result<Session, String> {
        let value = send_command_async(daemon_session_create_shell_request(request)).await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon session-create-shell: {err}"))
    }

    pub(crate) async fn start_daemon_process(
        &self,
        command: String,
        working_dir: Option<String>,
    ) -> Result<ProcessRecord, String> {
        let value = send_command_async(daemon_process_start_request(command, working_dir)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process start: {err}"))
    }

    pub(crate) async fn daemon_process_output(
        &self,
        id: String,
        max_bytes: Option<usize>,
    ) -> Result<ProcessSnapshot, String> {
        let value = send_command_async(daemon_process_output_request(id, max_bytes)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process output: {err}"))
    }

    pub(crate) async fn list_daemon_processes(&self) -> Result<Vec<ProcessRecord>, String> {
        let value = send_command_async(daemon_process_list_request()).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process list: {err}"))
    }

    pub(crate) async fn kill_daemon_process(&self, id: String) -> Result<ProcessRecord, String> {
        let value = send_command_async(daemon_process_kill_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process kill: {err}"))
    }

    pub(crate) async fn spawn_daemon_pty_shell(
        &self,
        id: Option<String>,
        working_dir: Option<String>,
        session_id: Option<String>,
        pane_id: Option<String>,
        profile: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_spawn_shell_request(
            id,
            working_dir,
            session_id,
            pane_id,
            profile,
            initial_size,
        ))
        .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty spawn shell: {err}"))
    }

    pub(crate) async fn spawn_daemon_pty_task(
        &self,
        command: String,
        id: Option<String>,
        working_dir: Option<String>,
        session_id: Option<String>,
        pane_id: Option<String>,
        profile: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_spawn_task_request(
            command,
            id,
            working_dir,
            session_id,
            pane_id,
            profile,
            initial_size,
        ))
        .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty spawn task: {err}"))
    }

    pub(crate) async fn daemon_pty_output(
        &self,
        id: String,
        max_bytes: Option<usize>,
    ) -> Result<PtySnapshot, String> {
        let value = send_command_async(daemon_pty_output_request(id, max_bytes)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty output: {err}"))
    }

    pub(crate) async fn list_daemon_ptys(&self) -> Result<Vec<PtyRecord>, String> {
        let value = send_command_async(daemon_pty_list_request()).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty list: {err}"))
    }

    pub(crate) async fn write_daemon_pty(&self, id: String, data: String) -> Result<(), String> {
        let _ = send_command_async(daemon_pty_write_request(id, data)).await?;
        Ok(())
    }

    pub(crate) async fn resize_daemon_pty(
        &self,
        id: String,
        cols: u16,
        rows: u16,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_resize_request(id, cols, rows)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty resize: {err}"))
    }

    pub(crate) async fn kill_daemon_pty(&self, id: String) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_kill_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty kill: {err}"))
    }

    pub(crate) async fn detach_daemon_pty(&self, id: String) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_detach_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty detach: {err}"))
    }

    pub(crate) async fn attach_daemon_pty_to_pane(
        &self,
        id: String,
        pane_id: String,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_attach_pane_request(id, pane_id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty attach pane: {err}"))
    }

    pub(crate) async fn mark_daemon_pty_read(&self, id: String) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_mark_read_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty mark read: {err}"))
    }

    pub(crate) async fn set_daemon_pty_name(
        &self,
        id: String,
        name: Option<String>,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_set_name_request(id, name)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty set name: {err}"))
    }

    pub(crate) fn spawn_daemon_pty_output_bridge(
        &self,
        id: String,
        channel: Channel<IpcResponse>,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(err) = attach_daemon_pty_output_blocking(id.clone(), channel, app) {
                rlog!("Daemon PTY output bridge for {id} stopped: {err}");
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartedDaemon {
    binary: PathBuf,
    pid: u32,
}

fn launch_local_daemon() -> Result<StartedDaemon, String> {
    if let Some(reason) = daemon_autostart_disabled_reason() {
        return Err(reason);
    }
    let binary = resolve_daemon_binary()?;
    let mut child = daemon_spawn_command(&binary)
        .spawn()
        .map_err(|err| format!("spawn {} daemon: {err}", binary.display()))?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(StartedDaemon { binary, pid })
}

fn wait_for_daemon(timeout: Duration, interval: Duration) -> Option<DaemonClient> {
    let started = std::time::Instant::now();
    loop {
        if let Some(client) = DaemonClient::detect() {
            return Some(client);
        }
        if started.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(interval);
    }
}

fn daemon_autostart_disabled_reason() -> Option<String> {
    daemon_autostart_disabled_reason_for(
        std::env::var("ROUX_DAEMON_AUTOSTART").ok().as_deref(),
        std::env::var("ROUX_SOCKET").ok().as_deref(),
    )
}

fn daemon_autostart_disabled_reason_for(
    autostart: Option<&str>,
    socket: Option<&str>,
) -> Option<String> {
    if autostart.and_then(parse_env_enabled) == Some(false) {
        return Some("ROUX_DAEMON_AUTOSTART disabled local daemon startup".to_string());
    }
    if socket.map(|value| !value.trim().is_empty()).unwrap_or(false) {
        return Some("ROUX_SOCKET is set, assuming external daemon endpoint".to_string());
    }
    None
}

fn parse_env_enabled(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "0" | "false" | "no" | "off" => Some(false),
        _ => Some(true),
    }
}

fn resolve_daemon_binary() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().ok();
    resolve_daemon_binary_from(current_exe.as_deref()).ok_or_else(|| {
        format!(
            "{} not found next to the desktop binary or on PATH",
            platform::roux_cli_file_name()
        )
    })
}

fn resolve_daemon_binary_from(current_exe: Option<&Path>) -> Option<PathBuf> {
    current_exe
        .and_then(platform::sibling_roux_cli_path)
        .filter(|path| path.is_file())
        .or_else(|| platform::find_executable_on_path(platform::roux_cli_file_name()))
}

fn daemon_spawn_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    command.arg("daemon").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    command
}

fn daemon_process_start_request(command: String, working_dir: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("command".to_string(), Value::String(command));
    if let Some(working_dir) = working_dir {
        args.insert("workingDir".to_string(), Value::String(working_dir));
    }
    serde_json::json!({
        "command": "daemon-process-start",
        "args": args,
    })
}

fn daemon_session_create_shell_request(request: DaemonCreateSessionShellRequest) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(request.id));
    args.insert("repoPath".to_string(), Value::String(request.repo_path));
    args.insert("name".to_string(), Value::String(request.name));
    if let Some(worktree_path) = request.worktree_path {
        args.insert("worktreePath".to_string(), Value::String(worktree_path));
    }
    if let Some(branch) = request.branch {
        args.insert("branch".to_string(), Value::String(branch));
    }
    if let Some(base) = request.base {
        args.insert("base".to_string(), Value::String(base));
    }
    if request.fetch_first {
        args.insert("fetchFirst".to_string(), Value::Bool(true));
    }
    if let Some(profile) = request.profile {
        args.insert("profile".to_string(), Value::String(profile));
    }
    if let Some((cols, rows)) = request.initial_size {
        args.insert("initialSize".to_string(), serde_json::json!([cols, rows]));
    }
    if let Some(project_id) = request.project_id {
        args.insert("projectId".to_string(), Value::String(project_id));
    }
    if let Some(blueprint_id) = request.blueprint_id {
        args.insert("blueprintId".to_string(), Value::String(blueprint_id));
    }
    if let Some(smol_machine_name) = request.smol_machine_name {
        args.insert("smolMachineName".to_string(), Value::String(smol_machine_name));
    }
    if let Some(notes) = request.notes {
        args.insert(
            "notesEnv".to_string(),
            serde_json::json!({
                "vaultRoot": notes.vault_root,
                "sessionSlug": notes.session_slug,
                "repoSlug": notes.repo_slug,
                "projectSlug": notes.project_slug,
                "contextPaths": notes.context_paths,
                "projectPrompt": notes.project_prompt,
            }),
        );
    }
    serde_json::json!({
        "command": "session-create-shell",
        "args": args,
    })
}

fn daemon_process_output_request(id: String, max_bytes: Option<usize>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(id));
    if let Some(max_bytes) = max_bytes {
        args.insert("maxBytes".to_string(), serde_json::json!(max_bytes));
    }
    serde_json::json!({
        "command": "daemon-process-output",
        "args": args,
    })
}

fn daemon_process_list_request() -> Value {
    serde_json::json!({ "command": "daemon-process-list" })
}

fn daemon_process_kill_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-process-kill",
        "args": { "id": id },
    })
}

fn daemon_pty_spawn_shell_request(
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> Value {
    serde_json::json!({
        "command": "daemon-pty-spawn-shell",
        "args": daemon_pty_spawn_args(id, working_dir, session_id, pane_id, profile, initial_size),
    })
}

fn daemon_pty_spawn_task_request(
    command: String,
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> Value {
    let mut args =
        daemon_pty_spawn_args(id, working_dir, session_id, pane_id, profile, initial_size);
    args.insert("command".to_string(), Value::String(command));
    serde_json::json!({
        "command": "daemon-pty-spawn-task",
        "args": args,
    })
}

fn daemon_pty_spawn_args(
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> serde_json::Map<String, Value> {
    let mut args = serde_json::Map::new();
    if let Some(id) = id {
        args.insert("id".to_string(), Value::String(id));
    }
    if let Some(working_dir) = working_dir {
        args.insert("workingDir".to_string(), Value::String(working_dir));
    }
    if let Some(session_id) = session_id {
        args.insert("sessionId".to_string(), Value::String(session_id));
    }
    if let Some(pane_id) = pane_id {
        args.insert("paneId".to_string(), Value::String(pane_id));
    }
    if let Some(profile) = profile {
        args.insert("profile".to_string(), Value::String(profile));
    }
    if let Some((cols, rows)) = initial_size {
        args.insert("initialSize".to_string(), serde_json::json!([cols, rows]));
    }
    args
}

fn daemon_pty_output_request(id: String, max_bytes: Option<usize>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(id));
    if let Some(max_bytes) = max_bytes {
        args.insert("maxBytes".to_string(), serde_json::json!(max_bytes));
    }
    serde_json::json!({
        "command": "daemon-pty-output",
        "args": args,
    })
}

fn daemon_pty_attach_request(id: String, max_bytes: Option<usize>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(id));
    if let Some(max_bytes) = max_bytes {
        args.insert("maxBytes".to_string(), serde_json::json!(max_bytes));
    }
    serde_json::json!({
        "command": "daemon-pty-attach",
        "args": args,
    })
}

fn daemon_pty_list_request() -> Value {
    serde_json::json!({ "command": "daemon-pty-list" })
}

fn daemon_pty_write_request(id: String, data: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-write",
        "args": { "id": id, "data": data },
    })
}

fn daemon_pty_resize_request(id: String, cols: u16, rows: u16) -> Value {
    serde_json::json!({
        "command": "daemon-pty-resize",
        "args": { "id": id, "cols": cols, "rows": rows },
    })
}

fn daemon_pty_detach_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-detach",
        "args": { "id": id },
    })
}

fn daemon_pty_attach_pane_request(id: String, pane_id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-attach-pane",
        "args": { "id": id, "paneId": pane_id },
    })
}

fn daemon_pty_mark_read_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-mark-read",
        "args": { "id": id },
    })
}

fn daemon_pty_set_name_request(id: String, name: Option<String>) -> Value {
    serde_json::json!({
        "command": "daemon-pty-set-name",
        "args": { "id": id, "name": name },
    })
}

fn daemon_pty_kill_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-kill",
        "args": { "id": id },
    })
}

async fn send_command_async(request: Value) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || send_command_blocking(request, COMMAND_TIMEOUT))
        .await
        .map_err(|err| format!("daemon client task failed: {err}"))?
}

#[derive(Debug, Deserialize)]
struct Response {
    ok: bool,
    data: Option<Value>,
    error: Option<String>,
}

fn decode_response(raw: &str) -> Result<Value, String> {
    let response: Response = serde_json::from_str(raw.trim())
        .map_err(|err| format!("invalid daemon response: {err}"))?;
    if response.ok {
        Ok(response.data.unwrap_or(Value::Null))
    } else {
        Err(response.error.unwrap_or_else(|| "daemon command failed".to_string()))
    }
}

fn send_command_blocking(request: Value, timeout: Duration) -> Result<Value, String> {
    #[cfg(windows)]
    {
        send_tcp_command(request, timeout)
    }
    #[cfg(not(windows))]
    {
        send_unix_command(request, timeout)
    }
}

#[cfg(not(windows))]
fn send_unix_command(request: Value, timeout: Duration) -> Result<Value, String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("set daemon read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;

    write_request(&mut stream, request)?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|err| format!("read daemon response: {err}"))?;
    decode_response(&raw)
}

#[cfg(windows)]
fn send_tcp_command(mut request: Value, timeout: Duration) -> Result<Value, String> {
    use std::net::{Shutdown, TcpStream};

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("set daemon read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;

    write_request(&mut stream, request)?;
    let _ = stream.shutdown(Shutdown::Write);
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|err| format!("read daemon response: {err}"))?;
    decode_response(&raw)
}

fn write_request(stream: &mut impl Write, request: Value) -> Result<(), String> {
    let json = serde_json::to_string(&request).map_err(|err| format!("encode request: {err}"))?;
    stream
        .write_all(json.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|err| format!("write daemon request: {err}"))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum PtyAttachStreamFrame {
    #[serde(rename = "ready")]
    Ready {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        record: PtyRecord,
        #[serde(rename = "replayOffset")]
        replay_offset: u64,
        #[serde(rename = "replayBytes")]
        replay_bytes: Vec<u8>,
    },
    #[serde(rename = "output")]
    Output { offset: u64, bytes: Vec<u8> },
    #[serde(rename = "exit")]
    Exit { code: Option<i32>, generation: u64 },
    #[serde(rename = "error")]
    Error { error: String },
}

fn attach_daemon_pty_output_blocking(
    id: String,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        attach_daemon_pty_output_tcp(id, channel, app)
    }
    #[cfg(not(windows))]
    {
        attach_daemon_pty_output_unix(id, channel, app)
    }
}

#[cfg(not(windows))]
fn attach_daemon_pty_output_unix(
    id: String,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(
        &mut stream,
        daemon_pty_attach_request(
            id.clone(),
            Some(roux_runtime::pty_service::PTY_OUTPUT_LIMIT_BYTES),
        ),
    )?;
    read_pty_attach_stream(id, stream, channel, app)
}

#[cfg(windows)]
fn attach_daemon_pty_output_tcp(
    id: String,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    use std::net::TcpStream;

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    let mut request = daemon_pty_attach_request(
        id.clone(),
        Some(roux_runtime::pty_service::PTY_OUTPUT_LIMIT_BYTES),
    );
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, request)?;
    read_pty_attach_stream(id, stream, channel, app)
}

fn read_pty_attach_stream(
    id: String,
    stream: impl Read,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut sent_until = 0_u64;
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| format!("read daemon pty attach frame: {err}"))?;
        if read == 0 {
            return Ok(());
        }
        let frame: PtyAttachStreamFrame = serde_json::from_str(line.trim())
            .map_err(|err| format!("decode daemon pty attach frame: {err}"))?;
        if !handle_pty_attach_frame(&id, frame, &channel, &app, &mut sent_until)? {
            return Ok(());
        }
    }
}

fn handle_pty_attach_frame(
    id: &str,
    frame: PtyAttachStreamFrame,
    channel: &Channel<IpcResponse>,
    app: &AppHandle,
    sent_until: &mut u64,
) -> Result<bool, String> {
    match frame {
        PtyAttachStreamFrame::Ready { replay_offset, replay_bytes, .. } => {
            let replay_end = replay_offset.saturating_add(replay_bytes.len() as u64);
            if !replay_bytes.is_empty() {
                channel
                    .send(IpcResponse::new(replay_bytes))
                    .map_err(|err| format!("send daemon pty replay to frontend: {err}"))?;
            }
            *sent_until = (*sent_until).max(replay_end);
            Ok(true)
        }
        PtyAttachStreamFrame::Output { offset, bytes } => {
            let frame_end = offset.saturating_add(bytes.len() as u64);
            if frame_end <= *sent_until {
                return Ok(true);
            }
            let start = if offset < *sent_until { (*sent_until - offset) as usize } else { 0 };
            let bytes = bytes[start..].to_vec();
            if !bytes.is_empty() {
                channel
                    .send(IpcResponse::new(bytes))
                    .map_err(|err| format!("send daemon pty output to frontend: {err}"))?;
            }
            *sent_until = (*sent_until).max(frame_end);
            Ok(true)
        }
        PtyAttachStreamFrame::Exit { code, generation } => {
            emit_daemon_pty_exit(app, id, code, generation);
            Ok(false)
        }
        PtyAttachStreamFrame::Error { error } => Err(error),
    }
}

fn emit_daemon_pty_exit(app: &AppHandle, id: &str, code: Option<i32>, generation: u64) {
    let code = code.and_then(|code| u32::try_from(code).ok());
    let _ = app.emit(
        &format!("session-exit:{id}"),
        &SessionExitPayload { code, generation, reason: SessionExitReason::Exit },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_response_returns_data_on_success() {
        let data = decode_response(r#"{"ok":true,"data":{"kind":"roux-daemon"}}"#).unwrap();
        assert_eq!(data["kind"], "roux-daemon");
    }

    #[test]
    fn decode_response_returns_error_message() {
        let err = decode_response(r#"{"ok":false,"error":"nope"}"#).unwrap_err();
        assert_eq!(err, "nope");
    }

    #[test]
    fn daemon_process_start_request_uses_daemon_command_shape() {
        let request =
            daemon_process_start_request("printf hi".to_string(), Some("/tmp".to_string()));

        assert_eq!(request["command"], "daemon-process-start");
        assert_eq!(request["args"]["command"], "printf hi");
        assert_eq!(request["args"]["workingDir"], "/tmp");
    }

    #[test]
    fn daemon_session_create_shell_request_uses_daemon_command_shape() {
        let request = daemon_session_create_shell_request(DaemonCreateSessionShellRequest {
            id: "session-a".to_string(),
            repo_path: "/repo".to_string(),
            name: "Daemon Session".to_string(),
            worktree_path: None,
            branch: Some("feature/demo".to_string()),
            base: Some("origin/main".to_string()),
            fetch_first: true,
            profile: Some("plain-shell".to_string()),
            initial_size: Some((100, 30)),
            project_id: Some("project-a".to_string()),
            blueprint_id: Some("blueprint-a".to_string()),
            smol_machine_name: None,
            notes: Some(NotesEnvInputs {
                vault_root: "/vault".to_string(),
                session_slug: "feature-demo--sessio".to_string(),
                repo_slug: "repo-a".to_string(),
                project_slug: Some("project-a".to_string()),
                context_paths: vec!["/repo/docs".to_string()],
                project_prompt: "Use project notes".to_string(),
            }),
        });

        assert_eq!(request["command"], "session-create-shell");
        assert_eq!(request["args"]["id"], "session-a");
        assert_eq!(request["args"]["repoPath"], "/repo");
        assert_eq!(request["args"]["branch"], "feature/demo");
        assert_eq!(request["args"]["base"], "origin/main");
        assert_eq!(request["args"]["fetchFirst"], true);
        assert_eq!(request["args"]["profile"], "plain-shell");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([100, 30]));
        assert_eq!(request["args"]["projectId"], "project-a");
        assert_eq!(request["args"]["notesEnv"]["vaultRoot"], "/vault");
        assert_eq!(request["args"]["notesEnv"]["contextPaths"][0], "/repo/docs");
    }

    #[test]
    fn daemon_process_output_request_uses_max_bytes() {
        let request = daemon_process_output_request("daemon-process-1".to_string(), Some(42));

        assert_eq!(request["command"], "daemon-process-output");
        assert_eq!(request["args"]["id"], "daemon-process-1");
        assert_eq!(request["args"]["maxBytes"], 42);
    }

    #[test]
    fn daemon_process_list_and_kill_requests_use_daemon_commands() {
        assert_eq!(daemon_process_list_request()["command"], "daemon-process-list");

        let kill = daemon_process_kill_request("daemon-process-1".to_string());
        assert_eq!(kill["command"], "daemon-process-kill");
        assert_eq!(kill["args"]["id"], "daemon-process-1");
    }

    #[test]
    fn daemon_pty_spawn_task_request_uses_daemon_command_shape() {
        let request = daemon_pty_spawn_task_request(
            "printf hi".to_string(),
            Some("pty-a".to_string()),
            Some("/tmp".to_string()),
            Some("session-a".to_string()),
            Some("pane-a".to_string()),
            Some("task".to_string()),
            Some((120, 40)),
        );

        assert_eq!(request["command"], "daemon-pty-spawn-task");
        assert_eq!(request["args"]["command"], "printf hi");
        assert_eq!(request["args"]["id"], "pty-a");
        assert_eq!(request["args"]["workingDir"], "/tmp");
        assert_eq!(request["args"]["sessionId"], "session-a");
        assert_eq!(request["args"]["paneId"], "pane-a");
        assert_eq!(request["args"]["profile"], "task");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([120, 40]));
    }

    #[test]
    fn daemon_pty_control_requests_use_daemon_commands() {
        let output = daemon_pty_output_request("pty-a".to_string(), Some(42));
        assert_eq!(output["command"], "daemon-pty-output");
        assert_eq!(output["args"]["id"], "pty-a");
        assert_eq!(output["args"]["maxBytes"], 42);

        let attach = daemon_pty_attach_request("pty-a".to_string(), Some(1024));
        assert_eq!(attach["command"], "daemon-pty-attach");
        assert_eq!(attach["args"]["id"], "pty-a");
        assert_eq!(attach["args"]["maxBytes"], 1024);

        assert_eq!(daemon_pty_list_request()["command"], "daemon-pty-list");

        let write = daemon_pty_write_request("pty-a".to_string(), "input\n".to_string());
        assert_eq!(write["command"], "daemon-pty-write");
        assert_eq!(write["args"]["data"], "input\n");

        let resize = daemon_pty_resize_request("pty-a".to_string(), 100, 30);
        assert_eq!(resize["command"], "daemon-pty-resize");
        assert_eq!(resize["args"]["cols"], 100);
        assert_eq!(resize["args"]["rows"], 30);

        let detach = daemon_pty_detach_request("pty-a".to_string());
        assert_eq!(detach["command"], "daemon-pty-detach");
        assert_eq!(detach["args"]["id"], "pty-a");

        let attach_pane = daemon_pty_attach_pane_request("pty-a".to_string(), "pane-b".to_string());
        assert_eq!(attach_pane["command"], "daemon-pty-attach-pane");
        assert_eq!(attach_pane["args"]["paneId"], "pane-b");

        let mark_read = daemon_pty_mark_read_request("pty-a".to_string());
        assert_eq!(mark_read["command"], "daemon-pty-mark-read");

        let set_name = daemon_pty_set_name_request("pty-a".to_string(), Some("Build".to_string()));
        assert_eq!(set_name["command"], "daemon-pty-set-name");
        assert_eq!(set_name["args"]["name"], "Build");
        let clear_name = daemon_pty_set_name_request("pty-a".to_string(), None);
        assert!(clear_name["args"]["name"].is_null());

        let kill = daemon_pty_kill_request("pty-a".to_string());
        assert_eq!(kill["command"], "daemon-pty-kill");
        assert_eq!(kill["args"]["id"], "pty-a");
    }

    #[test]
    fn daemon_autostart_policy_respects_external_endpoint_and_opt_out() {
        assert_eq!(daemon_autostart_disabled_reason_for(None, None), None);
        assert_eq!(daemon_autostart_disabled_reason_for(Some("1"), Some("")), None);

        assert!(daemon_autostart_disabled_reason_for(Some("0"), None)
            .unwrap()
            .contains("ROUX_DAEMON_AUTOSTART"));
        assert!(daemon_autostart_disabled_reason_for(None, Some("/tmp/remote.sock"))
            .unwrap()
            .contains("ROUX_SOCKET"));
    }

    #[test]
    fn parse_env_enabled_accepts_common_false_values() {
        assert_eq!(parse_env_enabled("0"), Some(false));
        assert_eq!(parse_env_enabled("false"), Some(false));
        assert_eq!(parse_env_enabled("off"), Some(false));
        assert_eq!(parse_env_enabled("yes"), Some(true));
        assert_eq!(parse_env_enabled(""), None);
    }

    #[test]
    fn resolve_daemon_binary_prefers_sibling_cli() {
        let dir = tempfile::tempdir().unwrap();
        let desktop = dir.path().join("roux-desktop");
        let cli = dir.path().join(platform::roux_cli_file_name());
        std::fs::write(&desktop, "").unwrap();
        std::fs::write(&cli, "").unwrap();

        assert_eq!(resolve_daemon_binary_from(Some(&desktop)), Some(cli));
    }
}
