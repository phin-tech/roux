//! Typed Rust SDK for Roux's daemon-first local API.

mod client;
mod endpoint;
mod error;
mod handles;
mod protocol;
mod requests;
mod streams;
mod types;

pub mod blocking;

pub use client::{Roux, RouxBuilder};
pub use endpoint::{parse_socket_endpoint, resolve_socket_endpoint, SocketEndpoint};
pub use error::{RouxError, RouxResult};
pub use handles::{LatestOutput, Pty, PtyWrite, Session, SpawnShell, SpawnTask};
pub use protocol::{CommandRequest, CommandResponse};
pub use requests::{CreateSessionShell, MailboxPost, NotesEnv, ReconnectSessionShell};
pub use streams::{
    AliasEventStreamFrame, MailboxEventStreamFrame, SubscriptionEventStreamFrame,
    WatchEventStreamFrame, WorkItemEventStreamFrame,
};
pub use types::{
    DaemonStatus, PtyAttachFrame, PtyKind, PtyRecord, PtySnapshot, WorkItemMigrationStatus,
    WorkItemMigrationStorage,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    #[cfg(not(windows))]
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    static ENDPOINT_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_endpoint_env<T>(base_path: &std::path::Path, f: impl FnOnce() -> T) -> T {
        let _guard = ENDPOINT_ENV_LOCK.lock().unwrap();
        let previous_base = std::env::var_os("ROUX_BASE_PATH");
        let previous_socket = std::env::var_os("ROUX_SOCKET");

        std::env::set_var("ROUX_BASE_PATH", base_path);
        std::env::remove_var("ROUX_SOCKET");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));

        match previous_base {
            Some(value) => std::env::set_var("ROUX_BASE_PATH", value),
            None => std::env::remove_var("ROUX_BASE_PATH"),
        }
        match previous_socket {
            Some(value) => std::env::set_var("ROUX_SOCKET", value),
            None => std::env::remove_var("ROUX_SOCKET"),
        }

        match result {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

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
    fn resolves_persisted_tcp_socket_endpoint_when_env_socket_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("roux-socket-addr"), "tcp://100.73.57.24:7777").unwrap();

        let endpoint = with_endpoint_env(dir.path(), resolve_socket_endpoint);

        assert_eq!(endpoint, Some(SocketEndpoint::Tcp("100.73.57.24:7777".to_string())));
    }

    #[cfg(not(windows))]
    #[test]
    fn resolves_persisted_relative_unix_socket_endpoint_when_env_socket_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("roux-socket-addr"), "roux.sock").unwrap();

        let endpoint = with_endpoint_env(dir.path(), resolve_socket_endpoint);

        assert_eq!(endpoint, Some(SocketEndpoint::Unix("roux.sock".into())));
    }

    #[cfg(not(windows))]
    #[test]
    fn resolves_legacy_bare_tcp_socket_endpoint_when_env_socket_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("roux-socket-addr"), "127.0.0.1:7777").unwrap();

        let endpoint = with_endpoint_env(dir.path(), resolve_socket_endpoint);

        assert_eq!(endpoint, Some(SocketEndpoint::Tcp("127.0.0.1:7777".to_string())));
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

        let status = blocking::send_raw_request(
            &SocketEndpoint::Tcp(addr),
            Some("secret"),
            Duration::from_secs(5),
            CommandRequest::new("daemon-status"),
        )
        .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["auth_token"], "secret");
        assert_eq!(status["ok"], true);
        assert_eq!(status["data"]["kind"], "roux-daemon");
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

    #[cfg(not(windows))]
    #[test]
    fn stream_lines_blocking_uses_unix_sock_without_auth() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("roux.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(br#"{"type":"ready"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            stream.write_all(br#"{"type":"event","id":"one"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder().endpoint(SocketEndpoint::Unix(sock)).connect().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines_for_callback = lines.clone();
        client
            .stream_lines_blocking(CommandRequest::new("watch-events"), move |line| {
                lines_for_callback.lock().unwrap().push(line.to_string());
                true
            })
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["auth_token"], Value::Null);
        assert_eq!(
            *lines.lock().unwrap(),
            vec![r#"{"type":"ready"}"#, r#"{"type":"event","id":"one"}"#]
        );
    }

    #[test]
    fn typed_watch_events_stream_uses_daemon_command_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(br#"{"type":"ready"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            stream.write_all(br#"{"type":"warning","message":"heads up"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let frames_for_callback = frames.clone();
        client
            .watch_events_blocking(true, move |frame| {
                let label = match frame {
                    WatchEventStreamFrame::Ready => "ready",
                    WatchEventStreamFrame::Warning { .. } => "warning",
                    WatchEventStreamFrame::Update { .. } => "update",
                    WatchEventStreamFrame::Error { .. } => "error",
                };
                frames_for_callback.lock().unwrap().push(label);
                true
            })
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["command"], "watch-events");
        assert_eq!(request["args"]["backlog"], true);
        assert_eq!(*frames.lock().unwrap(), vec!["ready", "warning"]);
    }

    #[test]
    fn typed_watch_events_stream_continues_after_malformed_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(br#"{"type":"ready"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            stream.write_all(b"not-json\n").unwrap();
            stream.write_all(br#"{"type":"warning","message":"heads up"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let frames_for_callback = frames.clone();
        let err = client
            .watch_events_blocking(true, move |frame| {
                let label = match frame {
                    WatchEventStreamFrame::Ready => "ready",
                    WatchEventStreamFrame::Warning { .. } => "warning",
                    WatchEventStreamFrame::Update { .. } => "update",
                    WatchEventStreamFrame::Error { .. } => "error",
                };
                frames_for_callback.lock().unwrap().push(label);
                true
            })
            .unwrap_err();
        let request = handle.join().unwrap();

        assert!(matches!(err, RouxError::Decode(_)));
        assert_eq!(request["command"], "watch-events");
        assert_eq!(*frames.lock().unwrap(), vec!["ready", "warning"]);
    }

    #[test]
    fn typed_status_decodes_from_daemon_response() {
        let (client, handle) = tcp_client_with_response(
            r#"{"ok":true,"data":{"kind":"roux-daemon","pid":42,"socket":"tcp://test","startedAtMs":10,"uptimeMs":20,"sessionCount":1,"projectCount":2,"watchCount":3,"processCount":4,"ptyCount":5,"workItemMigrationStatus":{"currentVersion":7,"targetVersion":7,"pendingVersions":[],"storage":"boardDb","error":null},"capabilities":["daemon-status","daemon-pty-list"]}}"#
                .to_string(),
        );
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let status = rt.block_on(client.status()).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["command"], "daemon-status");
        assert_eq!(status.kind, "roux-daemon");
        assert_eq!(status.pty_count, 5);
        assert_eq!(status.work_item_migration_status.as_ref().unwrap().current_version, 7);
        assert!(status.capabilities.iter().any(|capability| capability == "daemon-pty-list"));
    }

    #[test]
    fn typed_project_create_uses_daemon_command_shape() {
        let (client, handle) = tcp_client_with_response(
            r#"{"ok":true,"data":{"id":"project-a","name":"Alpha"}}"#.to_string(),
        );
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let project = rt.block_on(client.create_project("Alpha")).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(project.id, "project-a");
        assert_eq!(request["command"], "project-create");
        assert_eq!(request["args"]["name"], "Alpha");
    }

    #[test]
    fn typed_mailbox_post_uses_daemon_command_shape() {
        let (client, handle) = tcp_client_with_response(
            r#"{"ok":true,"data":{"id":"event-a","createdAt":1,"to":"reviewer","topic":"build.done","from":"runner","kind":"fyi","correlationId":"corr-a","projectId":"project-a","subject":"Build","body":"ready","structured":{"ok":true}}}"#
                .to_string(),
        );
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let event = rt
            .block_on(client.mailbox_post(MailboxPost {
                to: Some("reviewer".to_string()),
                topic: Some("build.done".to_string()),
                body: "ready".to_string(),
                subject: Some("Build".to_string()),
                kind: Some("fyi".to_string()),
                project_id: Some("project-a".to_string()),
                correlation_id: Some("corr-a".to_string()),
                structured: Some(serde_json::json!({ "ok": true })),
                from: Some("runner".to_string()),
            }))
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(event.id, "event-a");
        assert_eq!(request["command"], "mailbox-post");
        assert_eq!(request["args"]["to"], "reviewer");
        assert_eq!(request["args"]["topic"], "build.done");
        assert_eq!(request["args"]["body"], "ready");
        assert_eq!(request["args"]["subject"], "Build");
        assert_eq!(request["args"]["kind"], "fyi");
        assert_eq!(request["args"]["project_id"], "project-a");
        assert_eq!(request["args"]["correlation_id"], "corr-a");
        assert_eq!(request["args"]["structured"]["ok"], true);
        assert_eq!(request["args"]["from"], "runner");
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
    fn typed_create_session_shell_uses_daemon_command_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(r#"{{"ok":true,"data":{}}}"#, sample_session_json("session-a", false))
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
        let session = rt
            .block_on(client.create_session_shell(CreateSessionShell {
                id: "session-a".to_string(),
                repo_path: "/repo".to_string(),
                name: "Daemon Session".to_string(),
                worktree_path: None,
                branch: Some("feature/demo".to_string()),
                base: Some("origin/main".to_string()),
                fetch_first: true,
                profile: Some("plain-shell".to_string()),
                profile_data: None,
                env_overrides: None,
                initial_size: Some((100, 30)),
                project_id: Some("project-a".to_string()),
                blueprint_id: Some("blueprint-a".to_string()),
                notes: Some(NotesEnv {
                    vault_root: "/vault".to_string(),
                    session_slug: "feature-demo--sessio".to_string(),
                    repo_slug: "repo-a".to_string(),
                    project_slug: Some("project-a".to_string()),
                    context_paths: vec!["/repo/docs".to_string()],
                    project_prompt: "Use project notes".to_string(),
                }),
            }))
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(session.id, "session-a");
        assert_eq!(request["command"], "session-create-shell");
        assert_eq!(request["args"]["id"], "session-a");
        assert_eq!(request["args"]["repoPath"], "/repo");
        assert_eq!(request["args"]["branch"], "feature/demo");
        assert_eq!(request["args"]["base"], "origin/main");
        assert_eq!(request["args"]["fetchFirst"], true);
        assert_eq!(request["args"]["profile"], "plain-shell");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([100, 30]));
        assert_eq!(request["args"]["projectId"], "project-a");
        assert_eq!(request["args"]["blueprintId"], "blueprint-a");
        assert_eq!(request["args"]["notesEnv"]["vaultRoot"], "/vault");
        assert_eq!(request["args"]["notesEnv"]["contextPaths"][0], "/repo/docs");
    }

    #[test]
    fn typed_reconnect_session_shell_uses_daemon_command_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(r#"{{"ok":true,"data":{}}}"#, sample_session_json("session-a", false))
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
        let session = rt
            .block_on(client.reconnect_session_shell(ReconnectSessionShell {
                id: "session-a".to_string(),
                profile: Some("plain-shell".to_string()),
                profile_data: None,
                env_overrides: None,
                initial_size: Some((120, 40)),
                notes: Some(NotesEnv {
                    vault_root: "/vault".to_string(),
                    session_slug: "feature-demo--sessio".to_string(),
                    repo_slug: "repo-a".to_string(),
                    project_slug: None,
                    context_paths: vec![],
                    project_prompt: "".to_string(),
                }),
            }))
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(session.id, "session-a");
        assert_eq!(request["command"], "session-reconnect-shell");
        assert_eq!(request["session_id"], "session-a");
        assert_eq!(request["args"]["profile"], "plain-shell");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([120, 40]));
        assert_eq!(request["args"]["notesEnv"]["vaultRoot"], "/vault");
    }

    #[test]
    fn session_rename_none_sends_empty_string_to_clear_override() {
        let (client, handle) = tcp_client_with_response(r#"{"ok":true,"data":{}}"#.to_string());
        let session: roux_core::Session =
            serde_json::from_str(&sample_session_json("session-a", false)).unwrap();
        let session = client.session(session);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();

        rt.block_on(session.rename(None)).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["command"], "session-rename");
        assert_eq!(request["session_id"], "session-a");
        assert_eq!(request["args"]["name"], "");
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

    #[test]
    fn typed_pty_attach_continues_after_malformed_frame() {
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
            stream.write_all(b"not-json\n").unwrap();
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
        let err = rt
            .block_on(pty.attach(1024, move |frame| {
                let label = match frame {
                    PtyAttachFrame::Ready { .. } => "ready",
                    PtyAttachFrame::Output { .. } => "output",
                    PtyAttachFrame::Exit { .. } => "exit",
                    PtyAttachFrame::Error { .. } => "error",
                };
                frames_for_callback.lock().unwrap().push(label);
                true
            }))
            .unwrap_err();
        let request = handle.join().unwrap();

        assert!(matches!(err, RouxError::Decode(_)));
        assert_eq!(request["command"], "daemon-pty-attach");
        assert_eq!(*frames.lock().unwrap(), vec!["ready", "exit"]);
    }

    fn tcp_client_with_response(response: String) -> (Roux, thread::JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });
        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        (client, handle)
    }

    fn sample_session_json(id: &str, archived: bool) -> String {
        serde_json::json!({
            "id": id,
            "name": "Daemon Session",
            "repoRoot": "/repo",
            "worktreePath": "/repo",
            "branch": "feature/demo",
            "isWorktree": false,
            "model": null,
            "cost": null,
            "createdAt": 1,
            "status": "idle",
            "isGitRepo": true,
            "nameOverride": null,
            "primaryPtyId": "pty-primary",
            "archived": archived,
            "endedAt": null,
            "projectId": "project-a",
            "blueprintId": "blueprint-a",
            "pinnedPrUrl": null
        })
        .to_string()
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
