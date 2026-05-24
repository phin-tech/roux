use std::path::{Path, PathBuf};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
#[cfg(not(windows))]
use tokio::net::UnixListener;

use roux_runtime::host::RuntimeHost;
use roux_runtime::watch_runner::WatchRunner;

use crate::{daemon_log::DaemonLog, platform};

use super::handle_request_with_watch_runner;
use super::identity::{endpoint_path, DaemonIdentity};
use super::protocol::{Request, Response};
use super::streams::{
    handle_alias_events_stream, handle_daemon_pty_attach_stream, handle_mailbox_events_stream,
    handle_subscription_events_stream, handle_watch_events_stream,
};

pub(super) struct SocketServerHandle {
    join: tokio::task::JoinHandle<()>,
    cleanup: SocketCleanup,
    pub(super) endpoint: platform::SocketEndpoint,
}

impl SocketServerHandle {
    pub(super) fn shutdown(self) {
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

pub(super) async fn start_socket_server(
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
