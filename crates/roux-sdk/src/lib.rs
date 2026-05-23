//! Typed Rust SDK for Roux's daemon-first local API.

mod endpoint;
mod error;
mod protocol;
mod types;

pub mod blocking;

pub use endpoint::{parse_socket_endpoint, resolve_socket_endpoint, SocketEndpoint};
pub use error::{RouxError, RouxResult};
pub use protocol::{CommandRequest, CommandResponse};
pub use types::{DaemonStatus, PtyAttachFrame, PtyKind, PtyRecord, PtySnapshot};

use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Roux {
    endpoint: SocketEndpoint,
    auth_token: Option<String>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct RouxBuilder {
    endpoint: Option<SocketEndpoint>,
    auth_token: Option<String>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct Session {
    client: Roux,
    session: roux_core::Session,
}

#[derive(Debug, Clone)]
pub struct Pty {
    client: Roux,
    id: String,
}

#[derive(Debug, Clone)]
pub struct SpawnTask {
    client: Roux,
    command: String,
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
}

#[derive(Debug, Clone)]
pub struct SpawnShell {
    client: Roux,
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
}

impl Roux {
    pub fn builder() -> RouxBuilder {
        RouxBuilder::default()
    }

    pub fn connect() -> RouxResult<Self> {
        Self::builder().connect()
    }

    pub async fn status(&self) -> RouxResult<DaemonStatus> {
        self.command(CommandRequest::new("daemon-status")).await
    }

    pub async fn sessions(&self) -> RouxResult<Vec<roux_core::Session>> {
        self.command(CommandRequest::new("session-list")).await
    }

    pub async fn ptys(&self) -> RouxResult<Vec<PtyRecord>> {
        self.command(CommandRequest::new("daemon-pty-list")).await
    }

    pub async fn projects(&self) -> RouxResult<Vec<roux_core::Project>> {
        self.command(CommandRequest::new("project-list")).await
    }

    pub async fn watches(&self) -> RouxResult<Vec<roux_core::Watch>> {
        self.command(CommandRequest::new("watch-list")).await
    }

    pub fn session(&self, session: roux_core::Session) -> Session {
        Session { client: self.clone(), session }
    }

    pub fn pty(&self, id: impl Into<String>) -> Pty {
        Pty { client: self.clone(), id: id.into() }
    }

    pub fn spawn_task(&self, command: impl Into<String>) -> SpawnTask {
        SpawnTask {
            client: self.clone(),
            command: command.into(),
            id: None,
            working_dir: None,
            session_id: None,
            pane_id: None,
            profile: None,
            initial_size: None,
        }
    }

    pub fn spawn_shell(&self) -> SpawnShell {
        SpawnShell {
            client: self.clone(),
            id: None,
            working_dir: None,
            session_id: None,
            pane_id: None,
            profile: None,
            initial_size: None,
        }
    }

    pub async fn command<T>(&self, request: CommandRequest) -> RouxResult<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let client = self.clone();
        tokio::task::spawn_blocking(move || {
            let value = client.command_blocking(request)?;
            serde_json::from_value(value).map_err(RouxError::Decode)
        })
        .await
        .map_err(|err| RouxError::Transport(format!("SDK task join failed: {err}")))?
    }

    pub fn command_blocking(&self, request: CommandRequest) -> RouxResult<Value> {
        let response = blocking::send_request(
            &self.endpoint,
            self.auth_token.as_deref(),
            self.timeout,
            request,
        )?;
        response.into_result()
    }
}

impl Default for RouxBuilder {
    fn default() -> Self {
        Self { endpoint: None, auth_token: None, timeout: Duration::from_secs(5) }
    }
}

impl RouxBuilder {
    pub fn endpoint(mut self, endpoint: SocketEndpoint) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    pub fn auth_token(mut self, auth_token: impl Into<String>) -> Self {
        self.auth_token = Some(auth_token.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn connect(self) -> RouxResult<Roux> {
        let endpoint =
            self.endpoint.or_else(resolve_socket_endpoint).ok_or(RouxError::NotRunning)?;
        let auth_token = self.auth_token.or_else(endpoint::load_socket_auth_token);
        Ok(Roux { endpoint, auth_token, timeout: self.timeout })
    }
}

impl Session {
    pub fn id(&self) -> &str {
        &self.session.id
    }

    pub fn info(&self) -> &roux_core::Session {
        &self.session
    }

    pub async fn refresh(&mut self) -> RouxResult<&roux_core::Session> {
        let session: roux_core::Session =
            self.client.command(CommandRequest::new("session-poll").session_id(self.id())).await?;
        self.session = session;
        Ok(&self.session)
    }

    pub async fn send_text(&self, text: impl Into<String>, enter: bool) -> RouxResult<PtyWrite> {
        self.client
            .command(
                CommandRequest::new("send")
                    .session_id(self.id())
                    .args(serde_json::json!({ "text": text.into(), "enter": enter })),
            )
            .await
    }

    pub async fn latest_output(&self, max_bytes: usize) -> RouxResult<LatestOutput> {
        self.client
            .command(
                CommandRequest::new("latest-output")
                    .session_id(self.id())
                    .args(serde_json::json!({ "maxBytes": max_bytes })),
            )
            .await
    }

    pub async fn rename(&self, name: Option<String>) -> RouxResult<()> {
        let _: Value = self
            .client
            .command(
                CommandRequest::new("session-rename")
                    .session_id(self.id())
                    .args(serde_json::json!({ "name": name })),
            )
            .await?;
        Ok(())
    }

    pub async fn archive(&self) -> RouxResult<()> {
        let _: Value = self
            .client
            .command(CommandRequest::new("session-archive").session_id(self.id()))
            .await?;
        Ok(())
    }
}

impl Pty {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn snapshot(&self, max_bytes: usize) -> RouxResult<PtySnapshot> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-output")
                    .args(serde_json::json!({ "id": self.id, "maxBytes": max_bytes })),
            )
            .await
    }

    pub async fn write(&self, data: impl Into<String>) -> RouxResult<PtyWrite> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-write")
                    .args(serde_json::json!({ "id": self.id, "data": data.into() })),
            )
            .await
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> RouxResult<PtyRecord> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-resize")
                    .args(serde_json::json!({ "id": self.id, "cols": cols, "rows": rows })),
            )
            .await
    }

    pub async fn kill(&self) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-kill").args(serde_json::json!({ "id": self.id })),
            )
            .await
    }

    pub async fn detach(&self) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-detach").args(serde_json::json!({ "id": self.id })),
            )
            .await
    }

    pub async fn attach_to_pane(
        &self,
        pane_id: impl Into<String>,
    ) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-attach-pane")
                    .args(serde_json::json!({ "id": self.id, "paneId": pane_id.into() })),
            )
            .await
    }

    pub async fn mark_read(&self) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-mark-read")
                    .args(serde_json::json!({ "id": self.id })),
            )
            .await
    }

    pub async fn set_name(&self, name: Option<String>) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-set-name")
                    .args(serde_json::json!({ "id": self.id, "name": name })),
            )
            .await
    }

    pub async fn attach<F>(&self, max_replay_bytes: usize, mut on_frame: F) -> RouxResult<()>
    where
        F: FnMut(PtyAttachFrame) -> bool + Send + 'static,
    {
        let client = self.client.clone();
        let id = self.id.clone();
        tokio::task::spawn_blocking(move || {
            let mut parse_error = None;
            let result = blocking::stream_client_request(
                &client.endpoint,
                client.auth_token.as_deref(),
                CommandRequest::new("daemon-pty-attach")
                    .args(serde_json::json!({ "id": id, "maxBytes": max_replay_bytes })),
                |line| match serde_json::from_str::<PtyAttachFrame>(line) {
                    Ok(frame) => on_frame(frame),
                    Err(err) => {
                        parse_error = Some(RouxError::Decode(err));
                        false
                    }
                },
            );
            result.and_then(|_| match parse_error {
                Some(err) => Err(err),
                None => Ok(()),
            })
        })
        .await
        .map_err(|err| RouxError::Transport(format!("SDK task join failed: {err}")))?
    }
}

