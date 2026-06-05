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
    handle_session_events_stream, handle_subscription_events_stream, handle_watch_events_stream,
};
use super::work_items::handle_work_item_events_stream;

pub(super) struct SocketServerHandle {
    join: tokio::task::JoinHandle<()>,
    cleanup: SocketCleanup,
    owner_guard: SocketOwnerGuard,
    pub(super) endpoint: platform::SocketEndpoint,
}

impl SocketServerHandle {
    pub(super) fn shutdown(self) -> SocketOwnerGuard {
        let Self { join, cleanup, owner_guard, endpoint: _ } = self;
        join.abort();
        cleanup.remove();
        owner_guard
    }
}

#[cfg(not(windows))]
pub(super) struct SocketOwnerGuard {
    _file: std::fs::File,
}

#[cfg(windows)]
pub(super) struct SocketOwnerGuard {
    _file: std::fs::File,
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
    owner_guard: SocketOwnerGuard,
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
                    owner_guard,
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
                owner_guard,
                endpoint,
            })
        }
    }
}

#[cfg(not(windows))]
pub(super) fn acquire_daemon_owner(
    lock_path: &Path,
    endpoint: &str,
) -> Result<SocketOwnerGuard, String> {
    use std::os::fd::AsRawFd;

    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!("create daemon owner lock directory {}: {err}", parent.display())
        })?;
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|err| format!("open daemon socket owner lock {}: {err}", lock_path.display()))?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::WouldBlock {
            return Err(daemon_already_running_message(endpoint));
        }
        return Err(format!("lock daemon socket owner {}: {err}", lock_path.display()));
    }

    Ok(SocketOwnerGuard { _file: file })
}

#[cfg(windows)]
pub(super) fn acquire_daemon_owner(
    lock_path: &Path,
    endpoint: &str,
) -> Result<SocketOwnerGuard, String> {
    use std::os::windows::fs::OpenOptionsExt;

    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!("create daemon owner lock directory {}: {err}", parent.display())
        })?;
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(lock_path)
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock => {
                daemon_already_running_message(endpoint)
            }
            _ => format!("open daemon socket owner lock {}: {err}", lock_path.display()),
        })?;

    Ok(SocketOwnerGuard { _file: file })
}

fn daemon_already_running_message(attempted_endpoint: &str) -> String {
    format!("Roux daemon already running; attempted endpoint: {attempted_endpoint}")
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
    write_private_file(
        &platform::socket_addr_file_path(),
        format!("tcp://{local_addr}").as_bytes(),
        "daemon socket endpoint",
    )?;
    let token = identity.auth_token.as_deref().unwrap_or_default();
    write_private_file(
        &platform::socket_auth_token_file_path(),
        token.as_bytes(),
        "daemon socket token",
    )?;
    Ok(listener)
}

#[cfg(unix)]
fn write_private_file(path: &Path, contents: &[u8], label: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|err| format!("write {label}: {err}"))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("set {label} permissions: {err}"))?;
    file.write_all(contents).map_err(|err| format!("write {label}: {err}"))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, contents: &[u8], label: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|err| format!("write {label}: {err}"))
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
                    let ok = handle_session_events_stream(req, writer, host, identity).await;
                    if ok {
                        log.write("Handled socket command: session-events");
                    } else {
                        log.write("Socket command failed: session-events");
                    }
                    return;
                }
                if command == "work-item-events" {
                    let ok = handle_work_item_events_stream(req, writer, host, identity).await;
                    if ok {
                        log.write("Handled socket command: work-item-events");
                    } else {
                        log.write("Socket command failed: work-item-events");
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn write_private_file_sets_owner_only_permissions_on_unix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("socket-token");

        write_private_file(&path, b"secret", "test token").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private_file(&path, b"secret-2", "test token").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret-2");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn daemon_owner_lock_prevents_second_owner_and_leaves_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("roux-daemon.lock");

        let first = acquire_daemon_owner(&lock_path, "unix:///tmp/roux.sock").unwrap();
        assert!(lock_path.exists());

        let second = match acquire_daemon_owner(&lock_path, "tcp://127.0.0.1:0") {
            Ok(_) => panic!("second owner should fail while first guard is held"),
            Err(err) => err,
        };
        assert_eq!(second, "Roux daemon already running; attempted endpoint: tcp://127.0.0.1:0");

        drop(first);
        assert!(lock_path.exists(), "flock path should remain as a reusable inode");

        let third = acquire_daemon_owner(&lock_path, "test endpoint")
            .expect("released lock file should be reusable");
        drop(third);
    }
}
