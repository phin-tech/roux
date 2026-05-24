use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
#[cfg(not(windows))]
use tokio::net::UnixListener;
use tokio::sync::watch;

use roux_core::{
    AliasEvent, BusSubscriptionEvent, ConsumptionMode, CreateWatchConfig, EventBuilder, EventKind,
    MailboxEvent, Project, ProjectUpdate, PtyRole, PtyStatus, RuntimeState, SessionStatusEvent,
    Watch,
};
use roux_runtime::alias_service::AliasManager;
use roux_runtime::alias_store::{BindRequest, ProjectFilter};
use roux_runtime::automation_hooks::{
    context_from_run_request, hook_list_to_value, hook_run_to_value, request_from_socket_args,
    worktree_provider_hooks, AutomationHookManager, HookContext, HookEvent, HookRunSummary,
};
use roux_runtime::host::{RuntimeHost, RuntimeHostConfig};
use roux_runtime::mailbox_service::MailboxManager;
use roux_runtime::process_service::PROCESS_OUTPUT_DEFAULT_POLL_BYTES;
use roux_runtime::pty_service::{
    PtyEnvRequest, PtyOutputEvent, PtySpawnRequest, PTY_OUTPUT_DEFAULT_POLL_BYTES,
};
use roux_runtime::subscription_service::SubscriptionManager;
use roux_runtime::terminal_env::{NonoConfig, NotesEnvInputs, SmolvmExec};
use roux_runtime::watch_runner::WatchRunner;

use crate::{daemon_log::DaemonLog, paths, platform};

const DEFAULT_LATEST_OUTPUT_BYTES: usize = 8 * 1024;
const MAX_LATEST_OUTPUT_BYTES: usize = 64 * 1024;