impl SpawnTask {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn pane_id(mut self, pane_id: impl Into<String>) -> Self {
        self.pane_id = Some(pane_id.into());
        self
    }

    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    pub fn initial_size(mut self, cols: u16, rows: u16) -> Self {
        self.initial_size = Some((cols, rows));
        self
    }

    pub async fn spawn(self) -> RouxResult<Pty> {
        let mut args = serde_json::Map::new();
        args.insert("command".into(), Value::String(self.command));
        if let Some(id) = self.id {
            args.insert("id".into(), Value::String(id));
        }
        if let Some(working_dir) = self.working_dir {
            args.insert("workingDir".into(), Value::String(working_dir));
        }
        if let Some(session_id) = self.session_id {
            args.insert("sessionId".into(), Value::String(session_id));
        }
        if let Some(pane_id) = self.pane_id {
            args.insert("paneId".into(), Value::String(pane_id));
        }
        if let Some(profile) = self.profile {
            args.insert("profile".into(), Value::String(profile));
        }
        if let Some((cols, rows)) = self.initial_size {
            args.insert("initialSize".into(), serde_json::json!([cols, rows]));
        }
        let record: PtyRecord = self
            .client
            .command(CommandRequest::new("daemon-pty-spawn-task").args(Value::Object(args)))
            .await?;
        Ok(self.client.pty(record.id))
    }
}

impl SpawnShell {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn pane_id(mut self, pane_id: impl Into<String>) -> Self {
        self.pane_id = Some(pane_id.into());
        self
    }

    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    pub fn initial_size(mut self, cols: u16, rows: u16) -> Self {
        self.initial_size = Some((cols, rows));
        self
    }

    pub async fn spawn(self) -> RouxResult<Pty> {
        let mut args = serde_json::Map::new();
        if let Some(id) = self.id {
            args.insert("id".into(), Value::String(id));
        }
        if let Some(working_dir) = self.working_dir {
            args.insert("workingDir".into(), Value::String(working_dir));
        }
        if let Some(session_id) = self.session_id {
            args.insert("sessionId".into(), Value::String(session_id));
        }
        if let Some(pane_id) = self.pane_id {
            args.insert("paneId".into(), Value::String(pane_id));
        }
        if let Some(profile) = self.profile {
            args.insert("profile".into(), Value::String(profile));
        }
        if let Some((cols, rows)) = self.initial_size {
            args.insert("initialSize".into(), serde_json::json!([cols, rows]));
        }
        let record: PtyRecord = self
            .client
            .command(CommandRequest::new("daemon-pty-spawn-shell").args(Value::Object(args)))
            .await?;
        Ok(self.client.pty(record.id))
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PtyWrite {
    pub id: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LatestOutput {
    pub session_id: Option<String>,
    pub pane_id: Option<String>,
    pub pty_id: String,
    pub max_bytes: usize,
    pub output: String,
    pub output_bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    #[cfg(not(windows))]
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn parses_socket_endpoints() {
        assert_eq!(
            parse_socket_endpoint("tcp://127.0.0.1:4444"),
            Some(SocketEndpoint::Tcp("127.0.0.1:4444".to_string()))
        );
        assert_eq!(
            parse_socket_endpoint("unix:///tmp/roux.sock"),
            Some(SocketEndpoint::Unix("/tmp/roux.sock".into()))
        );
        #[cfg(not(windows))]
        assert_eq!(
            parse_socket_endpoint("/tmp/roux.sock"),
            Some(SocketEndpoint::Unix("/tmp/roux.sock".into()))
        );
    }

    #[test]
    fn command_request_serializes_protocol_shape() {
        let request = CommandRequest::new("daemon-status")
            .session_id("session-a")
            .pane_id("pane-a")
            .auth_token("secret")
            .args(serde_json::json!({ "maxBytes": 1024 }));

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "command": "daemon-status",
                "session_id": "session-a",
                "pane_id": "pane-a",
                "auth_token": "secret",
                "args": { "maxBytes": 1024 },
            })
        );
    }

    #[test]
    fn command_response_turns_error_frame_into_error() {
        let response: CommandResponse = serde_json::from_value(serde_json::json!({
            "ok": false,
            "error": "nope",
        }))
        .unwrap();

        assert_eq!(response.into_result().unwrap_err().to_string(), "nope");
    }