pub async fn run() -> Result<(), String> {
    paths::migrate_legacy_config_dir();
    let log = DaemonLog::init();

    let project_path = platform::projects_path();
    let session_path = platform::sessions_path();
    let watch_path = platform::watches_path();
    let projects = roux_runtime::project_service::load_persisted_from(&project_path);
    let sessions = roux_runtime::session_service::load_persisted_from(&session_path, &projects);
    let watches = roux_runtime::watch_service::load_persisted_from(&watch_path);
    log.write(&format!(
        "Loaded {} project(s) from {}, {} session(s) from {}, and {} watch(es) from {}",
        projects.len(),
        project_path.display(),
        sessions.len(),
        session_path.display(),
        watches.len(),
        watch_path.display()
    ));

    let services = RuntimeHostConfig {
        initial_sessions: sessions,
        session_persist_path: session_path,
        initial_projects: projects,
        project_persist_path: project_path,
        initial_watches: watches,
        watch_persist_path: Some(watch_path),
        work_item_db_path: platform::work_items_db_path(),
    }
    .build();

    let (host, joins) = services.spawn_with(tokio::spawn);

    let status_dir = platform::status_dir();
    if let Err(err) = roux_runtime::session_status_source::start_watching(
        status_dir,
        host.session_handle.clone(),
    ) {
        log.write(&format!("Warning: failed to start session status watcher: {err}"));
    }

    let watch_runner = WatchRunner::new(host.watch_handle.clone(), daemon_hook_manager());
    watch_runner.start_all().await;
    let endpoint = platform::daemon_bind_endpoint();
    let auth_token = daemon_auth_token(&endpoint)?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let identity =
        DaemonIdentity::new(endpoint, log.path().clone(), auth_token).with_shutdown(shutdown_tx);
    let socket_server =
        start_socket_server(host.clone(), watch_runner.clone(), identity.clone(), log.clone())
            .await?;
    log.write(&format!(
        "Started on {}; press Ctrl-C to stop",
        socket_server.endpoint.display_value()
    ));

    wait_for_shutdown_signal(shutdown_rx).await?;
    log.write("Shutdown signal received");

    socket_server.shutdown();
    log.write("Socket server stopped");
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    watch_runner.shutdown();
    host.watch_handle.shutdown().await;
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

#[derive(Clone)]
struct DaemonIdentity {
    started_at_ms: u64,
    socket: PathBuf,
    log_path: PathBuf,
    #[cfg_attr(not(windows), allow(dead_code))]
    auth_token: Option<String>,
    endpoint: platform::SocketEndpoint,
    alias_manager: AliasManager,
    subscription_manager: SubscriptionManager,
    mailbox_manager: MailboxManager,
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl DaemonIdentity {
    fn new(
        endpoint: platform::SocketEndpoint,
        log_path: PathBuf,
        auth_token: Option<String>,
    ) -> Self {
        let subscription_manager =
            SubscriptionManager::load_from(paths::roux_config_dir().join("subscriptions.json"));
        let mailbox_manager = MailboxManager::load_from(
            paths::roux_config_dir().join("events.jsonl"),
            paths::roux_config_dir().join("read_state.json"),
        )
        .with_subscriptions(subscription_manager.clone());
        Self {
            started_at_ms: unix_now_ms(),
            socket: endpoint_path(&endpoint),
            log_path,
            auth_token,
            endpoint,
            alias_manager: AliasManager::load_from(paths::roux_config_dir().join("aliases.json")),
            subscription_manager,
            mailbox_manager,
            shutdown_tx: None,
        }
    }

    fn with_shutdown(mut self, shutdown_tx: watch::Sender<bool>) -> Self {
        self.shutdown_tx = Some(shutdown_tx);
        self
    }

    fn endpoint_display(&self) -> String {
        self.endpoint.display_value()
    }

    fn request_shutdown(&self) -> bool {
        self.shutdown_tx.as_ref().map(|tx| tx.send(true).is_ok()).unwrap_or(false)
    }

    #[cfg(test)]
    fn new_for_test(socket: impl Into<PathBuf>) -> Self {
        let subscription_manager = SubscriptionManager::in_memory();
        let socket = socket.into();
        Self {
            started_at_ms: 1_000,
            socket: socket.clone(),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token: None,
            endpoint: platform::SocketEndpoint::Unix(socket),
            alias_manager: AliasManager::in_memory(),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::in_memory().with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }

    #[cfg(test)]
    fn new_for_test_with_endpoint(
        endpoint: platform::SocketEndpoint,
        auth_token: Option<String>,
    ) -> Self {
        let subscription_manager = SubscriptionManager::in_memory();
        Self {
            started_at_ms: 1_000,
            socket: endpoint_path(&endpoint),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token,
            endpoint,
            alias_manager: AliasManager::in_memory(),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::in_memory().with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }

    #[cfg(test)]
    fn new_for_test_with_alias_path(socket: impl Into<PathBuf>, alias_path: PathBuf) -> Self {
        let subscription_manager = SubscriptionManager::in_memory();
        let socket = socket.into();
        Self {
            started_at_ms: 1_000,
            socket: socket.clone(),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token: None,
            endpoint: platform::SocketEndpoint::Unix(socket),
            alias_manager: AliasManager::load_from(alias_path),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::in_memory().with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }

    #[cfg(test)]
    fn new_for_test_with_runtime_paths(
        socket: impl Into<PathBuf>,
        alias_path: PathBuf,
        subscription_path: PathBuf,
        mailbox_events_path: PathBuf,
        mailbox_read_state_path: PathBuf,
    ) -> Self {
        let subscription_manager = SubscriptionManager::load_from(subscription_path);
        let socket = socket.into();
        Self {
            started_at_ms: 1_000,
            socket: socket.clone(),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token: None,
            endpoint: platform::SocketEndpoint::Unix(socket),
            alias_manager: AliasManager::load_from(alias_path),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::load_from(
                mailbox_events_path,
                mailbox_read_state_path,
            )
            .with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }
}

fn endpoint_path(endpoint: &platform::SocketEndpoint) -> PathBuf {
    match endpoint {
        platform::SocketEndpoint::Unix(path) => path.clone(),
        platform::SocketEndpoint::Tcp(_) => PathBuf::from(endpoint.display_value()),
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

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum PtyAttachFrame {
    #[serde(rename = "ready")]
    Ready {
        id: String,
        record: Box<roux_runtime::pty_service::PtyRecord>,
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

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum WatchEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "update")]
    Update { event: Box<roux_core::WatchUpdateEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AliasEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: AliasEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum MailboxEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: Box<MailboxEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum SubscriptionEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: BusSubscriptionEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
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
    endpoint: platform::SocketEndpoint,
}

impl SocketServerHandle {
    fn shutdown(self) {
        self.join.abort();
        self.cleanup.remove();
    }
}

struct SocketCleanup {
    paths: Vec<PathBuf>,
}

impl SocketCleanup {
    fn remove(self) {
        for path in self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn start_socket_server(
    host: RuntimeHost,
    watch_runner: WatchRunner,
    mut identity: DaemonIdentity,
    log: DaemonLog,
) -> Result<SocketServerHandle, String> {
    match identity.endpoint.clone() {
        platform::SocketEndpoint::Unix(path) => {
            #[cfg(not(windows))]
            {
                let listener = bind_unix_listener(&path)?;
                log.write(&format!("Socket server listening on {}", path.display()));
                let cleanup_paths = vec![path.clone()];
                let endpoint = platform::SocketEndpoint::Unix(path);
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
                        let watch_runner = watch_runner.clone();
                        let identity = identity.clone();
                        let log = log.clone();
                        tokio::spawn(async move {
                            let (reader, mut writer) = stream.into_split();
                            let mut reader = BufReader::new(reader);
                            handle_connection(
                                &mut reader,
                                &mut writer,
                                &host,
                                &watch_runner,
                                &identity,
                                &log,
                            )
                            .await;
                        });
                    }
                });
                Ok(SocketServerHandle {
                    join,
                    cleanup: SocketCleanup { paths: cleanup_paths },
                    endpoint,
                })
            }

            #[cfg(windows)]
            {
                Err(format!(
                    "Unix socket endpoints are not supported on Windows: {}",
                    path.display()
                ))
            }
        }
        platform::SocketEndpoint::Tcp(addr) => {
            let listener = bind_tcp_listener(&addr, &identity).await?;
            let local_addr = listener
                .local_addr()
                .map_err(|err| format!("resolve daemon socket address: {err}"))?
                .to_string();
            identity.endpoint = platform::SocketEndpoint::Tcp(local_addr.clone());
            identity.socket = endpoint_path(&identity.endpoint);
            log.write(&format!("Socket server listening on tcp://{local_addr}"));
            let endpoint = identity.endpoint.clone();
            let cleanup_paths =
                vec![platform::socket_addr_file_path(), platform::socket_auth_token_file_path()];
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
                    let watch_runner = watch_runner.clone();
                    let identity = identity.clone();
                    let log = log.clone();
                    tokio::spawn(async move {
                        let (reader, mut writer) = stream.into_split();
                        let mut reader = BufReader::new(reader);
                        handle_connection(
                            &mut reader,
                            &mut writer,
                            &host,
                            &watch_runner,
                            &identity,
                            &log,
                        )
                        .await;
                    });
                }
            });
            Ok(SocketServerHandle {
                join,
                cleanup: SocketCleanup { paths: cleanup_paths },
                endpoint,
            })
        }
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

async fn bind_tcp_listener(addr: &str, identity: &DaemonIdentity) -> Result<TcpListener, String> {
    if identity.auth_token.as_deref().unwrap_or_default().is_empty() {
        return Err("TCP daemon bind requires ROUX_DAEMON_TOKEN".to_string());
    }

    if let Some(parent) = platform::socket_addr_file_path().parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create socket directory {}: {err}", parent.display()))?;
    }

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|err| format!("bind daemon TCP socket {addr}: {err}"))?;
    let local_addr = listener
        .local_addr()
        .map_err(|err| format!("resolve daemon socket address: {err}"))?
        .to_string();
    std::fs::write(platform::socket_addr_file_path(), &local_addr)
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
    watch_runner: &WatchRunner,
    identity: &DaemonIdentity,
    log: &DaemonLog,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut line = String::new();
    let mut shutdown_after_response = false;
    let response = match reader.read_line(&mut line).await {
        Ok(0) => return,
        Ok(_) => match serde_json::from_str::<Request>(line.trim()) {
            Ok(req) => {
                let command = req.command.clone();
                if command == "daemon-pty-attach" {
                    let ok = handle_daemon_pty_attach_stream(req, writer, host, identity).await;
                    if ok {
                        log.write("Handled socket command: daemon-pty-attach");
                    } else {
                        log.write("Socket command failed: daemon-pty-attach");
                    }
                    return;
                }
                if command == "watch-events" {
                    let ok =
                        handle_watch_events_stream(req, writer, host, watch_runner, identity).await;
                    if ok {
                        log.write("Handled socket command: watch-events");
                    } else {
                        log.write("Socket command failed: watch-events");
                    }
                    return;
                }
                if command == "mailbox-events" {
                    let ok = handle_mailbox_events_stream(req, writer, identity).await;
                    if ok {
                        log.write("Handled socket command: mailbox-events");
                    } else {
                        log.write("Socket command failed: mailbox-events");
                    }
                    return;
                }
                if command == "alias-events" {
                    let ok = handle_alias_events_stream(req, writer, identity).await;
                    if ok {
                        log.write("Handled socket command: alias-events");
                    } else {
                        log.write("Socket command failed: alias-events");
                    }
                    return;
                }
                if command == "subscription-events" {
                    let ok = handle_subscription_events_stream(req, writer, identity).await;
                    if ok {
                        log.write("Handled socket command: subscription-events");
                    } else {
                        log.write("Socket command failed: subscription-events");
                    }
                    return;
                }
                if command == "session-events" {
                    let ok = handle_session_events_stream(req, writer, host).await;
                    if ok {
                        log.write("Handled socket command: session-events");
                    } else {
                        log.write("Socket command failed: session-events");
                    }
                    return;
                }
                let response =
                    handle_request_with_watch_runner(req, host, Some(watch_runner), identity).await;
                shutdown_after_response = command == "daemon-stop" && response.ok;
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

    if shutdown_after_response {
        if identity.request_shutdown() {
            log.write("Shutdown requested by daemon-stop");
        } else {
            log.write("daemon-stop requested but shutdown channel is unavailable");
        }
    }
}

#[cfg(test)]
async fn handle_request(req: Request, host: &RuntimeHost, identity: &DaemonIdentity) -> Response {
    handle_request_with_watch_runner(req, host, None, identity).await
}

async fn handle_request_with_watch_runner(
    req: Request,
    host: &RuntimeHost,
    watch_runner: Option<&WatchRunner>,
    identity: &DaemonIdentity,
) -> Response {
    if !request_authorized(&req, identity) {
        return Response::err("unauthorized");
    }

    match req.command.as_str() {
        "daemon-status" => handle_daemon_status(host, identity).await,
        "daemon-stop" => handle_daemon_stop(identity).await,
        "session-list" => handle_session_list(host).await,
        "session-poll" => handle_session_poll(req, host).await,
        "session-create" => handle_cli_session_create(req, host, identity).await,
        "session-create-shell" => handle_session_create_shell(req, host, identity).await,
        "session-reconnect-shell" => handle_session_reconnect_shell(req, host, identity).await,
        "session-panes-list" => handle_session_panes_list(req, host).await,
        "session-panes-create" => handle_session_panes_create(req, host, identity).await,
        "session-archive" => handle_session_archive(req, host).await,
        "session-kill" => handle_session_archive(req, host).await,
        "session-restore" => handle_session_restore(req, host).await,
        "session-delete" => handle_session_delete(req, host).await,
        "session-worktree-exists" => handle_session_worktree_exists(req, host).await,
        "session-refresh-branch" => handle_session_refresh_branch(req, host).await,
        "session-rename" => handle_session_rename(req, host).await,
        "session-set-project" => handle_session_set_project(req, host).await,
        "session-set-pinned-pr-url" => handle_session_set_pinned_pr_url(req, host).await,
        "session-set-smol-machine" => handle_session_set_smol_machine(req, host).await,
        "alias-set" => handle_alias_set(req, identity).await,
        "alias-unset" => handle_alias_unset(req, identity).await,
        "alias-claim" => handle_alias_claim(req, identity).await,
        "alias-list" => handle_alias_list(req, identity).await,
        "alias-get" => handle_alias_get(req, identity).await,
        "alias-whoami" => handle_alias_whoami(req, identity).await,
        "alias-add-member" => handle_alias_add_member(req, identity).await,
        "alias-remove-member" => handle_alias_remove_member(req, identity).await,
        "alias-mode" => handle_alias_mode(req, identity).await,
        "mailbox-post" => handle_mailbox_post(req, identity).await,
        "mailbox-peek" => handle_mailbox_peek(req, identity).await,
        "mailbox-read" => handle_mailbox_read(req, identity).await,
        "mailbox-get" => handle_mailbox_get(req, identity).await,
        "mailbox-read-state" => handle_mailbox_read_state(req, identity).await,
        "mailbox-mark-read" => handle_mailbox_mark_read(req, identity).await,
        "mailbox-ack" => handle_mailbox_ack(req, identity).await,
        "mailbox-retract" => handle_mailbox_retract(req, identity).await,
        "mailbox-dismiss" => handle_mailbox_dismiss(req, identity).await,
        "mailbox-count" => handle_mailbox_count(req, identity).await,
        "mailbox-clear" => handle_mailbox_clear(req, identity).await,
        "mailbox-reply" => handle_mailbox_reply(req, identity).await,
        "mailbox-sent" => handle_mailbox_sent(req, identity).await,
        "bus-publish" => handle_bus_publish(req, identity).await,
        "bus-tail" => handle_bus_tail(req, identity).await,
        "bus-subscribe" => handle_bus_subscribe(req, identity).await,
        "bus-unsubscribe" => handle_bus_unsubscribe(req, identity).await,
        "bus-subscriptions" => handle_bus_subscriptions(req, identity).await,
        "project-list" => handle_project_list(host).await,
        "project-create" => handle_project_create(req, host).await,
        "project-remove" => handle_project_remove(req, host).await,
        "project-rename" => handle_project_rename(req, host).await,
        "project-update" => handle_project_update(req, host).await,
        "watch-list" => handle_watch_list(host).await,
        "watch-create" => match watch_runner {
            Some(runner) => handle_watch_create(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-find-or-create" => match watch_runner {
            Some(runner) => handle_watch_find_or_create(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-remove" => match watch_runner {
            Some(runner) => handle_watch_remove(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-pause" => match watch_runner {
            Some(runner) => handle_watch_pause(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-resume" => match watch_runner {
            Some(runner) => handle_watch_resume(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-replace" => match watch_runner {
            Some(runner) => handle_watch_replace(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-remove-for-session" => match watch_runner {
            Some(runner) => handle_watch_remove_for_session(req, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "watch-cleanup-orphans" => match watch_runner {
            Some(runner) => handle_watch_cleanup_orphans(host, runner).await,
            None => Response::err("watch runner unavailable"),
        },
        "worktree-list" => handle_worktree_list(req).await,
        "worktree-create" => handle_worktree_create(req).await,
        "worktree-remove" => handle_worktree_remove(req).await,
        "worktree-list-branches" => handle_worktree_list_branches(req).await,
        "git-init" => handle_git_init(req).await,
        "notes-read" => handle_notes_read(req, host).await,
        "notes-write" => handle_notes_write(req, host).await,
        "notes-append" => handle_notes_append(req, host).await,
        "notes-path" => handle_notes_path(req, host).await,
        "notes-search" => handle_notes_search(req).await,
        "notes-vault-root" => handle_notes_vault_root(req).await,
        "hook-show" => handle_hook_show(req).await,
        "hook-preview" => handle_hook_preview(req).await,
        "hook-run" => handle_hook_run(req).await,
        "hook-approve" => handle_hook_approve(req).await,
        "hook-clear-approvals" => handle_hook_clear_approvals().await,
        "hook-log-list" => handle_hook_log_list().await,
        "hook-log-read" => handle_hook_log_read(req).await,
        "run" => handle_daemon_process_start(req, host).await,
        "shell" => handle_session_panes_create(req, host, identity).await,
        "split" => handle_session_panes_create(req, host, identity).await,
        "send" => handle_cli_send(req, host).await,
        "latest-output" => handle_latest_output(req, host).await,
        "daemon-process-start" => handle_daemon_process_start(req, host).await,
        "daemon-process-output" => handle_daemon_process_output(req, host).await,
        "daemon-process-list" => handle_daemon_process_list(host).await,
        "daemon-process-kill" => handle_daemon_process_kill(req, host).await,
        "daemon-pty-spawn-shell" => handle_daemon_pty_spawn_shell(req, host, identity).await,
        "daemon-pty-spawn-task" => handle_daemon_pty_spawn_task(req, host, identity).await,
        "daemon-pty-output" => handle_daemon_pty_output(req, host).await,
        "daemon-pty-list" => handle_daemon_pty_list(host).await,
        "daemon-pty-write" => handle_daemon_pty_write(req, host).await,
        "daemon-pty-resize" => handle_daemon_pty_resize(req, host).await,
        "daemon-pty-detach" => handle_daemon_pty_detach(req, host).await,
        "daemon-pty-attach-pane" => handle_daemon_pty_attach_pane(req, host).await,
        "daemon-pty-mark-read" => handle_daemon_pty_mark_read(req, host).await,
        "daemon-pty-set-name" => handle_daemon_pty_set_name(req, host).await,
        "daemon-pty-kill" => handle_daemon_pty_kill(req, host).await,
        _ => Response::err(format!("unknown daemon command: {}", req.command)),
    }
}

async fn handle_daemon_status(host: &RuntimeHost, identity: &DaemonIdentity) -> Response {
    let session_count = host.session_handle.list().await.map(|s| s.len()).unwrap_or(0);
    let project_count = host.project_handle.list().await.map(|p| p.len()).unwrap_or(0);
    let watch_count = host.watch_handle.list().await.map(|w| w.len()).unwrap_or(0);
    let process_count = host.process_handle.list().await.map(|p| p.len()).unwrap_or(0);
    let pty_count = host.pty_handle.list().await.map(|p| p.len()).unwrap_or(0);
    let capabilities = vec![
        "daemon-status",
        "daemon-stop",
        "session-list",
        "session-poll",
        "session-events",
        "session-create",
        "session-create-shell",
        "session-reconnect-shell",
        "session-panes-list",
        "session-panes-create",
        "session-archive",
        "session-kill",
        "session-restore",
        "session-delete",
        "session-worktree-exists",
        "session-refresh-branch",
        "session-rename",
        "session-set-project",
        "session-set-pinned-pr-url",
        "session-set-smol-machine",
        "alias-set",
        "alias-unset",
        "alias-claim",
        "alias-list",
        "alias-get",
        "alias-whoami",
        "alias-add-member",
        "alias-remove-member",
        "alias-mode",
        "alias-events",
        "mailbox-post",
        "mailbox-peek",
        "mailbox-read",
        "mailbox-get",
        "mailbox-read-state",
        "mailbox-mark-read",
        "mailbox-ack",
        "mailbox-retract",
        "mailbox-dismiss",
        "mailbox-count",
        "mailbox-clear",
        "mailbox-reply",
        "mailbox-sent",
        "mailbox-events",
        "bus-publish",
        "bus-tail",
        "bus-subscribe",
        "bus-unsubscribe",
        "bus-subscriptions",
        "subscription-events",
        "project-list",
        "project-create",
        "project-remove",
        "project-rename",
        "project-update",
        "watch-list",
        "watch-create",
        "watch-find-or-create",
        "watch-remove",
        "watch-pause",
        "watch-resume",
        "watch-replace",
        "watch-events",
        "watch-remove-for-session",
        "watch-cleanup-orphans",
        "worktree-list",
        "worktree-create",
        "worktree-remove",
        "worktree-list-branches",
        "git-init",
        "notes-read",
        "notes-write",
        "notes-append",
        "notes-path",
        "notes-search",
        "notes-vault-root",
        "hook-show",
        "hook-preview",
        "hook-run",
        "hook-approve",
        "hook-clear-approvals",
        "hook-log-list",
        "hook-log-read",
        "run",
        "shell",
        "split",
        "send",
        "latest-output",
        "daemon-process-start",
        "daemon-process-output",
        "daemon-process-list",
        "daemon-process-kill",
        "daemon-pty-spawn-shell",
        "daemon-pty-spawn-task",
        "daemon-pty-output",
        "daemon-pty-attach",
        "daemon-pty-list",
        "daemon-pty-write",
        "daemon-pty-resize",
        "daemon-pty-detach",
        "daemon-pty-attach-pane",
        "daemon-pty-mark-read",
        "daemon-pty-set-name",
        "daemon-pty-kill",
    ];
    Response::success(serde_json::json!({
        "kind": "roux-daemon",
        "pid": std::process::id(),
        "socket": identity.endpoint_display(),
        "logPath": identity.log_path.to_string_lossy(),
        "startedAtMs": identity.started_at_ms,
        "uptimeMs": unix_now_ms().saturating_sub(identity.started_at_ms),
        "sessionCount": session_count,
        "projectCount": project_count,
        "watchCount": watch_count,
        "processCount": process_count,
        "ptyCount": pty_count,
        "capabilities": capabilities,
    }))
}

async fn handle_daemon_stop(identity: &DaemonIdentity) -> Response {
    if identity.shutdown_tx.is_none() {
        return Response::err("daemon shutdown channel unavailable");
    }

    Response::success(serde_json::json!({
        "stopping": true,
        "pid": std::process::id(),
        "socket": identity.endpoint_display(),
        "logPath": identity.log_path.to_string_lossy(),
    }))
}

async fn handle_daemon_pty_attach_stream<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_daemon_pty_attach_stream_inner(req, writer, host, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_daemon_pty_attach_stream_inner<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_attach_frame(writer, &PtyAttachFrame::Error { error: "unauthorized".into() })
            .await;
        return false;
    }
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        let _ = write_attach_frame(writer, &PtyAttachFrame::Error { error: "id required".into() })
            .await;
        return false;
    };
    let max_replay_bytes = req
        .args
        .get("maxBytes")
        .or_else(|| req.args.get("max_bytes"))
        .and_then(|max_bytes| max_bytes.as_u64())
        .map(|max_bytes| max_bytes as usize)
        .unwrap_or(PTY_OUTPUT_DEFAULT_POLL_BYTES);

    let mut attach = match host.pty_handle.attach(id, max_replay_bytes).await {
        Ok(Some(attach)) => attach,
        Ok(None) => {
            let _ = write_attach_frame(
                writer,
                &PtyAttachFrame::Error { error: "daemon pty not found".into() },
            )
            .await;
            return false;
        }
        Err(err) => {
            let _ =
                write_attach_frame(writer, &PtyAttachFrame::Error { error: err.to_string() }).await;
            return false;
        }
    };

    let record = attach.record.clone();
    if !write_attach_frame(
        writer,
        &PtyAttachFrame::Ready {
            id: record.id.clone(),
            record: Box::new(record.clone()),
            replay_offset: attach.replay_offset,
            replay_bytes: attach.replay_bytes.clone(),
        },
    )
    .await
    {
        return false;
    }

    if !record.running {
        let _ = write_attach_frame(
            writer,
            &PtyAttachFrame::Exit { code: record.exit_code, generation: record.generation },
        )
        .await;
        return true;
    }

    loop {
        match attach.events.recv().await {
            Ok(PtyOutputEvent::Output(frame)) => {
                if !write_attach_frame(
                    writer,
                    &PtyAttachFrame::Output { offset: frame.offset, bytes: frame.bytes },
                )
                .await
                {
                    return false;
                }
            }
            Ok(PtyOutputEvent::Exit { code, generation }) => {
                let _ =
                    write_attach_frame(writer, &PtyAttachFrame::Exit { code, generation }).await;
                return true;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let _ = write_attach_frame(
                    writer,
                    &PtyAttachFrame::Error {
                        error: format!("daemon pty output stream lagged by {skipped} frame(s)"),
                    },
                )
                .await;
                return false;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_attach_frame<W>(writer: &mut W, frame: &PtyAttachFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

async fn handle_watch_events_stream<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    watch_runner: &WatchRunner,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_watch_events_stream_inner(req, writer, host, watch_runner, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_watch_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    watch_runner: &WatchRunner,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_watch_event_frame(
            writer,
            &WatchEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let send_backlog = req.args.get("backlog").and_then(|value| value.as_bool()).unwrap_or(true);
    let mut rx = watch_runner.subscribe();

    if !write_watch_event_frame(writer, &WatchEventFrame::Ready).await {
        return false;
    }

    if send_backlog {
        let watches = match host.watch_handle.list().await {
            Ok(watches) => watches,
            Err(err) => {
                let _ = write_watch_event_frame(
                    writer,
                    &WatchEventFrame::Error { error: err.to_string() },
                )
                .await;
                return false;
            }
        };
        for watch in watches {
            let event =
                roux_core::WatchUpdateEvent { watch, changed: false, previous_outcome: None };
            if !write_watch_event_frame(writer, &WatchEventFrame::Update { event: Box::new(event) })
                .await
            {
                return false;
            }
        }
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_watch_event_frame(
                    writer,
                    &WatchEventFrame::Update { event: Box::new(event) },
                )
                .await
                {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = WatchEventFrame::Warning {
                    message: format!("dropped {skipped} buffered watch event(s)"),
                };
                if !write_watch_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_watch_event_frame<W>(writer: &mut W, frame: &WatchEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

async fn handle_alias_events_stream<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_alias_events_stream_inner(req, writer, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_alias_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_alias_event_frame(
            writer,
            &AliasEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let mut rx = identity.alias_manager.subscribe_events();
    if !write_alias_event_frame(writer, &AliasEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_alias_event_frame(writer, &AliasEventFrame::Event { event }).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = AliasEventFrame::Warning {
                    message: format!("dropped {skipped} buffered alias event(s)"),
                };
                if !write_alias_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_alias_event_frame<W>(writer: &mut W, frame: &AliasEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

async fn handle_mailbox_events_stream<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_mailbox_events_stream_inner(req, writer, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_mailbox_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_mailbox_event_frame(
            writer,
            &MailboxEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let mut rx = identity.mailbox_manager.subscribe_events();
    if !write_mailbox_event_frame(writer, &MailboxEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_mailbox_event_frame(
                    writer,
                    &MailboxEventFrame::Event { event: Box::new(event) },
                )
                .await
                {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = MailboxEventFrame::Warning {
                    message: format!("dropped {skipped} buffered mailbox event(s)"),
                };
                if !write_mailbox_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_mailbox_event_frame<W>(writer: &mut W, frame: &MailboxEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

async fn handle_subscription_events_stream<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_subscription_events_stream_inner(req, writer, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_subscription_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_subscription_event_frame(
            writer,
            &SubscriptionEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let mut rx = identity.subscription_manager.subscribe_events();
    if !write_subscription_event_frame(writer, &SubscriptionEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_subscription_event_frame(writer, &SubscriptionEventFrame::Event { event })
                    .await
                {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = SubscriptionEventFrame::Warning {
                    message: format!("dropped {skipped} buffered subscription event(s)"),
                };
                if !write_subscription_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_subscription_event_frame<W>(writer: &mut W, frame: &SubscriptionEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum SessionEventFrame {
    Ready,
    Event { event: SessionStatusEvent },
    Warning { message: String },
}

async fn handle_session_events_stream<W>(req: Request, writer: &mut W, host: &RuntimeHost) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_session_events_stream_inner(req, writer, host).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_session_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    // Auth is checked by the outer `request_authorized` path for non-streaming
    // commands; streaming handlers repeat the check since they bypass it.
    // session-events carries no secret data but we keep the pattern consistent.
    drop(req); // no per-stream args needed today
    let mut rx = host.session_handle.subscribe_status();
    if !write_session_event_frame(writer, &SessionEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_session_event_frame(writer, &SessionEventFrame::Event { event }).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = SessionEventFrame::Warning {
                    message: format!("dropped {skipped} buffered session event(s)"),
                };
                if !write_session_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_session_event_frame<W>(writer: &mut W, frame: &SessionEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

fn request_authorized(req: &Request, identity: &DaemonIdentity) -> bool {
    match identity.auth_token.as_deref() {
        Some(expected) if !expected.is_empty() => req.auth_token.as_deref() == Some(expected),
        _ => true,
    }
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

async fn handle_session_set_project(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let project_id = optional_nullable_string_arg(&req.args, &["projectId", "project_id"]);
    match host.session_handle.set_project(session_id, project_id.clone()).await {
        Ok(()) => Response::success(serde_json::json!({
            "session_id": session_id,
            "project_id": project_id,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_set_pinned_pr_url(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let url = optional_nullable_string_arg(&req.args, &["url", "pinnedPrUrl", "pinned_pr_url"]);
    match host.session_handle.set_pinned_pr_url(session_id, url.clone()).await {
        Ok(()) => Response::success(serde_json::json!({
            "session_id": session_id,
            "url": url,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_set_smol_machine(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let machine_name = optional_nullable_string_arg(&req.args, &["machineName", "machine_name"]);
    match host.session_handle.set_smol_machine_name(session_id, machine_name.clone()).await {
        Ok(()) => Response::success(serde_json::json!({
            "session_id": session_id,
            "machine_name": machine_name,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_alias_set(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let session_id = match request_arg_str(&req, "session_id")
        .map(String::from)
        .or_else(|| req.session_id.clone())
    {
        Some(session_id) => session_id,
        None => {
            return Response::err(
                "session_id required (call from a session, or pass args.session_id)",
            )
        }
    };
    let bind_req = BindRequest {
        project_id: request_arg_str(&req, "project_id").map(String::from),
        session_id: Some(session_id),
        pane_id: request_arg_str(&req, "pane_id").map(String::from).or_else(|| req.pane_id.clone()),
        auto_claimed: false,
        force: request_arg_bool(&req, "force").unwrap_or(false),
    };

    match identity.alias_manager.bind(&canonical, bind_req) {
        Ok(alias) => Response::success(serde_json::to_value(alias).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_alias_unset(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let changed = identity.alias_manager.unbind(&canonical, request_arg_str(&req, "project_id"));
    Response::success(serde_json::json!({ "changed": changed }))
}

async fn handle_alias_claim(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let session_id = match req.session_id.clone() {
        Some(session_id) => session_id,
        None => return Response::err("alias-claim must be invoked from inside a session"),
    };
    let bind_req = BindRequest {
        project_id: request_arg_str(&req, "project_id").map(String::from),
        session_id: Some(session_id),
        pane_id: request_arg_str(&req, "pane_id").map(String::from).or_else(|| req.pane_id.clone()),
        auto_claimed: false,
        force: request_arg_bool(&req, "steal").unwrap_or(false),
    };

    match identity.alias_manager.bind(&canonical, bind_req) {
        Ok(alias) => Response::success(serde_json::to_value(alias).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_alias_list(req: Request, identity: &DaemonIdentity) -> Response {
    let aliases = identity.alias_manager.list(
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
        request_arg_bool(&req, "only_unbound").unwrap_or(false),
    );
    Response::success(serde_json::to_value(aliases).unwrap_or_default())
}

async fn handle_alias_get(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let project_id = request_arg_str(&req, "project_id");

    if let Some(alias) = identity.alias_manager.get(&canonical, project_id) {
        Response::success(serde_json::to_value(alias).unwrap_or_default())
    } else if project_id.is_none() {
        let matches = identity.alias_manager.find_all_by_name(&canonical);
        match matches.len() {
            0 => Response::err(format!("alias '{canonical}' not found")),
            1 => Response::success(serde_json::to_value(&matches[0]).unwrap_or_default()),
            _ => {
                let projects: Vec<_> =
                    matches.iter().map(|alias| alias.project_id.clone()).collect();
                Response::err(format!(
                    "alias '{canonical}' is ambiguous across projects {projects:?}; pass project_id"
                ))
            }
        }
    } else {
        Response::err(format!("alias '{canonical}' not found"))
    }
}

async fn handle_alias_whoami(req: Request, identity: &DaemonIdentity) -> Response {
    let session_id = match request_arg_str(&req, "session_id")
        .map(String::from)
        .or_else(|| req.session_id.clone())
    {
        Some(session_id) => session_id,
        None => {
            return Response::err(
                "session_id required (call from a session, or pass args.session_id)",
            )
        }
    };
    Response::success(
        serde_json::to_value(identity.alias_manager.whoami(&session_id)).unwrap_or_default(),
    )
}

async fn handle_alias_add_member(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match canonical_user_alias_arg(&req) {
        Ok(alias) => alias,
        Err(response) => return response,
    };
    let pane_id = match request_arg_str(&req, "pane_id")
        .map(String::from)
        .or_else(|| req.pane_id.clone())
    {
        Some(pane_id) => pane_id,
        None => return Response::err("pane_id required (call from a pane, or pass args.pane_id)"),
    };
    match identity.alias_manager.add_member(&alias, request_arg_str(&req, "project_id"), &pane_id) {
        Ok(alias) => Response::success(serde_json::to_value(alias).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_alias_remove_member(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match canonical_user_alias_arg(&req) {
        Ok(alias) => alias,
        Err(response) => return response,
    };
    let pane_id =
        match request_arg_str(&req, "pane_id").map(String::from).or_else(|| req.pane_id.clone()) {
            Some(pane_id) => pane_id,
            None => return Response::err("pane_id required"),
        };
    match identity.alias_manager.remove_member(
        &alias,
        request_arg_str(&req, "project_id"),
        &pane_id,
    ) {
        Ok(removed) => Response::success(serde_json::json!({ "removed": removed })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_alias_mode(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match canonical_user_alias_arg(&req) {
        Ok(alias) => alias,
        Err(response) => return response,
    };
    let mode = match request_arg_str(&req, "mode") {
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

    match identity.alias_manager.set_consumption_mode(
        &alias,
        request_arg_str(&req, "project_id"),
        mode,
    ) {
        Ok(alias) => Response::success(serde_json::to_value(alias).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

fn canonical_user_alias_arg(req: &Request) -> Result<String, Response> {
    let raw_alias = request_arg_str(req, "alias").ok_or_else(|| Response::err("alias required"))?;
    roux_core::validate_user_alias_name(raw_alias).map_err(|err| Response::err(err.to_string()))
}

fn alias_project_filter<'a>(project: Option<&'a str>, global: Option<bool>) -> ProjectFilter<'a> {
    match (project, global) {
        (Some(project), _) => ProjectFilter::Exact(Some(project)),
        (None, Some(true)) => ProjectFilter::Exact(None),
        (None, _) => ProjectFilter::Any,
    }
}

fn request_arg_str<'a>(req: &'a Request, key: &str) -> Option<&'a str> {
    req.args.get(key).and_then(|value| value.as_str())
}

fn request_arg_bool(req: &Request, key: &str) -> Option<bool> {
    req.args.get(key).and_then(|value| value.as_bool())
}

fn parse_event_kind(value: &str) -> Result<EventKind, String> {
    match value {
        "task" => Ok(EventKind::Task),
        "result" => Ok(EventKind::Result),
        "question" => Ok(EventKind::Question),
        "fyi" => Ok(EventKind::Fyi),
        "signal" => Ok(EventKind::Signal),
        other => Err(format!("invalid kind: {other}; expected task|result|question|fyi|signal")),
    }
}

fn resolve_recipient_alias(
    identity: &DaemonIdentity,
    req: &Request,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(alias) = explicit {
        return roux_core::validate_alias_name(alias).map_err(|err| err.to_string());
    }

    let mut candidates = Vec::new();
    if let Some(pane_id) = req.pane_id.as_deref() {
        candidates.extend(identity.alias_manager.find_for_pane(pane_id));
    }
    if candidates.is_empty() {
        if let Some(session_id) = req.session_id.as_deref() {
            candidates.extend(identity.alias_manager.whoami(session_id));
        }
    }

    match candidates.len() {
        0 => Err(format!(
            "no alias bound to {context}; claim one with `roux alias claim <name>` or pass args.alias",
            context = req
                .pane_id
                .as_deref()
                .map(|pane_id| format!("pane {pane_id}"))
                .or_else(|| req.session_id.as_deref().map(|session_id| format!("session {session_id}")))
                .unwrap_or_else(|| "this caller".to_string())
        )),
        1 => Ok(candidates[0].alias.clone()),
        _ => {
            let names: Vec<_> = candidates.iter().map(|alias| alias.alias.clone()).collect();
            Err(format!("caller holds multiple aliases ({names:?}); pass args.alias"))
        }
    }
}

fn default_mailbox_from(identity: &DaemonIdentity, req: &Request) -> Option<String> {
    if let Some(pane_id) = req.pane_id.as_deref() {
        let pane_aliases = identity.alias_manager.find_for_pane(pane_id);
        if pane_aliases.len() == 1 {
            return Some(pane_aliases[0].alias.clone());
        }
    }
    let session_id = req.session_id.as_deref()?;
    let mine = identity.alias_manager.whoami(session_id);
    if mine.len() == 1 {
        Some(mine[0].alias.clone())
    } else {
        Some(session_id.to_string())
    }
}

async fn handle_mailbox_post(req: Request, identity: &DaemonIdentity) -> Response {
    let body = match request_arg_str(&req, "body") {
        Some(body) => body.to_string(),
        None => return Response::err("body required"),
    };
    let to_raw = request_arg_str(&req, "to");
    let topic = request_arg_str(&req, "topic").map(String::from);
    if to_raw.is_none() && topic.is_none() {
        return Response::err("at least one of `to` or `topic` required");
    }
    let canonical_to = match to_raw {
        Some(raw) => match roux_core::validate_alias_name(raw) {
            Ok(alias) => Some(alias),
            Err(err) => return Response::err(err.to_string()),
        },
        None => None,
    };
    let kind = match request_arg_str(&req, "kind") {
        Some(kind) => match parse_event_kind(kind) {
            Ok(kind) => kind,
            Err(err) => return Response::err(err),
        },
        None => EventKind::Task,
    };
    let from = request_arg_str(&req, "from")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req));
    let project_id = request_arg_str(&req, "project_id").map(String::from);

    if let Some(alias) = &canonical_to {
        identity.alias_manager.ensure(alias, project_id.clone());
    }

    let mut builder = EventBuilder::new(body).kind(kind);
    if let Some(alias) = canonical_to {
        builder = builder.to(alias);
    }
    if let Some(topic) = topic {
        builder = builder.topic(topic);
    }
    if let Some(from) = from {
        builder = builder.from(from);
    }
    if let Some(project_id) = project_id {
        builder = builder.project_id(project_id);
    }
    if let Some(subject) = request_arg_str(&req, "subject") {
        builder = builder.subject(subject.to_string());
    }
    if let Some(correlation_id) = request_arg_str(&req, "correlation_id") {
        builder = builder.correlation_id(correlation_id.to_string());
    }
    if let Some(structured) = req.args.get("structured").cloned().filter(|value| !value.is_null()) {
        builder = builder.structured(structured);
    }

    match identity.mailbox_manager.post(builder) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_mailbox_peek(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let mut events = identity.mailbox_manager.list_for_recipient(
        &alias,
        request_arg_bool(&req, "unread").unwrap_or(false),
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    if let Some(limit) = req.args.get("limit").and_then(|value| value.as_u64()) {
        events.truncate(limit as usize);
    }
    Response::success(serde_json::to_value(events).unwrap_or_default())
}

async fn handle_mailbox_read(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let mut events = identity.mailbox_manager.list_for_recipient(
        &alias,
        true,
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    if let Some(limit) = req.args.get("limit").and_then(|value| value.as_u64()) {
        events.truncate(limit as usize);
    }
    for event in &events {
        identity.mailbox_manager.mark_read(&event.id, &alias);
        if request_arg_bool(&req, "ack").unwrap_or(false) {
            identity.mailbox_manager.ack(&event.id, &alias, None);
        }
    }
    Response::success(serde_json::to_value(events).unwrap_or_default())
}

async fn handle_mailbox_get(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id,
        None => return Response::err("event_id required"),
    };
    Response::success(
        serde_json::to_value(identity.mailbox_manager.get(event_id)).unwrap_or_default(),
    )
}

async fn handle_mailbox_read_state(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id,
        None => return Response::err("event_id required"),
    };
    let recipient = match request_arg_str(&req, "recipient") {
        Some(recipient) => recipient,
        None => return Response::err("recipient required"),
    };
    Response::success(
        serde_json::to_value(identity.mailbox_manager.read_state(event_id, recipient))
            .unwrap_or_default(),
    )
}

async fn handle_mailbox_mark_read(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let recipient = match request_arg_str(&req, "recipient") {
        Some(recipient) => recipient.to_string(),
        None => return Response::err("recipient required"),
    };
    let changed = identity.mailbox_manager.mark_read(&event_id, &recipient);
    Response::success(serde_json::json!({ "changed": changed }))
}

async fn handle_mailbox_ack(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let changed = identity.mailbox_manager.ack(
        &event_id,
        &alias,
        request_arg_str(&req, "result").map(String::from),
    );
    Response::success(serde_json::json!({ "changed": changed }))
}

async fn handle_mailbox_retract(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    match identity.mailbox_manager.retract(&event_id, &alias) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_mailbox_dismiss(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let changed = identity.mailbox_manager.dismiss(&event_id, &alias);
    Response::success(serde_json::json!({ "changed": changed }))
}

async fn handle_mailbox_count(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let count = identity.mailbox_manager.unread_count(
        &alias,
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    Response::success(serde_json::json!({ "unread": count }))
}

async fn handle_mailbox_clear(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let cleared = identity.mailbox_manager.clear_read(
        &alias,
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    Response::success(serde_json::json!({ "cleared": cleared }))
}

async fn handle_mailbox_reply(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let body = match request_arg_str(&req, "body") {
        Some(body) => body.to_string(),
        None => return Response::err("body required"),
    };
    let original = match identity.mailbox_manager.get(&event_id) {
        Some(event) => event,
        None => return Response::err(format!("event_id not found: {event_id}")),
    };
    let recipient = match original.from.as_deref() {
        Some(sender) => sender.to_string(),
        None => {
            return Response::err("cannot reply: original event has no `from` (anonymous sender)")
        }
    };
    let canonical_to = roux_core::validate_alias_name(&recipient).ok().unwrap_or(recipient);
    let correlation_id = original.correlation_id.clone().unwrap_or_else(|| original.id.clone());
    let kind = match request_arg_str(&req, "kind") {
        Some(kind) => match parse_event_kind(kind) {
            Ok(kind) => kind,
            Err(err) => return Response::err(err),
        },
        None => EventKind::Result,
    };

    let mut builder =
        EventBuilder::new(body).to(canonical_to.clone()).kind(kind).correlation_id(correlation_id);
    if let Some(from) = request_arg_str(&req, "from")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req))
    {
        builder = builder.from(from);
    }
    if let Some(subject) = request_arg_str(&req, "subject") {
        builder = builder.subject(subject.to_string());
    }
    if let Some(project_id) = original.project_id {
        builder = builder.project_id(project_id);
    }
    if let Some(structured) = req.args.get("structured").cloned().filter(|value| !value.is_null()) {
        builder = builder.structured(structured);
    }

    identity.alias_manager.ensure(&canonical_to, builder.project_id.clone());
    match identity.mailbox_manager.post(builder) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_mailbox_sent(req: Request, identity: &DaemonIdentity) -> Response {
    let sender = match request_arg_str(&req, "sender")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req))
    {
        Some(sender) => sender,
        None => return Response::err("sender required (call from a session, or pass args.sender)"),
    };
    let limit = req.args.get("limit").and_then(|value| value.as_u64()).map(|value| value as usize);
    let payload: Vec<Value> = identity
        .mailbox_manager
        .list_sent_by(&sender, request_arg_str(&req, "to"), limit)
        .into_iter()
        .map(|(event, state)| serde_json::json!({ "event": event, "state": state }))
        .collect();
    Response::success(Value::Array(payload))
}

async fn handle_bus_publish(req: Request, identity: &DaemonIdentity) -> Response {
    let topic = match request_arg_str(&req, "topic") {
        Some(topic) => topic.to_string(),
        None => return Response::err("topic required"),
    };
    let body = request_arg_str(&req, "body").unwrap_or("").to_string();
    let structured = req.args.get("structured").cloned();
    if body.trim().is_empty() && structured.as_ref().map(|value| value.is_null()).unwrap_or(true) {
        return Response::err("body or structured payload required");
    }
    let kind = match request_arg_str(&req, "kind") {
        Some(kind) => match parse_event_kind(kind) {
            Ok(kind) => kind,
            Err(err) => return Response::err(err),
        },
        None => EventKind::Signal,
    };
    let mut builder = EventBuilder::new(body).topic(topic).kind(kind);
    if let Some(from) = request_arg_str(&req, "from")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req))
    {
        builder = builder.from(from);
    }
    if let Some(project_id) = request_arg_str(&req, "project_id") {
        builder = builder.project_id(project_id.to_string());
    }
    if let Some(subject) = request_arg_str(&req, "subject") {
        builder = builder.subject(subject.to_string());
    }
    if let Some(structured) = structured.filter(|value| !value.is_null()) {
        builder = builder.structured(structured);
    }
    match identity.mailbox_manager.post(builder) {
        Ok(event) => Response::success(serde_json::to_value(event).unwrap_or_default()),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_bus_tail(req: Request, identity: &DaemonIdentity) -> Response {
    let limit = req.args.get("limit").and_then(|value| value.as_u64()).map(|value| value as usize);
    let filter =
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global"));
    let events = match request_arg_str(&req, "topic") {
        Some(topic) => {
            let mut events = identity.mailbox_manager.list_for_topic(topic, filter);
            if let Some(limit) = limit {
                events.truncate(limit);
            }
            events
        }
        None => identity.mailbox_manager.list_all(filter, limit),
    };
    Response::success(serde_json::to_value(events).unwrap_or_default())
}

fn resolve_subscriber_alias(identity: &DaemonIdentity, req: &Request) -> Result<String, String> {
    if let Some(alias) = request_arg_str(req, "alias") {
        return Ok(alias.to_string());
    }
    let pane_id = req.pane_id.as_deref().ok_or_else(|| {
        "no --alias given and no pane context available; pass --alias <name>".to_string()
    })?;
    let held = identity.alias_manager.find_for_pane(pane_id);
    match held.len() {
        0 => {
            Err("no --alias given and the calling pane holds no alias; pass --alias <name>"
                .to_string())
        }
        1 => Ok(held[0].alias.clone()),
        _ => {
            let names: Vec<_> = held.iter().map(|alias| alias.alias.as_str()).collect();
            Err(format!(
                "no --alias given and the calling pane holds multiple aliases ({names:?}); pass --alias <name>"
            ))
        }
    }
}

async fn handle_bus_subscribe(req: Request, identity: &DaemonIdentity) -> Response {
    let pattern = match request_arg_str(&req, "pattern") {
        Some(pattern) if !pattern.trim().is_empty() => pattern.to_string(),
        _ => return Response::err("pattern required"),
    };
    let alias = match resolve_subscriber_alias(identity, &req) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    match identity.subscription_manager.subscribe(
        &alias,
        &pattern,
        request_arg_str(&req, "project_id").map(String::from),
    ) {
        Ok(subscription) => {
            Response::success(serde_json::to_value(subscription).unwrap_or_default())
        }
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_bus_unsubscribe(req: Request, identity: &DaemonIdentity) -> Response {
    let id = match request_arg_str(&req, "id") {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => return Response::err("subscription id required"),
    };
    match identity.subscription_manager.unsubscribe(&id) {
        Ok(removed) => Response::success(serde_json::json!({ "removed": removed })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_bus_subscriptions(req: Request, identity: &DaemonIdentity) -> Response {
    let filter =
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global"));
    let subscriptions = match request_arg_str(&req, "alias") {
        Some(alias) => identity.subscription_manager.for_alias(alias, filter),
        None => identity.subscription_manager.list(filter),
    };
    Response::success(serde_json::to_value(subscriptions).unwrap_or_default())
}

async fn handle_cli_session_create(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    if req.args.get("prompt").and_then(|prompt| prompt.as_str()).is_some() {
        return Response::err(
            "daemon session create does not support --prompt until a frontend attaches; create the session, attach, then send input",
        );
    }
    let normalized = match normalize_cli_session_create_request(req, host).await {
        Ok(req) => req,
        Err(response) => return response,
    };

    let create = handle_session_create_shell(normalized, host, identity).await;
    if !create.ok {
        return create;
    }

    let Some(session_id) = create
        .data
        .as_ref()
        .and_then(|data| data.get("id"))
        .and_then(|id| id.as_str())
        .map(str::to_string)
    else {
        return Response::err("session created but response did not include id");
    };

    Response::success(serde_json::json!({ "session_id": session_id }))
}

async fn normalize_cli_session_create_request(
    mut req: Request,
    host: &RuntimeHost,
) -> Result<Request, Response> {
    if req
        .args
        .get("flags")
        .and_then(|flags| flags.as_array())
        .is_some_and(|flags| !flags.is_empty())
    {
        return Err(Response::err(
            "--flag/-f is not supported by daemon session create; bake flags into a spawn profile's startup command instead",
        ));
    }
    let mut args = req.args.as_object().cloned().unwrap_or_default();
    let working_dir = args
        .get("working_dir")
        .or_else(|| args.get("workingDir"))
        .and_then(|working_dir| working_dir.as_str())
        .filter(|working_dir| !working_dir.trim().is_empty())
        .map(str::to_string);

    let mut repo_path = args
        .get("repoPath")
        .or_else(|| args.get("repo_path"))
        .and_then(|repo_path| repo_path.as_str())
        .filter(|repo_path| !repo_path.trim().is_empty())
        .map(str::to_string)
        .or(working_dir);

    if repo_path.is_none() {
        if let Some(session_id) = req.session_id.as_deref() {
            match host.session_handle.get(session_id).await {
                Ok(Some(session)) => repo_path = Some(session.repo_root),
                Ok(None) => {}
                Err(err) => return Err(Response::err(err.to_string())),
            }
        }
    }

    let Some(repo_path) = repo_path else {
        return Err(Response::err("working_dir, repoPath, or session_id required"));
    };
    args.insert("repoPath".to_string(), Value::String(repo_path));

    if let Some(branch) = args.remove("worktree_branch").or_else(|| args.remove("worktreeBranch")) {
        args.insert("branch".to_string(), branch);
    }
    if let Some(start_point) = args.remove("start_point").or_else(|| args.remove("startPoint")) {
        let fetch_first = start_point.as_str().is_some_and(|start| start.starts_with("origin/"));
        args.insert("base".to_string(), start_point);
        args.entry("fetchFirst".to_string()).or_insert(Value::Bool(fetch_first));
    }
    args.entry("profile".to_string()).or_insert(Value::String("claude".to_string()));

    req.args = Value::Object(args);
    Ok(req)
}

async fn handle_session_create_shell(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(repo_path) = req
        .args
        .get("repoPath")
        .or_else(|| req.args.get("repo_path"))
        .and_then(|repo_path| repo_path.as_str())
    else {
        return Response::err("repoPath required");
    };
    let name = req.args.get("name").and_then(|name| name.as_str()).unwrap_or("New Session");
    let id = req
        .args
        .get("id")
        .and_then(|id| id.as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    match host.session_handle.get(&id).await {
        Ok(Some(_)) => return Response::err(format!("session {id} already exists")),
        Ok(None) => {}
        Err(err) => return Response::err(err.to_string()),
    }

    let settings = load_daemon_settings();
    let target = parse_daemon_session_target(&req.args);
    let (work_dir, actual_branch, owns_worktree) =
        match resolve_daemon_session_target(repo_path, target, &settings, daemon_hook_manager())
            .await
        {
            Ok(resolved) => resolved,
            Err(err) => return Response::err(err),
        };

    let pane_id = format!("{id}-main");
    let profile = req.args.get("profile").and_then(|profile| profile.as_str()).map(str::to_string);
    let initial_size = parse_initial_size(&req.args);
    let project_id = req
        .args
        .get("projectId")
        .or_else(|| req.args.get("project_id"))
        .and_then(|project_id| project_id.as_str())
        .map(str::to_string);
    let blueprint_id = req
        .args
        .get("blueprintId")
        .or_else(|| req.args.get("blueprint_id"))
        .and_then(|blueprint_id| blueprint_id.as_str())
        .map(str::to_string);
    let smol_machine_name = req
        .args
        .get("smolMachineName")
        .or_else(|| req.args.get("smol_machine_name"))
        .and_then(|smol| smol.as_str())
        .map(str::trim)
        .filter(|smol| !smol.is_empty())
        .map(str::to_string);
    let nono = parse_nono_config(&req.args);
    let smolvm = match build_daemon_smolvm_exec_for_session(&settings, smol_machine_name.as_deref())
    {
        Ok(smolvm) => smolvm,
        Err(err) => return Response::err(err),
    };

    let spawn = host
        .pty_handle
        .spawn_shell(PtySpawnRequest {
            id: Some(id.clone()),
            working_dir: Some(PathBuf::from(&work_dir)),
            session_id: Some(id.clone()),
            pane_id: Some(pane_id),
            project_id: project_id.clone(),
            worktree_path: owns_worktree.then(|| work_dir.clone()),
            notes: parse_notes_env(&req.args),
            nono,
            smolvm,
            env: parse_pty_env_request(&req.args, identity),
            profile: profile.clone(),
            initial_size,
            role: roux_core::PtyRole::SessionPrimary,
        })
        .await;
    if let Err(err) = spawn {
        if owns_worktree {
            let _ = roux_core::remove_worktree(repo_path, &work_dir);
        }
        return Response::err(err.to_string());
    }

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let session = roux_core::Session {
        id: id.clone(),
        name: name.to_string(),
        repo_root: repo_path.to_string(),
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: owns_worktree,
        status: roux_core::SessionStatus::Idle,
        model: None,
        cost: None,
        created_at: now,
        project_id,
        is_git_repo: is_git_repo(repo_path),
        name_override: None,
        primary_pty_id: Some(id.clone()),
        archived: false,
        ended_at: None,
        blueprint_id,
        pinned_pr_url: None,
        smol_machine_name,
    };

    if let Err(err) = host.session_handle.add(session.clone()).await {
        let _ = host.pty_handle.kill(&id).await;
        if session.is_worktree {
            let _ = roux_core::remove_worktree(&session.repo_root, &session.worktree_path);
        }
        return Response::err(err.to_string());
    }

    match serde_json::to_value(session) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize session: {err}")),
    }
}

async fn handle_session_reconnect_shell(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let session = match host.session_handle.get(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    };
    let settings = load_daemon_settings();
    let smolvm =
        match build_daemon_smolvm_exec_for_session(&settings, session.smol_machine_name.as_deref())
        {
            Ok(smolvm) => smolvm,
            Err(err) => return Response::err(err),
        };
    let primary_pty_id = session.primary_pty_id.as_deref().unwrap_or(&session.id).to_string();
    let _ = host.pty_handle.remove(&primary_pty_id).await;
    let pane_id = format!("{}-main", session.id);
    let initial_size = parse_initial_size(&req.args);
    let profile = req.args.get("profile").and_then(|profile| profile.as_str()).map(str::to_string);
    let spawn = host
        .pty_handle
        .spawn_shell(PtySpawnRequest {
            id: Some(session.id.clone()),
            working_dir: Some(PathBuf::from(&session.worktree_path)),
            session_id: Some(session.id.clone()),
            pane_id: Some(pane_id),
            project_id: session.project_id.clone(),
            worktree_path: session.is_worktree.then(|| session.worktree_path.clone()),
            notes: parse_notes_env(&req.args),
            nono: parse_nono_config(&req.args),
            smolvm,
            env: parse_pty_env_request(&req.args, identity),
            profile,
            initial_size,
            role: roux_core::PtyRole::SessionPrimary,
        })
        .await;
    if let Err(err) = spawn {
        return Response::err(err.to_string());
    }

    if let Err(err) =
        host.session_handle.update_status(&session.id, roux_core::SessionStatus::Idle).await
    {
        let _ = host.pty_handle.kill(&session.id).await;
        return Response::err(err.to_string());
    }
    match host.session_handle.get(&session.id).await {
        Ok(Some(updated)) => serialize_session(updated),
        Ok(None) => Response::err("session not found after reconnect"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_archive(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    kill_session_ptys(host, session_id).await;
    if let Err(err) = host.session_handle.archive(session_id).await {
        return Response::err(err.to_string());
    }
    match host.session_handle.get(session_id).await {
        Ok(Some(session)) => serialize_session(session),
        Ok(None) => Response::err("session not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_restore(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    if let Err(err) = host.session_handle.restore(session_id).await {
        return Response::err(err.to_string());
    }
    if let Err(err) =
        host.session_handle.update_status(session_id, roux_core::SessionStatus::Disconnected).await
    {
        return Response::err(err.to_string());
    }
    match host.session_handle.get(session_id).await {
        Ok(Some(session)) => serialize_session(session),
        Ok(None) => Response::err("session not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_delete(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    kill_session_ptys(host, session_id).await;
    if let Err(err) = host.session_handle.remove(session_id).await {
        return Response::err(err.to_string());
    }
    Response::success(serde_json::json!({ "session_id": session_id }))
}

async fn handle_session_worktree_exists(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    match host.session_handle.get(session_id).await {
        Ok(Some(session)) => Response::success(serde_json::json!({
            "session_id": session_id,
            "exists": Path::new(&session.worktree_path).exists(),
        })),
        Ok(None) => Response::err("session not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_session_refresh_branch(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = request_session_id(&req) else {
        return Response::err("session_id required");
    };
    let session = match host.session_handle.get(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    };
    let is_git_repo = is_git_repo(&session.worktree_path);
    if is_git_repo != session.is_git_repo {
        if let Err(err) = host.session_handle.set_git_repo(session_id, is_git_repo).await {
            return Response::err(err.to_string());
        }
    }
    if !is_git_repo {
        return Response::success(serde_json::json!({ "branch": session.branch }));
    }
    let branch = get_current_branch(&session.worktree_path)
        .filter(|branch| !branch.is_empty())
        .unwrap_or(session.branch);
    if let Err(err) = host.session_handle.set_branch(session_id, branch.clone()).await {
        return Response::err(err.to_string());
    }
    Response::success(serde_json::json!({ "branch": branch }))
}

async fn kill_session_ptys(host: &RuntimeHost, session_id: &str) {
    let ptys = host.pty_handle.list().await.unwrap_or_default();
    for pty in ptys {
        if pty.info.session_id.as_deref() == Some(session_id) {
            let _ = host.pty_handle.remove(&pty.id).await;
        }
    }
}

fn serialize_session(session: roux_core::Session) -> Response {
    match serde_json::to_value(session) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize session: {err}")),
    }
}

fn daemon_hook_manager() -> AutomationHookManager {
    AutomationHookManager::from_config_root(platform::app_config_dir())
}

async fn handle_hook_show(req: Request) -> Response {
    handle_hook_show_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_show_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let repo_path = req
        .args
        .get("repoPath")
        .or_else(|| req.args.get("repo_path"))
        .and_then(|value| value.as_str());
    match hooks.list_hooks(repo_path) {
        Ok(items) => Response::success(hook_list_to_value(items)),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_preview(req: Request) -> Response {
    handle_hook_preview_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_preview_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let request = match request_from_socket_args(req.args) {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    let settings = load_daemon_settings();
    let wt_available = resolve_wt_binary(&settings).is_some();
    let (event, context) =
        match context_from_run_request(request, Some(settings.worktree_provider), wt_available) {
            Ok(parts) => parts,
            Err(err) => return Response::err(err.to_string()),
        };
    match hooks.preview(event, &context) {
        Ok(items) => match serde_json::to_value(items) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize hook preview: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_run(req: Request) -> Response {
    handle_hook_run_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_run_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let request = match request_from_socket_args(req.args) {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    let settings = load_daemon_settings();
    let wt_available = resolve_wt_binary(&settings).is_some();
    let (event, context) =
        match context_from_run_request(request, Some(settings.worktree_provider), wt_available) {
            Ok(parts) => parts,
            Err(err) => return Response::err(err.to_string()),
        };
    let result = if event.is_blocking() {
        hooks.run_blocking(event, context).await
    } else {
        hooks.run_background(event, context).await
    };
    match result {
        Ok(ran) => Response::success(hook_run_to_value(HookRunSummary {
            event: event.as_str().into(),
            ran,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_approve(req: Request) -> Response {
    handle_hook_approve_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_approve_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(approval_id) = req
        .args
        .get("approvalId")
        .or_else(|| req.args.get("approval_id"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return Response::err("approvalId required");
    };
    match hooks.approve(approval_id) {
        Ok(()) => Response::success(Value::Null),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_clear_approvals() -> Response {
    handle_hook_clear_approvals_with_hooks(daemon_hook_manager()).await
}

async fn handle_hook_clear_approvals_with_hooks(hooks: AutomationHookManager) -> Response {
    match hooks.clear_approvals() {
        Ok(()) => Response::success(Value::Null),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_hook_log_list() -> Response {
    handle_hook_log_list_with_hooks(daemon_hook_manager()).await
}

async fn handle_hook_log_list_with_hooks(hooks: AutomationHookManager) -> Response {
    match serde_json::to_value(hooks.list_logs()) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize hook logs: {err}")),
    }
}

async fn handle_hook_log_read(req: Request) -> Response {
    handle_hook_log_read_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_hook_log_read_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(path) = req
        .args
        .get("path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return Response::err("path required");
    };
    match hooks.read_log(path) {
        Ok(content) => Response::success(Value::String(content)),
        Err(err) => Response::err(err.to_string()),
    }
}

fn build_daemon_post_worktree_create_context(
    provider: roux_core::WorktreeProvider,
    wt_available: bool,
    repo_path: &str,
    branch: &str,
    worktree_path: &str,
) -> HookContext {
    let mut context =
        HookContext::new(HookEvent::PostWorktreeCreate).with_provider(provider, wt_available);
    context.repo_path = Some(repo_path.to_string());
    context.worktree_path = Some(worktree_path.to_string());
    context.branch = Some(branch.to_string());
    context.cwd = Some(worktree_path.to_string());
    context.provider_hooks_ran =
        worktree_provider_hooks(HookEvent::PostWorktreeCreate, context.worktrunk);
    context
}

fn build_daemon_post_worktree_remove_context(
    provider: roux_core::WorktreeProvider,
    wt_available: bool,
    repo_path: &str,
    worktree_path: &str,
) -> HookContext {
    let mut context =
        HookContext::new(HookEvent::PostWorktreeRemove).with_provider(provider, wt_available);
    context.repo_path = Some(repo_path.to_string());
    context.worktree_path = Some(worktree_path.to_string());
    context.cwd = Some(repo_path.to_string());
    context.provider_hooks_ran =
        worktree_provider_hooks(HookEvent::PostWorktreeRemove, context.worktrunk);
    context
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

async fn handle_project_create(req: Request, host: &RuntimeHost) -> Response {
    let Some(name) = optional_string_arg(&req.args, &["name"]) else {
        return Response::err("name required");
    };
    let id =
        optional_string_arg(&req.args, &["id"]).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let project = Project {
        id,
        name,
        repo_roots: Vec::new(),
        context_paths: Vec::new(),
        session_blueprints: Vec::new(),
        project_prompt: String::new(),
    };
    match host.project_handle.add(project.clone()).await {
        Ok(()) => serialize_project(project),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_project_remove(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id", "projectId", "project_id"]) else {
        return Response::err("id required");
    };
    let removed = match host.project_handle.get(&id).await {
        Ok(project) => project,
        Err(err) => return Response::err(err.to_string()),
    };
    if let Err(err) = host.project_handle.remove(&id).await {
        return Response::err(err.to_string());
    }
    if let Err(err) = host.session_handle.clear_project_refs(&id).await {
        if let Some(project) = removed {
            let _ = host.project_handle.add(project).await;
        }
        return Response::err(err.to_string());
    }
    Response::success(serde_json::json!({ "id": id }))
}

async fn handle_project_rename(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id", "projectId", "project_id"]) else {
        return Response::err("id required");
    };
    let Some(name) = optional_string_arg(&req.args, &["name"]) else {
        return Response::err("name required");
    };
    match host.project_handle.rename(&id, &name).await {
        Ok(()) => Response::success(serde_json::json!({ "id": id, "name": name })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_project_update(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id", "projectId", "project_id"]) else {
        return Response::err("id required");
    };
    let patch_value = req.args.get("patch").cloned().unwrap_or_else(|| req.args.clone());
    let patch: ProjectUpdate = match serde_json::from_value(patch_value) {
        Ok(patch) => patch,
        Err(err) => return Response::err(format!("invalid project patch: {err}")),
    };
    match host.project_handle.update(&id, patch).await {
        Ok(Some(project)) => serialize_project(project),
        Ok(None) => Response::err(format!("project {id} not found")),
        Err(err) => Response::err(err.to_string()),
    }
}

fn serialize_project(project: Project) -> Response {
    match serde_json::to_value(project) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize project: {err}")),
    }
}

async fn handle_watch_list(host: &RuntimeHost) -> Response {
    match host.watch_handle.list().await {
        Ok(watches) => match serde_json::to_value(watches) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize watches: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_watch_create(req: Request, watch_runner: &WatchRunner) -> Response {
    let config = match parse_watch_config(&req) {
        Ok(config) => config,
        Err(err) => return Response::err(err),
    };
    let watch = watch_from_config(config);
    match watch_runner.add_watch(watch).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_watch_find_or_create(req: Request, watch_runner: &WatchRunner) -> Response {
    let config = match parse_watch_config(&req) {
        Ok(config) => config,
        Err(err) => return Response::err(err),
    };
    let watch = watch_from_config(config);
    match watch_runner.find_or_add_github_pr(watch).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_watch_remove(req: Request, watch_runner: &WatchRunner) -> Response {
    let Some(id) = request_watch_id(&req) else {
        return Response::err("id required");
    };
    let id = id.to_string();
    match watch_runner.remove_watch(&id).await {
        Ok(()) => Response::success(serde_json::json!({ "id": id })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_watch_pause(req: Request, watch_runner: &WatchRunner) -> Response {
    let Some(id) = request_watch_id(&req) else {
        return Response::err("id required");
    };
    match watch_runner.pause_watch(id).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err),
    }
}

async fn handle_watch_resume(req: Request, watch_runner: &WatchRunner) -> Response {
    let Some(id) = request_watch_id(&req) else {
        return Response::err("id required");
    };
    match watch_runner.resume_watch(id).await {
        Ok(watch) => serialize_watch(watch),
        Err(err) => Response::err(err),
    }
}

async fn handle_watch_replace(req: Request, watch_runner: &WatchRunner) -> Response {
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

async fn handle_watch_remove_for_session(req: Request, watch_runner: &WatchRunner) -> Response {
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

async fn handle_watch_cleanup_orphans(host: &RuntimeHost, watch_runner: &WatchRunner) -> Response {
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

fn parse_watch_config(req: &Request) -> Result<CreateWatchConfig, String> {
    let value = req.args.get("config").cloned().unwrap_or_else(|| req.args.clone());
    serde_json::from_value(value).map_err(|err| format!("invalid watch config: {err}"))
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

async fn handle_worktree_list(req: Request) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let repo_path = repo_path.to_string();
    let settings = load_daemon_settings();
    match tokio::task::spawn_blocking(move || {
        let wt = resolve_wt_binary(&settings);
        roux_core::list_worktrees_enriched(&repo_path, wt.as_ref()).map_err(|err| err.to_string())
    })
    .await
    {
        Ok(Ok(worktrees)) => match serde_json::to_value(worktrees) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize worktrees: {err}")),
        },
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-list task failed: {err}")),
    }
}

async fn handle_worktree_create(req: Request) -> Response {
    handle_worktree_create_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_worktree_create_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let Some(branch) = req.args.get("branch").and_then(|branch| branch.as_str()) else {
        return Response::err("branch required");
    };
    let repo_path = repo_path.to_string();
    let branch = branch.to_string();
    let start_point = optional_string_arg(&req.args, &["startPoint", "start_point", "base"]);
    let base_path = optional_string_arg(&req.args, &["basePath", "base_path"]);
    let fetch_first = bool_arg(&req.args, &["fetchFirst", "fetch_first"]).unwrap_or(false);
    let settings = load_daemon_settings();
    let provider = settings.worktree_provider;
    let wt = resolve_wt_binary(&settings);
    let wt_available = wt.is_some();
    let pre_context = HookContext {
        repo_path: Some(repo_path.clone()),
        branch: Some(branch.clone()),
        cwd: Some(repo_path.clone()),
        ..HookContext::new(HookEvent::PreWorktreeCreate).with_provider(provider, wt_available)
    };
    if let Err(err) = hooks.run_blocking(HookEvent::PreWorktreeCreate, pre_context).await {
        return Response::err(err.to_string());
    }
    let post_hooks = hooks.clone();
    let post_repo_path = repo_path.clone();
    let post_branch = branch.clone();

    match tokio::task::spawn_blocking(move || {
        if fetch_first {
            roux_core::fetch_origin(&repo_path).map_err(|err| err.to_string())?;
        }
        let base_path = base_path.as_deref().or(settings.worktree_base_path.as_deref());
        roux_core::create_worktree_with_provider(
            &repo_path,
            &branch,
            base_path,
            start_point.as_deref(),
            provider,
            wt.as_ref(),
        )
        .map_err(|err| err.to_string())
    })
    .await
    {
        Ok(Ok(path)) => {
            let context = build_daemon_post_worktree_create_context(
                provider,
                wt_available,
                &post_repo_path,
                &post_branch,
                &path,
            );
            post_hooks.spawn_background(HookEvent::PostWorktreeCreate, context);
            Response::success(serde_json::json!({ "path": path }))
        }
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-create task failed: {err}")),
    }
}

async fn handle_worktree_remove(req: Request) -> Response {
    handle_worktree_remove_with_hooks(req, daemon_hook_manager()).await
}

async fn handle_worktree_remove_with_hooks(req: Request, hooks: AutomationHookManager) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let Some(worktree_path) = req
        .args
        .get("worktreePath")
        .or_else(|| req.args.get("worktree_path"))
        .and_then(|path| path.as_str())
    else {
        return Response::err("worktreePath required");
    };
    let repo_path = repo_path.to_string();
    let worktree_path = worktree_path.to_string();
    let response_repo_path = repo_path.clone();
    let response_worktree_path = worktree_path.clone();
    let also_branch = bool_arg(&req.args, &["alsoBranch", "also_branch"]).unwrap_or(false);
    let force = bool_arg(&req.args, &["force"]).unwrap_or(false);
    let settings = load_daemon_settings();
    let provider = settings.worktree_provider;
    let wt = resolve_wt_binary(&settings);
    let wt_available = wt.is_some();
    let pre_context = HookContext {
        repo_path: Some(repo_path.clone()),
        worktree_path: Some(worktree_path.clone()),
        cwd: Some(worktree_path.clone()),
        ..HookContext::new(HookEvent::PreWorktreeRemove).with_provider(provider, wt_available)
    };
    if let Err(err) = hooks.run_blocking(HookEvent::PreWorktreeRemove, pre_context).await {
        return Response::err(err.to_string());
    }
    let post_hooks = hooks.clone();

    match tokio::task::spawn_blocking(move || {
        roux_core::remove_worktree_with_provider(
            &repo_path,
            &worktree_path,
            also_branch,
            force,
            provider,
            wt.as_ref(),
        )
        .map_err(|err| err.to_string())
    })
    .await
    {
        Ok(Ok(())) => {
            let context = build_daemon_post_worktree_remove_context(
                provider,
                wt_available,
                &response_repo_path,
                &response_worktree_path,
            );
            post_hooks.spawn_background(HookEvent::PostWorktreeRemove, context);
            Response::success(serde_json::json!({
                "repoPath": response_repo_path,
                "worktreePath": response_worktree_path,
            }))
        }
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-remove task failed: {err}")),
    }
}

async fn handle_worktree_list_branches(req: Request) -> Response {
    let Some(repo_path) = request_repo_path(&req) else {
        return Response::err("repoPath required");
    };
    let repo_path = repo_path.to_string();
    match tokio::task::spawn_blocking(move || list_branches(&repo_path)).await {
        Ok(Ok(branches)) => match serde_json::to_value(branches) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize branches: {err}")),
        },
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("worktree-list-branches task failed: {err}")),
    }
}

async fn handle_git_init(req: Request) -> Response {
    let Some(path) = req.args.get("path").and_then(|path| path.as_str()) else {
        return Response::err("path required");
    };
    let path = path.to_string();
    let response_path = path.clone();
    match tokio::task::spawn_blocking(move || git_init(&path)).await {
        Ok(Ok(())) => Response::success(serde_json::json!({ "path": response_path })),
        Ok(Err(err)) => Response::err(err),
        Err(err) => Response::err(format!("git-init task failed: {err}")),
    }
}

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

async fn handle_notes_read(req: Request, host: &RuntimeHost) -> Response {
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

async fn handle_notes_write(req: Request, host: &RuntimeHost) -> Response {
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

async fn handle_notes_append(req: Request, host: &RuntimeHost) -> Response {
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

async fn handle_notes_path(req: Request, host: &RuntimeHost) -> Response {
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

async fn handle_notes_search(req: Request) -> Response {
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

async fn handle_notes_vault_root(req: Request) -> Response {
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

async fn handle_cli_send(req: Request, host: &RuntimeHost) -> Response {
    let Some(text) = req.args.get("text").and_then(|text| text.as_str()) else {
        return Response::err("text required");
    };
    let enter = req.args.get("enter").and_then(|enter| enter.as_bool()).unwrap_or(true);
    let mut data = text.as_bytes().to_vec();
    if enter {
        data.push(b'\r');
    }

    let pty_id = match resolve_cli_send_pty_id(&req, host).await {
        Ok(pty_id) => pty_id,
        Err(response) => return response,
    };
    match host.pty_handle.write(&pty_id, data.clone()).await {
        Ok(()) => Response::success(serde_json::json!({ "id": pty_id, "bytes": data.len() })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_latest_output(req: Request, host: &RuntimeHost) -> Response {
    let max_bytes = latest_output_max_bytes(&req.args);
    let pty_id = match resolve_latest_output_pty_id(&req, host).await {
        Ok(pty_id) => pty_id,
        Err(response) => return response,
    };
    match host.pty_handle.snapshot(&pty_id, max_bytes).await {
        Ok(Some(snapshot)) => {
            let pane_id = req.pane_id.clone().or_else(|| daemon_record_pane_id(&snapshot.record));
            Response::success(latest_output_payload(
                snapshot.record.info.session_id,
                pane_id,
                snapshot.record.id,
                max_bytes,
                &snapshot.output_bytes,
            ))
        }
        Ok(None) => Response::err(format!("pty not found: {pty_id}")),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn resolve_latest_output_pty_id(
    req: &Request,
    host: &RuntimeHost,
) -> Result<String, Response> {
    let ptys = host.pty_handle.list().await.map_err(|err| Response::err(err.to_string()))?;

    if let Some(pane_id) = req.pane_id.as_deref().filter(|pane_id| !pane_id.trim().is_empty()) {
        return ptys
            .iter()
            .find(|pty| {
                pty_matches_pane(pty, pane_id)
                    && req
                        .session_id
                        .as_deref()
                        .is_none_or(|session_id| pty.info.session_id.as_deref() == Some(session_id))
            })
            .map(|pty| pty.id.clone())
            .ok_or_else(|| Response::err(format!("daemon PTY not found for pane {pane_id}")));
    }

    resolve_cli_send_pty_id(req, host).await
}

fn latest_output_max_bytes(args: &Value) -> usize {
    args.get("max_bytes")
        .or_else(|| args.get("maxBytes"))
        .and_then(|value| value.as_u64())
        .map(|bytes| (bytes as usize).clamp(1, MAX_LATEST_OUTPUT_BYTES))
        .unwrap_or(DEFAULT_LATEST_OUTPUT_BYTES)
}

fn latest_output_payload(
    session_id: Option<String>,
    pane_id: Option<String>,
    pty_id: String,
    max_bytes: usize,
    bytes: &[u8],
) -> Value {
    let mut data = serde_json::Map::new();
    data.insert("session_id".into(), optional_string_value(session_id));
    data.insert("pane_id".into(), optional_string_value(pane_id));
    data.insert("pty_id".into(), Value::String(pty_id));
    data.insert("max_bytes".into(), Value::Number(max_bytes.into()));
    data.insert("byte_count".into(), Value::Number(bytes.len().into()));
    data.insert("replay_bytes_base64".into(), Value::String(BASE64_STANDARD.encode(bytes)));
    if let Ok(text) = std::str::from_utf8(bytes) {
        data.insert("text".into(), Value::String(text.to_string()));
    }
    Value::Object(data)
}

fn optional_string_value(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

async fn resolve_cli_send_pty_id(req: &Request, host: &RuntimeHost) -> Result<String, Response> {
    let ptys = host.pty_handle.list().await.map_err(|err| Response::err(err.to_string()))?;

    if let Some(pane_id) = req.pane_id.as_deref().filter(|pane_id| !pane_id.trim().is_empty()) {
        return ptys
            .iter()
            .find(|pty| {
                pty_matches_pane(pty, pane_id)
                    && req
                        .session_id
                        .as_deref()
                        .is_none_or(|session_id| pty.info.session_id.as_deref() == Some(session_id))
            })
            .map(|pty| pty.id.clone())
            .ok_or_else(|| Response::err(format!("daemon PTY not found for pane {pane_id}")));
    }

    if let Some(pane_type) = req
        .args
        .get("pane_type")
        .and_then(|pane_type| pane_type.as_str())
        .filter(|pane_type| !pane_type.trim().is_empty())
    {
        let Some(session_id) = req.session_id.as_deref() else {
            return Err(Response::err("session_id required when using pane_type"));
        };
        return ptys
            .iter()
            .find(|pty| {
                pty.info.session_id.as_deref() == Some(session_id)
                    && pty.info.profile.as_deref() == Some(pane_type)
            })
            .map(|pty| pty.id.clone())
            .ok_or_else(|| {
                Response::err(format!(
                    "daemon PTY with profile {pane_type} not found for session {session_id}"
                ))
            });
    }

    let Some(session_id) = req.session_id.as_deref() else {
        return Err(Response::err("session_id or pane_id required"));
    };

    if let Some(primary_pty_id) = host
        .session_handle
        .get(session_id)
        .await
        .map_err(|err| Response::err(err.to_string()))?
        .and_then(|session| session.primary_pty_id)
    {
        return Ok(primary_pty_id);
    }

    ptys.iter()
        .find(|pty| {
            pty.info.session_id.as_deref() == Some(session_id)
                && matches!(pty.info.role, PtyRole::SessionPrimary)
        })
        .map(|pty| pty.id.clone())
        .ok_or_else(|| {
            Response::err(format!("primary daemon PTY not found for session {session_id}"))
        })
}

fn pty_matches_pane(pty: &roux_runtime::pty_service::PtyRecord, pane_id: &str) -> bool {
    pty.id == pane_id
        || pty.info.id == pane_id
        || matches!(&pty.info.status, PtyStatus::RunningAttached { pane_id: attached } if attached == pane_id)
}

fn daemon_record_pane_id(pty: &roux_runtime::pty_service::PtyRecord) -> Option<String> {
    match &pty.info.status {
        PtyStatus::RunningAttached { pane_id } => Some(pane_id.clone()),
        _ => None,
    }
}

async fn handle_session_panes_list(req: Request, host: &RuntimeHost) -> Response {
    let Some(session_id) = req.session_id.as_deref().filter(|session_id| !session_id.is_empty())
    else {
        return Response::err("session_id required");
    };
    match host.session_handle.get(session_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    }

    let mut ptys: Vec<_> = match host.pty_handle.list().await {
        Ok(ptys) => ptys
            .into_iter()
            .filter(|pty| pty.info.session_id.as_deref() == Some(session_id))
            .collect(),
        Err(err) => return Response::err(err.to_string()),
    };
    ptys.sort_by(|a, b| {
        let a_primary = matches!(a.info.role, PtyRole::SessionPrimary);
        let b_primary = matches!(b.info.role, PtyRole::SessionPrimary);
        b_primary.cmp(&a_primary).then_with(|| daemon_pane_id(a).cmp(&daemon_pane_id(b)))
    });

    let descriptors: Vec<Value> = ptys.iter().map(daemon_pane_descriptor).collect();
    Response::success(serde_json::json!({
        "sessionId": session_id,
        "layout": Value::Null,
        "descriptors": descriptors,
    }))
}

async fn handle_session_panes_create(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(session_id) = req.session_id.as_deref().filter(|session_id| !session_id.is_empty())
    else {
        return Response::err("session_id required");
    };
    let session = match host.session_handle.get(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Response::err("session not found"),
        Err(err) => return Response::err(err.to_string()),
    };

    let direction =
        req.args.get("direction").and_then(|direction| direction.as_str()).unwrap_or("horizontal");
    if direction != "horizontal" && direction != "vertical" {
        return Response::err("direction must be horizontal or vertical");
    }

    let profile =
        req.args.get("profile").and_then(|profile| profile.as_str()).unwrap_or("plain-shell");
    let working_dir = req
        .args
        .get("workingDir")
        .or_else(|| req.args.get("working_dir"))
        .and_then(|working_dir| working_dir.as_str())
        .unwrap_or(&session.worktree_path)
        .to_string();
    let pty_id = req
        .args
        .get("id")
        .and_then(|id| id.as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let pane_id = req
        .args
        .get("paneId")
        .or_else(|| req.args.get("pane_id"))
        .and_then(|pane_id| pane_id.as_str())
        .filter(|pane_id| !pane_id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    match host
        .pty_handle
        .spawn_shell(PtySpawnRequest {
            id: Some(pty_id.clone()),
            working_dir: Some(PathBuf::from(&working_dir)),
            session_id: Some(session_id.to_string()),
            pane_id: Some(pane_id.clone()),
            project_id: session.project_id.clone(),
            worktree_path: session.is_worktree.then(|| session.worktree_path.clone()),
            notes: parse_notes_env(&req.args),
            nono: parse_nono_config(&req.args),
            smolvm: match build_daemon_smolvm_exec_for_session(
                &load_daemon_settings(),
                session.smol_machine_name.as_deref(),
            ) {
                Ok(smolvm) => smolvm,
                Err(err) => return Response::err(err),
            },
            env: parse_pty_env_request(&req.args, identity),
            profile: Some(profile.to_string()),
            initial_size: parse_initial_size(&req.args),
            role: PtyRole::Secondary,
        })
        .await
    {
        Ok(_) => Response::success(serde_json::json!({
            "pane_id": pane_id,
            "pty_id": pty_id,
        })),
        Err(err) => Response::err(err.to_string()),
    }
}

fn daemon_pane_id(pty: &roux_runtime::pty_service::PtyRecord) -> String {
    match &pty.info.status {
        PtyStatus::RunningAttached { pane_id } => pane_id.clone(),
        _ => pty.info.id.clone(),
    }
}

fn daemon_pane_descriptor(pty: &roux_runtime::pty_service::PtyRecord) -> Value {
    let mut descriptor = serde_json::Map::new();
    descriptor.insert("id".to_string(), Value::String(daemon_pane_id(pty)));
    descriptor.insert(
        "type".to_string(),
        Value::String(if pty.command.is_some() { "command" } else { "shell" }.to_string()),
    );
    descriptor.insert("ptyId".to_string(), Value::String(pty.id.clone()));
    descriptor.insert("workingDir".to_string(), Value::String(pty.working_dir.clone()));
    if let Some(name) = &pty.info.name {
        descriptor.insert("name".to_string(), Value::String(name.clone()));
    }
    if let Some(command) = &pty.command {
        descriptor.insert("command".to_string(), Value::String(command.clone()));
    }
    if let Some(profile) = &pty.info.profile {
        descriptor.insert("profileId".to_string(), Value::String(profile.clone()));
    }
    Value::Object(descriptor)
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

async fn handle_daemon_pty_spawn_shell(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let request = match parse_pty_spawn_request(&req, host, identity).await {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    match host.pty_handle.spawn_shell(request).await {
        Ok(record) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_spawn_task(
    req: Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Response {
    let Some(command) = req.args.get("command").and_then(|command| command.as_str()) else {
        return Response::err("command required");
    };
    let request = match parse_pty_spawn_request(&req, host, identity).await {
        Ok(request) => request,
        Err(err) => return Response::err(err),
    };
    match host.pty_handle.spawn_task(command.to_string(), request).await {
        Ok(record) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_output(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let max_bytes = req
        .args
        .get("maxBytes")
        .or_else(|| req.args.get("max_bytes"))
        .and_then(|max_bytes| max_bytes.as_u64())
        .map(|max_bytes| max_bytes as usize)
        .unwrap_or(PTY_OUTPUT_DEFAULT_POLL_BYTES);

    match host.pty_handle.snapshot(id, max_bytes).await {
        Ok(Some(snapshot)) => match serde_json::to_value(snapshot) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty output: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_list(host: &RuntimeHost) -> Response {
    match host.pty_handle.list().await {
        Ok(ptys) => match serde_json::to_value(ptys) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon ptys: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_write(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let Some(data) = req.args.get("data").and_then(|data| data.as_str()) else {
        return Response::err("data required");
    };
    match host.pty_handle.write(id, data.as_bytes().to_vec()).await {
        Ok(()) => Response::success(serde_json::json!({ "id": id, "bytes": data.len() })),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_resize(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let cols = req
        .args
        .get("cols")
        .and_then(|cols| cols.as_u64())
        .and_then(|cols| u16::try_from(cols).ok())
        .unwrap_or(80);
    let rows = req
        .args
        .get("rows")
        .and_then(|rows| rows.as_u64())
        .and_then(|rows| u16::try_from(rows).ok())
        .unwrap_or(24);
    match host.pty_handle.resize(id, cols, rows).await {
        Ok(Some(record)) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_kill(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    match host.pty_handle.kill(id).await {
        Ok(Some(record)) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn handle_daemon_pty_detach(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    serialize_daemon_pty_metadata_result(host.pty_handle.detach(id).await)
}

async fn handle_daemon_pty_attach_pane(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let Some(pane_id) = req
        .args
        .get("paneId")
        .or_else(|| req.args.get("pane_id"))
        .and_then(|pane_id| pane_id.as_str())
    else {
        return Response::err("paneId required");
    };
    serialize_daemon_pty_metadata_result(
        host.pty_handle.attach_to_pane(id, pane_id.to_string()).await,
    )
}

async fn handle_daemon_pty_mark_read(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    serialize_daemon_pty_metadata_result(host.pty_handle.mark_read(id).await)
}

async fn handle_daemon_pty_set_name(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        return Response::err("id required");
    };
    let name = req.args.get("name").and_then(|name| {
        if name.is_null() {
            Some(None)
        } else {
            name.as_str().map(|name| Some(name.to_string()))
        }
    });
    let Some(name) = name else {
        return Response::err("name required");
    };
    serialize_daemon_pty_metadata_result(host.pty_handle.set_name(id, name).await)
}

fn serialize_daemon_pty_metadata_result(
    result: Result<
        Option<roux_runtime::pty_service::PtyRecord>,
        roux_runtime::pty_service::PtyServiceError,
    >,
) -> Response {
    match result {
        Ok(Some(record)) => match serde_json::to_value(record) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize daemon pty: {err}")),
        },
        Ok(None) => Response::err("daemon pty not found"),
        Err(err) => Response::err(err.to_string()),
    }
}

async fn parse_pty_spawn_request(
    req: &Request,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> Result<PtySpawnRequest, String> {
    let working_dir = req
        .args
        .get("workingDir")
        .or_else(|| req.args.get("working_dir"))
        .and_then(|working_dir| working_dir.as_str())
        .map(PathBuf::from);
    let session_id = req
        .args
        .get("sessionId")
        .or_else(|| req.args.get("session_id"))
        .and_then(|session_id| session_id.as_str())
        .map(str::to_string)
        .or_else(|| req.session_id.clone());
    let pane_id = req
        .args
        .get("paneId")
        .or_else(|| req.args.get("pane_id"))
        .and_then(|pane_id| pane_id.as_str())
        .map(str::to_string)
        .or_else(|| req.pane_id.clone());
    let role = match req.args.get("role").and_then(|role| role.as_str()) {
        Some("sessionPrimary") | Some("session_primary") => roux_core::PtyRole::SessionPrimary,
        _ => roux_core::PtyRole::Secondary,
    };

    let mut request = PtySpawnRequest {
        id: req.args.get("id").and_then(|id| id.as_str()).map(str::to_string),
        working_dir,
        session_id,
        pane_id,
        project_id: req
            .args
            .get("projectId")
            .or_else(|| req.args.get("project_id"))
            .and_then(|project_id| project_id.as_str())
            .map(str::to_string),
        worktree_path: req
            .args
            .get("worktreePath")
            .or_else(|| req.args.get("worktree_path"))
            .and_then(|worktree_path| worktree_path.as_str())
            .map(str::to_string),
        notes: parse_notes_env(&req.args),
        nono: parse_nono_config(&req.args),
        smolvm: None,
        env: parse_pty_env_request(&req.args, identity),
        profile: req.args.get("profile").and_then(|profile| profile.as_str()).map(str::to_string),
        initial_size: parse_initial_size(&req.args),
        role,
    };
    apply_session_spawn_bindings(&mut request, host).await?;
    Ok(request)
}

async fn apply_session_spawn_bindings(
    request: &mut PtySpawnRequest,
    host: &RuntimeHost,
) -> Result<(), String> {
    let Some(session_id) = request.session_id.as_deref() else {
        return Ok(());
    };
    let Some(session) = host.session_handle.get(session_id).await.map_err(|err| err.to_string())?
    else {
        return Ok(());
    };
    if request.project_id.is_none() {
        request.project_id = session.project_id.clone();
    }
    if request.worktree_path.is_none() && session.is_worktree {
        request.worktree_path = Some(session.worktree_path.clone());
    }
    request.smolvm = build_daemon_smolvm_exec_for_session(
        &load_daemon_settings(),
        session.smol_machine_name.as_deref(),
    )?;
    Ok(())
}

fn parse_nono_config(args: &Value) -> Option<NonoConfig> {
    let profile = optional_string_arg(args, &["nonoProfile", "nono_profile"])?;
    let allow_dirs = args
        .get("nonoAllowDirs")
        .or_else(|| args.get("nono_allow_dirs"))
        .and_then(|dirs| dirs.as_array())
        .map(|dirs| {
            dirs.iter().filter_map(|dir| dir.as_str()).map(str::to_string).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(NonoConfig { profile, allow_dirs })
}

fn build_daemon_smolvm_exec_for_session(
    settings: &roux_core::RouxSettings,
    smol_machine_name: Option<&str>,
) -> Result<Option<SmolvmExec>, String> {
    let Some(name) = smol_machine_name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    let override_path =
        settings.smolvm_binary_path.as_deref().map(str::trim).filter(|p| !p.is_empty());
    let detection = roux_core::smolvm::detect(override_path);
    let Some(binary_path) = detection.binary_path else {
        return Err(format!(
            "session is bound to smol machine '{name}', but smolvm is not installed; unbind or install smolvm to continue"
        ));
    };
    Ok(Some(SmolvmExec {
        binary: PathBuf::from(binary_path),
        machine_name: name.to_string(),
        guest_shell: "/bin/sh".to_string(),
    }))
}

fn parse_pty_env_request(args: &Value, identity: &DaemonIdentity) -> PtyEnvRequest {
    let current_exe = std::env::current_exe().ok();
    let cli_path = args
        .get("cliPath")
        .or_else(|| args.get("cli_path"))
        .and_then(|cli_path| cli_path.as_str())
        .map(str::to_string)
        .or_else(|| current_exe.as_ref().map(|path| path.to_string_lossy().into_owned()));
    let cli_bin_dir = args
        .get("cliBinDir")
        .or_else(|| args.get("cli_bin_dir"))
        .and_then(|cli_bin_dir| cli_bin_dir.as_str())
        .map(str::to_string)
        .or_else(|| {
            current_exe
                .as_ref()
                .and_then(|path| path.parent())
                .map(|path| path.to_string_lossy().into_owned())
        });

    PtyEnvRequest {
        user_path: args
            .get("userPath")
            .or_else(|| args.get("user_path"))
            .and_then(|user_path| user_path.as_str())
            .map(str::to_string)
            .or_else(|| std::env::var("PATH").ok()),
        socket_path: args
            .get("socketPath")
            .or_else(|| args.get("socket_path"))
            .and_then(|socket_path| socket_path.as_str())
            .map(str::to_string)
            .or_else(|| Some(identity.endpoint_display())),
        cli_bin_dir,
        cli_path,
        pane_alias: args
            .get("paneAlias")
            .or_else(|| args.get("pane_alias"))
            .and_then(|pane_alias| pane_alias.as_str())
            .map(str::to_string),
    }
}

fn parse_notes_env(args: &Value) -> Option<NotesEnvInputs> {
    let value = args.get("notesEnv").or_else(|| args.get("notes_env"))?;
    Some(NotesEnvInputs {
        vault_root: value
            .get("vaultRoot")
            .or_else(|| value.get("vault_root"))
            .and_then(|root| root.as_str())?
            .to_string(),
        session_slug: value
            .get("sessionSlug")
            .or_else(|| value.get("session_slug"))
            .and_then(|slug| slug.as_str())?
            .to_string(),
        repo_slug: value
            .get("repoSlug")
            .or_else(|| value.get("repo_slug"))
            .and_then(|slug| slug.as_str())?
            .to_string(),
        project_slug: value
            .get("projectSlug")
            .or_else(|| value.get("project_slug"))
            .and_then(|slug| slug.as_str())
            .map(str::to_string),
        context_paths: value
            .get("contextPaths")
            .or_else(|| value.get("context_paths"))
            .and_then(|paths| paths.as_array())
            .map(|paths| {
                paths.iter().filter_map(|path| path.as_str().map(str::to_string)).collect()
            })
            .unwrap_or_default(),
        project_prompt: value
            .get("projectPrompt")
            .or_else(|| value.get("project_prompt"))
            .and_then(|prompt| prompt.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn request_session_id(req: &Request) -> Option<&str> {
    req.session_id
        .as_deref()
        .or_else(|| req.args.get("sessionId").or_else(|| req.args.get("session_id"))?.as_str())
}

fn request_repo_path(req: &Request) -> Option<&str> {
    req.args
        .get("repoPath")
        .or_else(|| req.args.get("repo_path"))
        .and_then(|repo_path| repo_path.as_str())
}

fn optional_string_arg(args: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| args.get(*name))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_nullable_string_arg(args: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| args.get(*name)).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_str().map(str::trim).filter(|value| !value.is_empty()).map(str::to_string)
        }
    })
}

fn bool_arg(args: &Value, names: &[&str]) -> Option<bool> {
    names.iter().find_map(|name| args.get(*name)).and_then(|value| value.as_bool())
}

enum DaemonSessionTarget {
    Repo,
    ExistingWorktree { path: String },
    NewWorktree { branch: String, start_point: Option<String>, fetch_first: bool },
}

fn parse_daemon_session_target(args: &Value) -> DaemonSessionTarget {
    if let Some(path) = args
        .get("worktreePath")
        .or_else(|| args.get("worktree_path"))
        .and_then(|path| path.as_str())
        .filter(|path| !path.trim().is_empty())
    {
        return DaemonSessionTarget::ExistingWorktree { path: path.to_string() };
    }
    if let Some(branch) = args
        .get("branch")
        .or_else(|| args.get("worktreeBranch"))
        .or_else(|| args.get("worktree_branch"))
        .and_then(|branch| branch.as_str())
        .filter(|branch| !branch.trim().is_empty())
    {
        let start_point = args
            .get("base")
            .or_else(|| args.get("startPoint"))
            .or_else(|| args.get("start_point"))
            .and_then(|base| base.as_str())
            .filter(|base| !base.trim().is_empty())
            .map(str::to_string);
        let fetch_first = args
            .get("fetchFirst")
            .or_else(|| args.get("fetch_first"))
            .and_then(|fetch| fetch.as_bool())
            .unwrap_or(false);
        return DaemonSessionTarget::NewWorktree {
            branch: branch.to_string(),
            start_point,
            fetch_first,
        };
    }
    DaemonSessionTarget::Repo
}

async fn resolve_daemon_session_target(
    repo_path: &str,
    target: DaemonSessionTarget,
    settings: &roux_core::RouxSettings,
    hooks: AutomationHookManager,
) -> Result<(String, String, bool), String> {
    match target {
        DaemonSessionTarget::Repo => {
            let branch = get_current_branch(repo_path).unwrap_or_else(|| "main".to_string());
            Ok((repo_path.to_string(), branch, false))
        }
        DaemonSessionTarget::ExistingWorktree { path } => {
            let branch = get_current_branch(&path).unwrap_or_else(|| "main".to_string());
            Ok((path, branch, false))
        }
        DaemonSessionTarget::NewWorktree { branch, start_point, fetch_first } => {
            let wt = resolve_wt_binary(settings);
            let provider = settings.worktree_provider;
            let wt_available = wt.is_some();
            let pre_context = HookContext {
                repo_path: Some(repo_path.to_string()),
                branch: Some(branch.clone()),
                cwd: Some(repo_path.to_string()),
                ..HookContext::new(HookEvent::PreWorktreeCreate)
                    .with_provider(provider, wt_available)
            };
            hooks
                .run_blocking(HookEvent::PreWorktreeCreate, pre_context)
                .await
                .map_err(|err| err.to_string())?;
            if fetch_first {
                roux_core::fetch_origin(repo_path).map_err(|err| err.to_string())?;
            }
            let worktree_path = roux_core::create_worktree_with_provider(
                repo_path,
                &branch,
                settings.worktree_base_path.as_deref(),
                start_point.as_deref(),
                provider,
                wt.as_ref(),
            )
            .map_err(|err| err.to_string())?;
            let context = build_daemon_post_worktree_create_context(
                provider,
                wt_available,
                repo_path,
                &branch,
                &worktree_path,
            );
            hooks.spawn_background(HookEvent::PostWorktreeCreate, context);
            Ok((worktree_path, branch, true))
        }
    }
}

fn load_daemon_settings() -> roux_core::RouxSettings {
    let path = platform::settings_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<roux_core::RouxSettings>(&content).unwrap_or_default().normalized()
    } else {
        roux_core::RouxSettings::default()
    }
}

fn resolve_wt_binary(settings: &roux_core::RouxSettings) -> Option<roux_worktrunk::WtBinary> {
    let override_path =
        settings.worktrunk_binary_path.as_deref().map(str::trim).filter(|p| !p.is_empty());
    match override_path {
        Some(path) => roux_worktrunk::detect_wt(Some(path)),
        None => roux_worktrunk::detect_wt(None),
    }
}

fn get_current_branch(repo_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn list_branches(repo_path: &str) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Failed to list branches: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_string)
        .collect())
}

fn git_init(path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .map_err(|err| format!("Failed to run git init: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn is_git_repo(path: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn parse_initial_size(args: &Value) -> Option<(u16, u16)> {
    let value = args.get("initialSize").or_else(|| args.get("initial_size"))?;
    let array = value.as_array()?;
    let cols = array.first()?.as_u64().and_then(|cols| u16::try_from(cols).ok())?;
    let rows = array.get(1)?.as_u64().and_then(|rows| u16::try_from(rows).ok())?;
    Some((cols, rows))
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn daemon_auth_token(endpoint: &platform::SocketEndpoint) -> Result<Option<String>, String> {
    match endpoint {
        platform::SocketEndpoint::Unix(_) => Ok(None),
        platform::SocketEndpoint::Tcp(addr) => {
            if let Some(token) = daemon_env_auth_token() {
                return Ok(Some(token));
            }

            #[cfg(windows)]
            {
                Ok(Some(format!("{}-{}", std::process::id(), unix_now_ms())))
            }

            #[cfg(not(windows))]
            {
                Err(format!("TCP daemon bind tcp://{addr} requires ROUX_DAEMON_TOKEN"))
            }
        }
    }
}

fn daemon_env_auth_token() -> Option<String> {
    for key in ["ROUX_DAEMON_TOKEN", "ROUX_AUTH_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

async fn wait_for_shutdown_signal(mut daemon_stop_rx: watch::Receiver<bool>) -> Result<(), String> {
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
            _ = wait_for_daemon_stop(&mut daemon_stop_rx) => {}
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|err| format!("failed to wait for shutdown signal: {err}"))
            }
            _ = wait_for_daemon_stop(&mut daemon_stop_rx) => Ok(()),
        }
    }
}

async fn wait_for_daemon_stop(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow_and_update() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
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

    fn make_watch_config() -> roux_core::CreateWatchConfig {
        roux_core::CreateWatchConfig {
            name: "HTTP".to_string(),
            kind: roux_core::WatchKind::HttpHealth {
                url: "http://localhost".to_string(),
                expected_status: 200,
            },
            mode: roux_core::WatchMode::Recurring { interval_secs: 30 },
            scope: roux_core::WatchScope::Global,
            notify: None,
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
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
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
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("daemon-stop")));
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("daemon-pty-attach")));
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("worktree-list")));
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("watch-list")));
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("watch-events")));
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("mailbox-events")));
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("alias-events")));
        assert!(data["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("subscription-events")));

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_watch_commands_mutate_runtime_state() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let watch_runner = WatchRunner::new(
            host.watch_handle.clone(),
            AutomationHookManager::from_config_root(dir.path()),
        );
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create = handle_request_with_watch_runner(
            Request {
                command: "watch-create".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "config": make_watch_config() }),
            },
            &host,
            Some(&watch_runner),
            &identity,
        )
        .await;
        assert!(create.ok, "create failed: {:?}", create.error);
        let created: roux_core::Watch =
            serde_json::from_value(create.data.clone().expect("created watch")).unwrap();
        assert!(matches!(created.runtime_state, roux_core::RuntimeState::Active));

        let list = handle_request(
            Request {
                command: "watch-list".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::Value::Null,
            },
            &host,
            &identity,
        )
        .await;
        assert!(list.ok, "list failed: {:?}", list.error);
        assert_eq!(list.data.as_ref().unwrap().as_array().unwrap().len(), 1);

        let pause = handle_request_with_watch_runner(
            Request {
                command: "watch-pause".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": created.id }),
            },
            &host,
            Some(&watch_runner),
            &identity,
        )
        .await;
        assert!(pause.ok, "pause failed: {:?}", pause.error);
        assert_eq!(pause.data.as_ref().unwrap()["runtimeState"]["type"], "paused");

        let mut replacement: roux_core::Watch =
            serde_json::from_value(pause.data.clone().expect("paused watch")).unwrap();
        replacement.name = "Updated by client".to_string();
        let replace = handle_request_with_watch_runner(
            Request {
                command: "watch-replace".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "watch": replacement }),
            },
            &host,
            Some(&watch_runner),
            &identity,
        )
        .await;
        assert!(replace.ok, "replace failed: {:?}", replace.error);
        assert_eq!(replace.data.as_ref().unwrap()["name"], "Updated by client");

        let resume = handle_request_with_watch_runner(
            Request {
                command: "watch-resume".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": replace.data.as_ref().unwrap()["id"] }),
            },
            &host,
            Some(&watch_runner),
            &identity,
        )
        .await;
        assert!(resume.ok, "resume failed: {:?}", resume.error);
        assert_eq!(resume.data.as_ref().unwrap()["runtimeState"]["type"], "active");

        let remove = handle_request_with_watch_runner(
            Request {
                command: "watch-remove".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": resume.data.as_ref().unwrap()["id"] }),
            },
            &host,
            Some(&watch_runner),
            &identity,
        )
        .await;
        assert!(remove.ok, "remove failed: {:?}", remove.error);
        assert!(host.watch_handle.list().await.unwrap().is_empty());

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_mailbox_events_streams_live_mailbox_events() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
        let (mut server, client) = tokio::io::duplex(4096);
        let identity_for_stream = identity.clone();
        let stream_task = tokio::spawn(async move {
            handle_mailbox_events_stream(
                Request {
                    command: "mailbox-events".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::Value::Null,
                },
                &mut server,
                &identity_for_stream,
            )
            .await
        });

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(ready["type"], "ready");

        identity.mailbox_manager.post(EventBuilder::new("hello").to("auditor")).unwrap();

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let frame: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(frame["type"], "event");
        assert_eq!(frame["event"]["kind"], "posted");
        assert_eq!(frame["event"]["event"]["body"], "hello");
        assert_eq!(frame["event"]["event"]["to"], "auditor");

        stream_task.abort();
    }

    #[tokio::test]
    async fn daemon_subscription_events_streams_live_subscription_events() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
        let (mut server, client) = tokio::io::duplex(4096);
        let identity_for_stream = identity.clone();
        let stream_task = tokio::spawn(async move {
            handle_subscription_events_stream(
                Request {
                    command: "subscription-events".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::Value::Null,
                },
                &mut server,
                &identity_for_stream,
            )
            .await
        });

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(ready["type"], "ready");

        identity.subscription_manager.subscribe("auditor", "build.**", None).unwrap();

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let frame: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(frame["type"], "event");
        assert_eq!(frame["event"]["kind"], "created");
        assert_eq!(frame["event"]["subscription"]["alias"], "auditor");
        assert_eq!(frame["event"]["subscription"]["pattern"], "build.**");

        stream_task.abort();
    }

    #[tokio::test]
    async fn daemon_alias_events_streams_live_alias_events() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
        let (mut server, client) = tokio::io::duplex(4096);
        let identity_for_stream = identity.clone();
        let stream_task = tokio::spawn(async move {
            handle_alias_events_stream(
                Request {
                    command: "alias-events".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::Value::Null,
                },
                &mut server,
                &identity_for_stream,
            )
            .await
        });

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(ready["type"], "ready");

        identity
            .alias_manager
            .bind(
                "reviewer",
                BindRequest { session_id: Some("session-1".into()), ..Default::default() },
            )
            .unwrap();

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let frame: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(frame["type"], "event");
        assert_eq!(frame["event"]["kind"], "set");
        assert_eq!(frame["event"]["alias"]["alias"], "reviewer");
        assert_eq!(frame["event"]["alias"]["sessionId"], "session-1");

        stream_task.abort();
    }

    #[tokio::test]
    async fn daemon_watch_events_stream_sends_ready_and_backlog() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let dir = tempfile::tempdir().unwrap();
        let mut watch = roux_core::Watch {
            id: "watch-a".to_string(),
            name: "HTTP".to_string(),
            kind: roux_core::WatchKind::HttpHealth {
                url: "http://localhost".to_string(),
                expected_status: 200,
            },
            mode: roux_core::WatchMode::Recurring { interval_secs: 30 },
            scope: roux_core::WatchScope::Global,
            runtime_state: roux_core::RuntimeState::Paused,
            last_result: None,
            last_checked: None,
            notify: roux_core::NotifyConfig::default(),
            created_at: 0,
        };
        watch.last_checked = Some(1);
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: vec![watch],
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let watch_runner = WatchRunner::new(
            host.watch_handle.clone(),
            AutomationHookManager::from_config_root(dir.path()),
        );
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
        let (mut server, client) = tokio::io::duplex(4096);
        let host_for_stream = host.clone();
        let runner_for_stream = watch_runner.clone();
        let identity_for_stream = identity.clone();
        let stream_task = tokio::spawn(async move {
            handle_watch_events_stream(
                Request {
                    command: "watch-events".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::json!({ "backlog": true }),
                },
                &mut server,
                &host_for_stream,
                &runner_for_stream,
                &identity_for_stream,
            )
            .await
        });

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(ready["type"], "ready");

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let update: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(update["type"], "update");
        assert_eq!(update["event"]["watch"]["id"], "watch-a");
        assert_eq!(update["event"]["changed"], false);

        stream_task.abort();
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
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
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
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
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    fn git(repo: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("failed to invoke git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(repo: &std::path::Path) {
        std::fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "-q", "-b", "main"]);
        git(repo, &["config", "user.email", "t@t.test"]);
        git(repo, &["config", "user.name", "Test"]);
        git(repo, &["commit", "--allow-empty", "-m", "init"]);
    }

    fn shell_quote(path: &std::path::Path) -> String {
        format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
    }

    fn toml_escape(value: &str) -> String {
        value.replace('\\', "\\\\").replace('"', "\\\"")
    }

    async fn wait_for_marker(path: &std::path::Path, expected: &str) {
        for _ in 0..200 {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            if content.contains(expected) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("marker {} did not contain {expected:?}", path.display());
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_hook_commands_use_daemon_hook_manager() {
        let dir = tempfile::tempdir().unwrap();
        let hook_root = dir.path().join("hook-root");
        std::fs::create_dir_all(&hook_root).unwrap();
        std::fs::write(hook_root.join("hooks.toml"), r#"post-task-run = "true""#).unwrap();

        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".config").join("roux")).unwrap();
        std::fs::write(
            repo.join(".config").join("roux").join("hooks.toml"),
            r#"pre-watch-run = "cat >/dev/null""#,
        )
        .unwrap();

        let hooks = AutomationHookManager::from_config_root(&hook_root);
        let show = handle_hook_show_with_hooks(
            Request {
                command: "hook-show".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "repoPath": repo }),
            },
            hooks.clone(),
        )
        .await;
        assert!(show.ok, "hook-show failed: {:?}", show.error);
        let shown = show.data.as_ref().unwrap().as_array().unwrap();
        assert!(shown.iter().any(|item| item["source"] == "user"));
        assert!(shown.iter().any(|item| item["source"] == "project"));

        let preview_args = serde_json::json!({
            "event": "pre-watch-run",
            "repoPath": repo,
        });
        let preview = handle_hook_preview_with_hooks(
            Request {
                command: "hook-preview".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: preview_args.clone(),
            },
            hooks.clone(),
        )
        .await;
        assert!(preview.ok, "hook-preview failed: {:?}", preview.error);
        let preview_items = preview.data.as_ref().unwrap().as_array().unwrap();
        let approval_id =
            preview_items[0]["approvalId"].as_str().expect("project hook approval id").to_string();
        assert_eq!(preview_items[0]["approved"], false);

        let rejected = handle_hook_run_with_hooks(
            Request {
                command: "hook-run".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: preview_args.clone(),
            },
            hooks.clone(),
        )
        .await;
        assert!(!rejected.ok);
        assert!(rejected.error.as_deref().unwrap_or_default().contains("approval"));

        let approved = handle_hook_approve_with_hooks(
            Request {
                command: "hook-approve".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "approvalId": approval_id }),
            },
            hooks.clone(),
        )
        .await;
        assert!(approved.ok, "hook-approve failed: {:?}", approved.error);

        let run = handle_hook_run_with_hooks(
            Request {
                command: "hook-run".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: preview_args,
            },
            hooks.clone(),
        )
        .await;
        assert!(run.ok, "hook-run failed: {:?}", run.error);
        assert_eq!(run.data.as_ref().unwrap()["event"], "pre-watch-run");
        assert_eq!(run.data.as_ref().unwrap()["ran"], 1);

        let logs = handle_hook_log_list_with_hooks(hooks.clone()).await;
        assert!(logs.ok, "hook-log-list failed: {:?}", logs.error);
        let log_items = logs.data.as_ref().unwrap().as_array().unwrap();
        assert_eq!(log_items.len(), 1);
        let log_path = log_items[0]["path"].as_str().expect("hook log path").to_string();
        let log = handle_hook_log_read_with_hooks(
            Request {
                command: "hook-log-read".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "path": log_path }),
            },
            hooks.clone(),
        )
        .await;
        assert!(log.ok, "hook-log-read failed: {:?}", log.error);
        assert!(log.data.as_ref().unwrap().as_str().unwrap().contains("pre-watch-run"));

        let cleared = handle_hook_clear_approvals_with_hooks(hooks).await;
        assert!(cleared.ok, "hook-clear-approvals failed: {:?}", cleared.error);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_worktree_commands_mutate_git_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let worktree_base = dir.path().join("worktrees");
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let before = handle_request(
            Request {
                command: "worktree-list".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "repoPath": repo }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(before.ok, "list failed: {:?}", before.error);
        assert_eq!(before.data.as_ref().unwrap().as_array().unwrap().len(), 1);

        let create = handle_request(
            Request {
                command: "worktree-create".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "repoPath": repo,
                    "branch": "feature/daemon-worktree",
                    "startPoint": "main",
                    "basePath": worktree_base,
                    "fetchFirst": false,
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create.ok, "create failed: {:?}", create.error);
        let worktree_path = create.data.as_ref().unwrap()["path"].as_str().unwrap().to_string();
        assert!(std::path::Path::new(&worktree_path).exists());

        let branches = handle_request(
            Request {
                command: "worktree-list-branches".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "repoPath": repo }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(branches.ok, "branches failed: {:?}", branches.error);
        assert!(branches
            .data
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("feature/daemon-worktree")));

        let after_create = handle_request(
            Request {
                command: "worktree-list".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "repoPath": repo }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(after_create.ok, "list after create failed: {:?}", after_create.error);
        assert!(after_create.data.as_ref().unwrap().as_array().unwrap().iter().any(|entry| {
            entry["branch"] == "feature/daemon-worktree" && entry["path"] == worktree_path
        }));

        let remove = handle_request(
            Request {
                command: "worktree-remove".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "repoPath": repo,
                    "worktreePath": worktree_path,
                    "alsoBranch": true,
                    "force": true,
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(remove.ok, "remove failed: {:?}", remove.error);
        assert!(!std::path::Path::new(&worktree_path).exists());

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_worktree_commands_run_hooks_server_side() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let worktree_base = dir.path().join("worktrees");
        let hook_root = dir.path().join("hook-root");
        std::fs::create_dir_all(&hook_root).unwrap();
        let marker = dir.path().join("hook-events.txt");
        let marker_arg = shell_quote(&marker);
        let hooks_toml = format!(
            r#"
pre-worktree-create = "{pre_create}"
post-worktree-create = "{post_create}"
pre-worktree-remove = "{pre_remove}"
post-worktree-remove = "{post_remove}"
"#,
            pre_create = toml_escape(&format!("printf pre-create >> {marker_arg}")),
            post_create = toml_escape(&format!("printf post-create >> {marker_arg}")),
            pre_remove = toml_escape(&format!("printf pre-remove >> {marker_arg}")),
            post_remove = toml_escape(&format!("printf post-remove >> {marker_arg}")),
        );
        std::fs::write(hook_root.join("hooks.toml"), hooks_toml).unwrap();
        let hooks = AutomationHookManager::from_config_root(&hook_root);
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);

        let create = handle_worktree_create_with_hooks(
            Request {
                command: "worktree-create".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "repoPath": repo,
                    "branch": "feature/hooked-worktree",
                    "startPoint": "main",
                    "basePath": worktree_base,
                }),
            },
            hooks.clone(),
        )
        .await;
        assert!(create.ok, "create failed: {:?}", create.error);
        let worktree_path = create.data.as_ref().unwrap()["path"].as_str().unwrap().to_string();
        wait_for_marker(&marker, "pre-create").await;
        wait_for_marker(&marker, "post-create").await;

        let remove = handle_worktree_remove_with_hooks(
            Request {
                command: "worktree-remove".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "repoPath": repo,
                    "worktreePath": worktree_path,
                    "alsoBranch": true,
                    "force": true,
                }),
            },
            hooks,
        )
        .await;
        assert!(remove.ok, "remove failed: {:?}", remove.error);
        wait_for_marker(&marker, "pre-remove").await;
        wait_for_marker(&marker, "post-remove").await;

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_session_worktree_target_runs_hooks_server_side() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_repo(&repo);
        let worktree_base = dir.path().join("worktrees");
        let hook_root = dir.path().join("hook-root");
        std::fs::create_dir_all(&hook_root).unwrap();
        let marker = dir.path().join("session-worktree-hook-events.txt");
        let marker_arg = shell_quote(&marker);
        let hooks_toml = format!(
            r#"
pre-worktree-create = "{pre_create}"
post-worktree-create = "{post_create}"
"#,
            pre_create = toml_escape(&format!("printf pre-create >> {marker_arg}")),
            post_create = toml_escape(&format!("printf post-create >> {marker_arg}")),
        );
        std::fs::write(hook_root.join("hooks.toml"), hooks_toml).unwrap();
        let hooks = AutomationHookManager::from_config_root(&hook_root);
        let settings = roux_core::RouxSettings {
            worktree_base_path: Some(worktree_base.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let (worktree_path, branch, owns_worktree) = resolve_daemon_session_target(
            &repo.to_string_lossy(),
            DaemonSessionTarget::NewWorktree {
                branch: "feature/session-hooked-worktree".to_string(),
                start_point: Some("main".to_string()),
                fetch_first: false,
            },
            &settings,
            hooks,
        )
        .await
        .expect("session target should create worktree");

        assert_eq!(branch, "feature/session-hooked-worktree");
        assert!(owns_worktree);
        assert!(std::path::Path::new(&worktree_path).exists());
        wait_for_marker(&marker, "pre-create").await;
        wait_for_marker(&marker, "post-create").await;
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_session_create_shell_owns_session_and_primary_pty() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let response = handle_request(
            Request {
                command: "session-create-shell".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "session-a",
                    "repoPath": dir.path(),
                    "name": "Daemon Session",
                    "profile": "plain-shell",
                    "initialSize": [100, 30]
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(response.ok, "create failed: {:?}", response.error);
        let data = response.data.expect("session payload");
        assert_eq!(data["id"], "session-a");
        assert_eq!(data["name"], "Daemon Session");
        assert_eq!(data["primaryPtyId"], "session-a");

        let session = host.session_handle.get("session-a").await.unwrap().unwrap();
        assert_eq!(session.name, "Daemon Session");
        assert_eq!(session.primary_pty_id.as_deref(), Some("session-a"));

        let ptys = host.pty_handle.list().await.unwrap();
        let pty = ptys.iter().find(|pty| pty.id == "session-a").expect("primary pty");
        assert_eq!(pty.working_dir, dir.path().to_string_lossy());
        assert_eq!(pty.cols, 100);
        assert_eq!(pty.rows, 30);
        assert!(matches!(pty.info.role, roux_core::PtyRole::SessionPrimary));
        assert_eq!(pty.info.session_id.as_deref(), Some("session-a"));

        let _ = host.pty_handle.kill("session-a").await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_session_create_alias_creates_daemon_session() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create = handle_request(
            Request {
                command: "session-create".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "name": "Created from CLI",
                    "working_dir": dir.path(),
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create.ok, "session-create alias failed: {:?}", create.error);
        let session_id = create.data.as_ref().unwrap()["session_id"].as_str().unwrap();
        let session = host.session_handle.get(session_id).await.unwrap().expect("session");
        assert_eq!(session.name, "Created from CLI");
        assert_eq!(session.primary_pty_id.as_deref(), Some(session_id));

        let _ = host.pty_handle.kill(session_id).await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_session_create_alias_rejects_prompt_until_attach_queue_exists() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create = handle_request(
            Request {
                command: "session-create".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "working_dir": dir.path(),
                    "prompt": "do the thing",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(!create.ok);
        assert!(create.error.as_deref().unwrap_or("").contains("--prompt"));

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_session_panes_create_spawns_secondary_pty_and_list_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create_session = handle_request(
            Request {
                command: "session-create-shell".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "session-panes",
                    "repoPath": dir.path(),
                    "name": "Pane Session",
                    "profile": "plain-shell",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create_session.ok, "session create failed: {:?}", create_session.error);

        let create_pane = handle_request(
            Request {
                command: "session-panes-create".to_string(),
                session_id: Some("session-panes".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "profile": "plain-shell",
                    "direction": "vertical",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create_pane.ok, "pane create failed: {:?}", create_pane.error);
        let pane_id = create_pane.data.as_ref().unwrap()["pane_id"].as_str().unwrap().to_string();
        let pty_id = create_pane.data.as_ref().unwrap()["pty_id"].as_str().unwrap().to_string();

        let ptys = host.pty_handle.list().await.unwrap();
        let pty = ptys.iter().find(|pty| pty.id == pty_id).expect("secondary pty");
        assert_eq!(pty.info.session_id.as_deref(), Some("session-panes"));
        assert!(matches!(pty.info.role, roux_core::PtyRole::Secondary));
        assert_eq!(pty.info.profile.as_deref(), Some("plain-shell"));
        assert!(pty_matches_pane(pty, &pane_id));

        let list = handle_request(
            Request {
                command: "session-panes-list".to_string(),
                session_id: Some("session-panes".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(list.ok, "pane list failed: {:?}", list.error);
        let data = list.data.as_ref().unwrap();
        assert_eq!(data["sessionId"], "session-panes");
        assert!(data["layout"].is_null());
        assert!(data["descriptors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|descriptor| descriptor["id"] == pane_id && descriptor["ptyId"] == pty_id));

        let _ = host.pty_handle.kill("session-panes").await;
        let _ = host.pty_handle.kill(&pty_id).await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_top_level_shell_spawns_secondary_pty() {
        let dir = tempfile::tempdir().unwrap();
        let alternate_dir = dir.path().join("alternate");
        std::fs::create_dir_all(&alternate_dir).unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create_session = handle_request(
            Request {
                command: "session-create-shell".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "session-shell",
                    "repoPath": dir.path(),
                    "name": "Shell Session",
                    "profile": "plain-shell",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create_session.ok, "session create failed: {:?}", create_session.error);

        let shell = handle_request(
            Request {
                command: "shell".to_string(),
                session_id: Some("session-shell".to_string()),
                pane_id: Some("session-shell-main".to_string()),
                auth_token: None,
                args: serde_json::json!({ "working_dir": alternate_dir }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(shell.ok, "shell failed: {:?}", shell.error);
        let pty_id = shell.data.as_ref().unwrap()["pty_id"].as_str().unwrap().to_string();

        let pty = host
            .pty_handle
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|pty| pty.id == pty_id)
            .expect("shell pty");
        assert_eq!(pty.info.session_id.as_deref(), Some("session-shell"));
        assert!(matches!(pty.info.role, roux_core::PtyRole::Secondary));
        assert_eq!(pty.info.profile.as_deref(), Some("plain-shell"));
        assert_eq!(pty.working_dir, alternate_dir.to_string_lossy());

        let _ = host.pty_handle.kill("session-shell").await;
        let _ = host.pty_handle.kill(&pty_id).await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_top_level_split_spawns_secondary_pty() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create_session = handle_request(
            Request {
                command: "session-create-shell".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "session-split",
                    "repoPath": dir.path(),
                    "name": "Split Session",
                    "profile": "plain-shell",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create_session.ok, "session create failed: {:?}", create_session.error);

        let split = handle_request(
            Request {
                command: "split".to_string(),
                session_id: Some("session-split".to_string()),
                pane_id: Some("session-split-main".to_string()),
                auth_token: None,
                args: serde_json::json!({ "direction": "vertical" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(split.ok, "split failed: {:?}", split.error);
        let pty_id = split.data.as_ref().unwrap()["pty_id"].as_str().unwrap().to_string();

        let pty = host
            .pty_handle
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|pty| pty.id == pty_id)
            .expect("split pty");
        assert_eq!(pty.info.session_id.as_deref(), Some("session-split"));
        assert!(matches!(pty.info.role, roux_core::PtyRole::Secondary));
        assert_eq!(pty.info.profile.as_deref(), Some("plain-shell"));
        assert_eq!(pty.working_dir, dir.path().to_string_lossy());

        let _ = host.pty_handle.kill("session-split").await;
        let _ = host.pty_handle.kill(&pty_id).await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_notes_commands_use_shared_vault_service() {
        let dir = tempfile::tempdir().unwrap();
        let vault_root = dir.path().join("vault");
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
        let target = serde_json::json!({
            "scope": "global",
            "sessionId": null,
            "topic": null,
            "overrideSlug": null,
        });

        let append = handle_request(
            Request {
                command: "notes-append".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "vaultRoot": vault_root,
                    "target": target,
                    "content": "daemon note #daemon",
                    "timestamped": false,
                    "tags": ["daemon"],
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(append.ok, "append failed: {:?}", append.error);

        let read = handle_request(
            Request {
                command: "notes-read".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "vaultRoot": vault_root,
                    "scope": "global",
                    "sessionId": null,
                    "topic": null,
                    "overrideSlug": null,
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(read.ok, "read failed: {:?}", read.error);
        assert!(read.data.as_ref().unwrap()["content"].as_str().unwrap().contains("daemon note"));

        let search = handle_request(
            Request {
                command: "notes-search".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "vaultRoot": vault_root,
                    "tags": ["daemon"],
                    "scope": "global",
                    "exact": false,
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(search.ok, "search failed: {:?}", search.error);
        assert_eq!(search.data.as_ref().unwrap().as_array().unwrap().len(), 1);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_latest_output_alias_reads_daemon_pty_replay() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let start = handle_request(
            Request {
                command: "daemon-pty-spawn-task".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "latest-pty",
                    "command": "cat",
                    "workingDir": dir.path(),
                    "sessionId": "session-latest",
                    "paneId": "pane-latest",
                    "profile": "task",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(start.ok, "spawn task failed: {:?}", start.error);
        let pty_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let write = handle_request(
            Request {
                command: "daemon-pty-write".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": pty_id, "data": "daemon-latest\n" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(write.ok, "write failed: {:?}", write.error);

        let mut latest = None;
        for _ in 0..50 {
            let response = handle_request(
                Request {
                    command: "latest-output".to_string(),
                    session_id: None,
                    pane_id: Some("pane-latest".to_string()),
                    auth_token: None,
                    args: serde_json::json!({ "max_bytes": 1024 }),
                },
                &host,
                &identity,
            )
            .await;
            if !response.ok {
                assert!(
                    response.error.as_deref().unwrap_or("").contains("daemon PTY not found"),
                    "latest-output failed: {:?}",
                    response.error
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }
            let data = response.data.unwrap();
            if data["text"].as_str().unwrap_or("").contains("daemon-latest") {
                latest = Some(data);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let latest = latest.expect("latest output should include task output");
        assert_eq!(latest["pty_id"], pty_id);
        assert_eq!(latest["pane_id"], "pane-latest");
        assert!(latest["byte_count"].as_u64().unwrap() >= "daemon-latest".len() as u64);
        assert!(!latest["replay_bytes_base64"].as_str().unwrap().is_empty());

        let _ = host.pty_handle.kill(&pty_id).await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_alias_commands_mutate_daemon_alias_state() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let alias_path = dir.path().join("aliases.json");
        let identity =
            DaemonIdentity::new_for_test_with_alias_path("/tmp/roux.sock", alias_path.clone());

        let set = handle_request(
            Request {
                command: "alias-set".to_string(),
                session_id: Some("session-alias".to_string()),
                pane_id: Some("pane-alias".to_string()),
                auth_token: None,
                args: serde_json::json!({ "alias": "reviewer" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(set.ok, "alias-set failed: {:?}", set.error);
        assert_eq!(set.data.as_ref().unwrap()["alias"], "reviewer");
        assert_eq!(set.data.as_ref().unwrap()["sessionId"], "session-alias");
        assert_eq!(set.data.as_ref().unwrap()["paneId"], "pane-alias");

        let get = handle_request(
            Request {
                command: "alias-get".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "alias": "reviewer" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(get.ok, "alias-get failed: {:?}", get.error);
        assert_eq!(get.data.as_ref().unwrap()["sessionId"], "session-alias");

        let reloaded_identity =
            DaemonIdentity::new_for_test_with_alias_path("/tmp/roux.sock", alias_path);
        let reloaded_get = handle_request(
            Request {
                command: "alias-get".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "alias": "reviewer" }),
            },
            &host,
            &reloaded_identity,
        )
        .await;
        assert!(reloaded_get.ok, "alias reload failed: {:?}", reloaded_get.error);
        assert_eq!(reloaded_get.data.as_ref().unwrap()["sessionId"], "session-alias");

        let whoami = handle_request(
            Request {
                command: "alias-whoami".to_string(),
                session_id: Some("session-alias".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(whoami.ok, "alias-whoami failed: {:?}", whoami.error);
        assert_eq!(whoami.data.as_ref().unwrap().as_array().unwrap().len(), 1);

        let unset = handle_request(
            Request {
                command: "alias-unset".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "alias": "reviewer" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(unset.ok, "alias-unset failed: {:?}", unset.error);
        assert_eq!(unset.data.as_ref().unwrap()["changed"], true);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_mailbox_and_bus_commands_mutate_daemon_state() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let alias_path = dir.path().join("aliases.json");
        let subscription_path = dir.path().join("subscriptions.json");
        let mailbox_events_path = dir.path().join("events.jsonl");
        let mailbox_read_state_path = dir.path().join("read_state.json");
        let identity = DaemonIdentity::new_for_test_with_runtime_paths(
            "/tmp/roux.sock",
            alias_path.clone(),
            subscription_path.clone(),
            mailbox_events_path.clone(),
            mailbox_read_state_path.clone(),
        );

        let subscribe = handle_request(
            Request {
                command: "bus-subscribe".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "alias": "auditor",
                    "pattern": "build.**",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(subscribe.ok, "bus-subscribe failed: {:?}", subscribe.error);
        let subscription_id = subscribe.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let post = handle_request(
            Request {
                command: "mailbox-post".to_string(),
                session_id: Some("session-sender".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "topic": "build.completed",
                    "body": "green",
                    "from": "builder",
                    "kind": "signal",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(post.ok, "mailbox-post failed: {:?}", post.error);
        let event_id = post.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let reloaded_identity = DaemonIdentity::new_for_test_with_runtime_paths(
            "/tmp/roux.sock",
            alias_path,
            subscription_path,
            mailbox_events_path,
            mailbox_read_state_path,
        );

        let read = handle_request(
            Request {
                command: "mailbox-read".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "alias": "auditor" }),
            },
            &host,
            &reloaded_identity,
        )
        .await;
        assert!(read.ok, "mailbox-read failed: {:?}", read.error);
        let events = read.data.as_ref().unwrap().as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], event_id);

        let count = handle_request(
            Request {
                command: "mailbox-count".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "alias": "auditor" }),
            },
            &host,
            &reloaded_identity,
        )
        .await;
        assert!(count.ok, "mailbox-count failed: {:?}", count.error);
        assert_eq!(count.data.as_ref().unwrap()["unread"], 0);

        let unsubscribe = handle_request(
            Request {
                command: "bus-unsubscribe".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": subscription_id }),
            },
            &host,
            &reloaded_identity,
        )
        .await;
        assert!(unsubscribe.ok, "bus-unsubscribe failed: {:?}", unsubscribe.error);
        assert_eq!(unsubscribe.data.as_ref().unwrap()["removed"], true);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_session_lifecycle_commands_mutate_state_and_ptys() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create = handle_request(
            Request {
                command: "session-create-shell".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "session-life",
                    "repoPath": dir.path(),
                    "name": "Lifecycle",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create.ok, "create failed: {:?}", create.error);

        let reconnect = handle_request(
            Request {
                command: "session-reconnect-shell".to_string(),
                session_id: Some("session-life".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "initialSize": [100, 30] }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(reconnect.ok, "reconnect failed: {:?}", reconnect.error);
        let pty = host
            .pty_handle
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|pty| pty.id == "session-life")
            .expect("primary pty after reconnect");
        assert_eq!(pty.cols, 100);
        assert_eq!(pty.rows, 30);

        let exists = handle_request(
            Request {
                command: "session-worktree-exists".to_string(),
                session_id: Some("session-life".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(exists.ok, "exists failed: {:?}", exists.error);
        assert_eq!(exists.data.as_ref().unwrap()["exists"], true);

        let archive = handle_request(
            Request {
                command: "session-archive".to_string(),
                session_id: Some("session-life".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(archive.ok, "archive failed: {:?}", archive.error);
        assert_eq!(archive.data.as_ref().unwrap()["archived"], true);
        assert!(host.pty_handle.list().await.unwrap().iter().all(|pty| pty
            .info
            .session_id
            .as_deref()
            != Some("session-life")));

        let restore = handle_request(
            Request {
                command: "session-restore".to_string(),
                session_id: Some("session-life".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(restore.ok, "restore failed: {:?}", restore.error);
        assert_eq!(restore.data.as_ref().unwrap()["archived"], false);
        assert_eq!(restore.data.as_ref().unwrap()["status"], "disconnected");

        let delete = handle_request(
            Request {
                command: "session-delete".to_string(),
                session_id: Some("session-life".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(delete.ok, "delete failed: {:?}", delete.error);
        assert!(host.session_handle.get("session-life").await.unwrap().is_none());

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_session_refresh_branch_updates_git_status_when_repo_is_initialized() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = make_session("s1");
        session.repo_root = dir.path().to_string_lossy().into_owned();
        session.worktree_path = session.repo_root.clone();
        session.is_git_repo = false;

        let services = RuntimeHostConfig {
            initial_sessions: vec![session],
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let init = Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
        assert!(
            init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );

        let refresh = handle_request(
            Request {
                command: "session-refresh-branch".to_string(),
                session_id: Some("s1".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(refresh.ok, "refresh failed: {:?}", refresh.error);

        let refreshed = host.session_handle.get("s1").await.unwrap().unwrap();
        assert!(refreshed.is_git_repo);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_project_and_session_metadata_commands_mutate_runtime_state() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: vec![make_session("s1")],
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let create = handle_request(
            Request {
                command: "project-create".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "name": "Alpha" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(create.ok, "project-create failed: {:?}", create.error);
        let project_id = create.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let update = handle_request(
            Request {
                command: "project-update".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": project_id,
                    "patch": { "name": "Beta", "contextPaths": ["/docs"] },
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(update.ok, "project-update failed: {:?}", update.error);
        assert_eq!(update.data.as_ref().unwrap()["name"], "Beta");
        assert_eq!(update.data.as_ref().unwrap()["contextPaths"][0], "/docs");

        let set_project = handle_request(
            Request {
                command: "session-set-project".to_string(),
                session_id: Some("s1".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "projectId": project_id }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(set_project.ok, "session-set-project failed: {:?}", set_project.error);

        let set_pinned = handle_request(
            Request {
                command: "session-set-pinned-pr-url".to_string(),
                session_id: Some("s1".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "url": "https://github.com/o/r/pull/1" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(set_pinned.ok, "session-set-pinned-pr-url failed: {:?}", set_pinned.error);

        let set_smol = handle_request(
            Request {
                command: "session-set-smol-machine".to_string(),
                session_id: Some("s1".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "machineName": "vm-a" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(set_smol.ok, "session-set-smol-machine failed: {:?}", set_smol.error);

        let session = host.session_handle.get("s1").await.unwrap().unwrap();
        assert_eq!(session.project_id.as_deref(), Some(project_id.as_str()));
        assert_eq!(session.pinned_pr_url.as_deref(), Some("https://github.com/o/r/pull/1"));
        assert_eq!(session.smol_machine_name.as_deref(), Some("vm-a"));

        let remove_project = handle_request(
            Request {
                command: "project-remove".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": project_id }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(remove_project.ok, "project-remove failed: {:?}", remove_project.error);
        let session = host.session_handle.get("s1").await.unwrap().unwrap();
        assert!(session.project_id.is_none());

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[tokio::test]
    async fn daemon_session_kill_alias_archives_session() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: vec![make_session("session-kill")],
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let kill = handle_request(
            Request {
                command: "session-kill".to_string(),
                session_id: Some("session-kill".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({}),
            },
            &host,
            &identity,
        )
        .await;
        assert!(kill.ok, "session-kill alias failed: {:?}", kill.error);
        assert_eq!(kill.data.as_ref().unwrap()["archived"], true);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
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
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
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
        for _ in 0..500 {
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
            if data["output"].as_str().unwrap_or("").contains("daemon-owned")
                && !data["record"]["running"].as_bool().unwrap_or(true)
            {
                output = Some(data);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let output = output.expect("daemon-owned output should be pollable");
        assert_eq!(output["record"]["id"], process_id);
        assert_eq!(output["record"]["running"], false);
        assert_eq!(output["record"]["exitCode"], 0);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_top_level_run_alias_starts_daemon_process() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let start = handle_request(
            Request {
                command: "run".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "command": "printf daemon-run-alias",
                    "working_dir": dir.path(),
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(start.ok, "run alias failed: {:?}", start.error);
        let process_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let mut output = None;
        for _ in 0..500 {
            let poll = handle_request(
                Request {
                    command: "daemon-process-output".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::json!({ "id": process_id, "maxBytes": 1024 }),
                },
                &host,
                &identity,
            )
            .await;
            assert!(poll.ok, "poll failed: {:?}", poll.error);
            let data = poll.data.unwrap();
            if data["output"].as_str().unwrap_or("").contains("daemon-run-alias")
                && !data["record"]["running"].as_bool().unwrap_or(true)
            {
                output = Some(data);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let output = output.expect("top-level run output should be daemon-owned");
        assert_eq!(output["record"]["exitCode"], 0);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_pty_spawn_task_and_output_poll_are_daemon_owned() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);

        let start = handle_request(
            Request {
                command: "daemon-pty-spawn-task".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "command": "printf daemon-pty-owned",
                    "workingDir": dir.path(),
                    "initialSize": [80, 24],
                    "sessionId": "session-a",
                    "paneId": "pane-a",
                    "profile": "task",
                }),
            },
            &host,
            &DaemonIdentity::new_for_test("/tmp/roux.sock"),
        )
        .await;
        assert!(start.ok, "start failed: {:?}", start.error);
        let pty_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        let mut output = None;
        for _ in 0..50 {
            let poll = handle_request(
                Request {
                    command: "daemon-pty-output".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::json!({ "id": pty_id, "maxBytes": 1024 }),
                },
                &host,
                &DaemonIdentity::new_for_test("/tmp/roux.sock"),
            )
            .await;
            assert!(poll.ok, "poll failed: {:?}", poll.error);
            let data = poll.data.unwrap();
            if data["output"].as_str().unwrap_or("").contains("daemon-pty-owned")
                && data["record"]["running"] == false
            {
                output = Some(data);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let output = output.expect("daemon-owned PTY output should be pollable");
        assert_eq!(output["record"]["id"], pty_id);
        assert_eq!(output["record"]["running"], false);
        assert_eq!(output["record"]["exitCode"], 0);
        assert_eq!(output["record"]["info"]["session_id"], "session-a");

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_top_level_send_writes_to_session_primary_pty() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = make_session("session-send");
        session.primary_pty_id = Some("primary-pty".to_string());
        let services = RuntimeHostConfig {
            initial_sessions: vec![session],
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let start = handle_request(
            Request {
                command: "daemon-pty-spawn-task".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "id": "primary-pty",
                    "command": "cat",
                    "workingDir": dir.path(),
                    "sessionId": "session-send",
                    "paneId": "pane-send",
                    "role": "sessionPrimary",
                    "profile": "shell",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(start.ok, "pty start failed: {:?}", start.error);

        let send = handle_request(
            Request {
                command: "send".to_string(),
                session_id: Some("session-send".to_string()),
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "text": "daemon-send-alias", "enter": true }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(send.ok, "send alias failed: {:?}", send.error);
        assert_eq!(send.data.as_ref().unwrap()["id"], "primary-pty");

        let mut output = None;
        for _ in 0..50 {
            let poll = handle_request(
                Request {
                    command: "daemon-pty-output".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::json!({ "id": "primary-pty", "maxBytes": 2048 }),
                },
                &host,
                &identity,
            )
            .await;
            assert!(poll.ok, "poll failed: {:?}", poll.error);
            let data = poll.data.unwrap();
            if data["output"].as_str().unwrap_or("").contains("daemon-send-alias") {
                output = Some(data);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        output.expect("sent text should appear in daemon PTY output");

        let _ = host.pty_handle.kill("primary-pty").await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_pty_spawn_request_populates_runtime_env() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux-daemon-test.sock");

        let start = handle_request(
            Request {
                command: "daemon-pty-spawn-task".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "command": "printf '%s|%s|%s|%s|%s|%s' \"$ROUX_SESSION_ID\" \"$ROUX_PANE_ID\" \"$ROUX_PROJECT_ID\" \"$ROUX_WORKTREE_PATH\" \"$ROUX_SOCKET\" \"$ROUX_NOTES_ROOT\"",
                    "workingDir": dir.path(),
                    "id": "pty-env",
                    "sessionId": "session-a",
                    "paneId": "pane-a",
                    "projectId": "project-a",
                    "worktreePath": "/worktrees/session-a",
                    "notesEnv": {
                        "vaultRoot": "/vault",
                        "sessionSlug": "session-a",
                        "repoSlug": "repo-a"
                    }
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(start.ok, "start failed: {:?}", start.error);

        let mut output = None;
        for _ in 0..50 {
            let response = handle_request(
                Request {
                    command: "daemon-pty-output".to_string(),
                    session_id: None,
                    pane_id: None,
                    auth_token: None,
                    args: serde_json::json!({ "id": "pty-env", "maxBytes": 4096 }),
                },
                &host,
                &identity,
            )
            .await;
            assert!(response.ok, "output failed: {:?}", response.error);
            let data = response.data.expect("output payload");
            if !data["record"]["running"].as_bool().unwrap_or(true) {
                output = data["output"].as_str().map(str::to_string);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            output.as_deref(),
            Some(
                "session-a|pane-a|project-a|/worktrees/session-a|/tmp/roux-daemon-test.sock|/vault"
            )
        );

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_pty_attach_stream_replays_output_and_exit() {
        use tokio::io::AsyncReadExt;

        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let start = handle_request(
            Request {
                command: "daemon-pty-spawn-task".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "command": "printf daemon-pty-stream",
                    "workingDir": dir.path(),
                    "sessionId": "session-a",
                    "paneId": "pane-a",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(start.ok, "start failed: {:?}", start.error);
        let pty_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

        for _ in 0..50 {
            let snapshot = host.pty_handle.snapshot(&pty_id, 1024).await.unwrap().unwrap();
            if snapshot.output.contains("daemon-pty-stream") && !snapshot.record.running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let (mut reader, mut writer) = tokio::io::duplex(8192);
        let ok = handle_daemon_pty_attach_stream(
            Request {
                command: "daemon-pty-attach".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": pty_id, "maxBytes": 1024 }),
            },
            &mut writer,
            &host,
            &identity,
        )
        .await;
        assert!(ok);
        drop(writer);

        let mut body = String::new();
        reader.read_to_string(&mut body).await.unwrap();
        let frames: Vec<serde_json::Value> =
            body.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(frames[0]["type"], "ready");
        assert_eq!(frames[0]["id"], pty_id);
        let replay_bytes: Vec<u8> = frames[0]["replayBytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|byte| byte.as_u64().unwrap() as u8)
            .collect();
        assert!(String::from_utf8_lossy(&replay_bytes).contains("daemon-pty-stream"));
        assert_eq!(frames[1]["type"], "exit");
        assert_eq!(frames[1]["code"], 0);

        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_pty_metadata_commands_mutate_info() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

        let start = handle_request(
            Request {
                command: "daemon-pty-spawn-task".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({
                    "command": "sleep 1",
                    "workingDir": dir.path(),
                    "id": "pty-meta",
                    "sessionId": "session-a",
                    "paneId": "pane-a",
                }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(start.ok, "start failed: {:?}", start.error);

        let detach = handle_request(
            Request {
                command: "daemon-pty-detach".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": "pty-meta" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(detach.ok, "detach failed: {:?}", detach.error);
        assert_eq!(detach.data.as_ref().unwrap()["info"]["status"]["type"], "RunningDetached");

        let attach = handle_request(
            Request {
                command: "daemon-pty-attach-pane".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": "pty-meta", "paneId": "pane-b" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(attach.ok, "attach failed: {:?}", attach.error);
        assert_eq!(attach.data.as_ref().unwrap()["info"]["status"]["pane_id"], "pane-b");

        let rename = handle_request(
            Request {
                command: "daemon-pty-set-name".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": "pty-meta", "name": "Build shell" }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(rename.ok, "rename failed: {:?}", rename.error);
        assert_eq!(rename.data.as_ref().unwrap()["info"]["name"], "Build shell");

        let clear = handle_request(
            Request {
                command: "daemon-pty-set-name".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": "pty-meta", "name": null }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(clear.ok, "clear failed: {:?}", clear.error);
        assert!(clear.data.as_ref().unwrap()["info"]["name"].is_null());

        let _ = host.pty_handle.kill("pty-meta").await;
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
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
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let watch_runner = WatchRunner::new(
            host.watch_handle.clone(),
            AutomationHookManager::from_config_root(dir.path()),
        );
        let log_path = dir.path().join("roux-daemon.log");
        let server = start_socket_server(
            host.clone(),
            watch_runner,
            DaemonIdentity::new(
                platform::SocketEndpoint::Unix(socket_path.clone()),
                log_path.clone(),
                None,
            ),
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
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn daemon_socket_stop_requests_shutdown_after_response() {
        use tokio::io::AsyncReadExt;

        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("roux.sock");
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
            work_item_db_path: dir.path().join("board.db"),
        }
        .build();
        let (host, joins) = services.spawn_with(tokio::spawn);
        let watch_runner = WatchRunner::new(
            host.watch_handle.clone(),
            AutomationHookManager::from_config_root(dir.path()),
        );
        let log_path = dir.path().join("roux-daemon.log");
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let server = start_socket_server(
            host.clone(),
            watch_runner,
            DaemonIdentity::new(
                platform::SocketEndpoint::Unix(socket_path.clone()),
                log_path.clone(),
                None,
            )
            .with_shutdown(shutdown_tx),
            DaemonLog::new_for_test(log_path),
        )
        .await
        .unwrap();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        stream.write_all(br#"{"command":"daemon-stop"}"#).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        let value: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["stopping"], true);

        for _ in 0..10 {
            if *shutdown_rx.borrow_and_update() {
                break;
            }
            let _ = shutdown_rx.changed().await;
        }
        assert!(*shutdown_rx.borrow(), "daemon-stop should signal shutdown");

        server.shutdown();
        host.process_handle.shutdown().await;
        host.pty_handle.shutdown().await;
        host.watch_handle.shutdown().await;
        host.session_handle.shutdown().await;
        host.project_handle.shutdown().await;
        drop(host);
        for join in joins {
            join.await.unwrap();
        }
    }

    #[test]
    fn request_authorized_requires_identity_token() {
        let identity = DaemonIdentity::new_for_test_with_endpoint(
            platform::SocketEndpoint::Tcp("127.0.0.1:7777".to_string()),
            Some("secret-token".to_string()),
        );
        let request = |auth_token: Option<&str>| Request {
            command: "daemon-status".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: auth_token.map(str::to_string),
            args: serde_json::json!({}),
        };

        assert!(!request_authorized(&request(None), &identity));
        assert!(!request_authorized(&request(Some("wrong-token")), &identity));
        assert!(request_authorized(&request(Some("secret-token")), &identity));
    }
}