    #[test]
    fn blocking_tcp_client_injects_auth_token_and_returns_raw_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(br#"{"ok":true,"data":{"kind":"roux-daemon","pid":1,"socket":"tcp://test","startedAtMs":1,"uptimeMs":2,"sessionCount":0,"projectCount":0,"watchCount":0,"processCount":0,"ptyCount":0,"capabilities":["daemon-status"]}}"#)
                .unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let status = client.command_blocking(CommandRequest::new("daemon-status")).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["auth_token"], "secret");
        assert_eq!(status["kind"], "roux-daemon");
    }

    #[cfg(not(windows))]
    #[test]
    fn blocking_unix_socket_client_uses_sock_file_without_auth() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("roux.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(br#"{"ok":true,"data":{"seen":true}}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder().endpoint(SocketEndpoint::Unix(sock)).connect().unwrap();
        let response = client.command_blocking(CommandRequest::new("daemon-status")).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["auth_token"], Value::Null);
        assert_eq!(response["seen"], true);
    }

    #[test]
    fn typed_status_decodes_from_daemon_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            stream
                .write_all(br#"{"ok":true,"data":{"kind":"roux-daemon","pid":42,"socket":"tcp://test","startedAtMs":10,"uptimeMs":20,"sessionCount":1,"projectCount":2,"watchCount":3,"processCount":4,"ptyCount":5,"capabilities":["daemon-status","daemon-pty-list"]}}"#)
                .unwrap();
            stream.write_all(b"\n").unwrap();
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let status = rt.block_on(client.status()).unwrap();
        handle.join().unwrap();

        assert_eq!(status.kind, "roux-daemon");
        assert_eq!(status.pty_count, 5);
        assert!(status.capabilities.iter().any(|capability| capability == "daemon-pty-list"));
    }

    #[test]
    fn typed_spawn_task_builder_returns_pty_handle() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(r#"{{"ok":true,"data":{}}}"#, sample_pty_record_json("pty-1"))
                        .as_bytes(),
                )
                .unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let pty = rt
            .block_on(
                client
                    .spawn_task("printf hello")
                    .id("pty-requested")
                    .working_dir("/tmp")
                    .session_id("session-a")
                    .pane_id("pane-a")
                    .profile("task")
                    .initial_size(100, 30)
                    .spawn(),
            )
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(pty.id(), "pty-1");
        assert_eq!(request["command"], "daemon-pty-spawn-task");
        assert_eq!(request["args"]["command"], "printf hello");
        assert_eq!(request["args"]["id"], "pty-requested");
        assert_eq!(request["args"]["workingDir"], "/tmp");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([100, 30]));
    }

    #[test]
    fn typed_pty_attach_decodes_ndjson_frames() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(
                        "{{\"type\":\"ready\",\"id\":\"pty-1\",\"record\":{},\"replayOffset\":0,\"replayBytes\":[104,105]}}\n",
                        sample_pty_record_json("pty-1")
                    )
                    .as_bytes(),
                )
                .unwrap();
            stream.write_all(br#"{"type":"exit","code":0,"generation":1}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let pty = client.pty("pty-1");
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let frames_for_callback = frames.clone();
        rt.block_on(pty.attach(1024, move |frame| {
            let label = match frame {
                PtyAttachFrame::Ready { .. } => "ready",
                PtyAttachFrame::Output { .. } => "output",
                PtyAttachFrame::Exit { .. } => "exit",
                PtyAttachFrame::Error { .. } => "error",
            };
            frames_for_callback.lock().unwrap().push(label);
            true
        }))
        .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["command"], "daemon-pty-attach");
        assert_eq!(request["args"]["id"], "pty-1");
        assert_eq!(*frames.lock().unwrap(), vec!["ready", "exit"]);
    }

    fn sample_pty_record_json(id: &str) -> String {
        serde_json::json!({
            "id": id,
            "kind": "task",
            "command": "printf hello",
            "workingDir": "/tmp",
            "startedAtMs": 1,
            "running": false,
            "exitCode": 0,
            "generation": 1,
            "retainedOutputBytes": 5,
            "outputTruncated": false,
            "cols": 80,
            "rows": 24,
            "info": {
                "id": id,
                "session_id": "session-a",
                "role": "secondary",
                "status": { "type": "Exited", "code": 0, "at_ms": 2 },
                "name": null,
                "working_dir": "/tmp",
                "profile": "task",
                "unread_output": false,
                "bell_pending": false,
            }
        })
        .to_string()
    }
}
