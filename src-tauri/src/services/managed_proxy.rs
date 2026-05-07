//! Managed HTTP proxy lifecycle. Roux runs a user-installed proxy
//! (tinyproxy / mitmproxy / squid / whatever) as a child process and
//! gives the smol machines panel a "Start / Stop proxy" toggle.
//!
//! Roux does **not** bundle, link, or ship any proxy implementation.
//! The user installs their preferred proxy and configures the start
//! command in Settings. License-clean by construction.
//!
//! Lifecycle invariants:
//! - At most one managed proxy child runs at a time.
//! - On Roux quit, the child is killed via the registered shutdown
//!   path (caller's responsibility — `stop()` is idempotent and
//!   safe to call from a Drop or a signal handler).
//! - Start verifies success by polling the listen socket up to ~5s
//!   before declaring failure.

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use roux_core::ManagedProxyConfig;
use serde::Serialize;

/// Snapshot of the managed proxy's runtime state. Surfaced to the
/// frontend via `cmd_managed_proxy_status`.
#[derive(Debug, Clone, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedProxyStatus {
    /// `true` when a managed proxy child is alive and the listen
    /// socket has accepted at least one probe connection. `false`
    /// includes both "never started" and "started then crashed".
    pub running: bool,
    pub port: Option<u16>,
    pub bind: Option<String>,
    pub pid: Option<u32>,
    /// stderr tail captured when the child failed to start or
    /// exited unexpectedly. Cleared on successful start.
    pub last_error: Option<String>,
}

/// Mutable state held in `AppState`. Wrapped in a Mutex because
/// start/stop can race with the periodic status poll the panel runs.
pub(crate) struct ManagedProxyState {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Child process handle when running. `None` after stop or on
    /// initial state.
    child: Option<Child>,
    /// Mirror of `child.id()` so status reads don't need to lock the
    /// child handle (which holds an OS-level resource we'd rather
    /// not block on).
    pid: Option<u32>,
    port: Option<u16>,
    bind: Option<String>,
    last_error: Option<String>,
}

impl ManagedProxyState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                child: None,
                pid: None,
                port: None,
                bind: None,
                last_error: None,
            }),
        })
    }

    pub fn status(&self) -> ManagedProxyStatus {
        // Mutex lock is short — we just read fields. We don't try to
        // detect "child crashed since last call" here; that surfaces
        // on the next start attempt or when the user clicks stop.
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        // Reap a finished child so `running` flips to false without
        // requiring an explicit stop call.
        if let Some(child) = guard.child.as_mut() {
            if let Ok(Some(_status)) = child.try_wait() {
                guard.child = None;
                guard.pid = None;
            }
        }
        ManagedProxyStatus {
            running: guard.child.is_some(),
            port: guard.port,
            bind: guard.bind.clone(),
            pid: guard.pid,
            last_error: guard.last_error.clone(),
        }
    }

    pub fn start(&self, config: &ManagedProxyConfig) -> Result<ManagedProxyStatus, String> {
        // Stop any existing child first so a settings-change-then-start
        // sequence doesn't leave a stale proxy on the old port.
        self.stop_inner();

        let bind = config.bind.as_deref().unwrap_or("127.0.0.1").to_string();
        let port = config.port;
        let command = config.command.trim();
        if command.is_empty() {
            return Err("managed proxy command is empty".to_string());
        }

        // Spawn via the user's login shell so PATH / aliases /
        // `~/.config/...` references in the command resolve. We use
        // `sh -lc` rather than the user's $SHELL because tinyproxy/
        // mitmproxy don't need shell features and `sh` is universally
        // available.
        let mut cmd = Command::new("sh");
        cmd.args(["-lc", command])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child =
            cmd.spawn().map_err(|e| format!("failed to spawn `{command}`: {e}"))?;
        let pid = child.id();

        // Capture stderr in the background so we have something to
        // surface if the proxy dies. ~64KB ring would be ideal; for
        // v1 we just keep the most-recent N lines.
        let stderr = child.stderr.take();
        let stderr_buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        if let Some(stderr) = stderr {
            let buf = Arc::clone(&stderr_buf);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let mut guard = buf.lock().unwrap();
                    guard.push(line);
                    // Keep only the last 32 lines.
                    let len = guard.len();
                    if len > 32 {
                        guard.drain(0..(len - 32));
                    }
                }
            });
        }

        // Poll the listen socket. tinyproxy + most proxies bind
        // within a few hundred ms; we give 5s before declaring
        // failure to cover slower-starting tools (mitmproxy on first
        // run can be slow to compile its CA cert).
        let deadline = Instant::now() + Duration::from_secs(5);
        let addr = format!("{bind}:{port}");
        let mut last_connect_err: Option<std::io::Error> = None;
        let mut bound = false;
        while Instant::now() < deadline {
            // Check whether the child has already exited — no point
            // waiting for a socket that won't open.
            if let Ok(Some(status)) = child.try_wait() {
                let stderr_tail =
                    stderr_buf.lock().unwrap().join("\n");
                let mut guard = self.inner.lock().unwrap();
                guard.last_error = Some(format!(
                    "proxy exited before binding ({status}): {stderr_tail}",
                ));
                return Err(guard.last_error.clone().unwrap());
            }
            match TcpStream::connect_timeout(
                &addr.parse().map_err(|e| format!("invalid bind {addr}: {e}"))?,
                Duration::from_millis(100),
            ) {
                Ok(_) => {
                    bound = true;
                    break;
                }
                Err(e) => {
                    last_connect_err = Some(e);
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        if !bound {
            // Kill the child — it won't bind, so it's just a
            // resource leak otherwise. Capture stderr for the error
            // message before killing.
            let _ = child.kill();
            let _ = child.wait();
            let stderr_tail = stderr_buf.lock().unwrap().join("\n");
            let connect_err = last_connect_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "no connection attempted".into());
            let msg = format!(
                "proxy didn't accept connections on {addr} within 5s ({connect_err}): {stderr_tail}",
            );
            let mut guard = self.inner.lock().unwrap();
            guard.last_error = Some(msg.clone());
            return Err(msg);
        }

        let mut guard = self.inner.lock().unwrap();
        guard.child = Some(child);
        guard.pid = Some(pid);
        guard.port = Some(port);
        guard.bind = Some(bind);
        guard.last_error = None;
        Ok(ManagedProxyStatus {
            running: true,
            port: guard.port,
            bind: guard.bind.clone(),
            pid: guard.pid,
            last_error: None,
        })
    }

    pub fn stop(&self) -> ManagedProxyStatus {
        self.stop_inner();
        self.status()
    }

    fn stop_inner(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut child) = guard.child.take() {
            terminate_gracefully(&mut child);
        }
        guard.pid = None;
        // Keep `port` / `bind` so the panel can still display them
        // as the "last running" config until next start.
    }
}

/// Send SIGTERM, poll up to 2 seconds for graceful exit, then fall back
/// to SIGKILL. Most proxies (tinyproxy, mitmproxy, squid) flush their
/// listen socket and a small amount of state on SIGTERM; SIGKILL skips
/// that and can leave torn writes for proxies that persist anything
/// (e.g. mitmproxy's CA cert + flow log). On Windows, `Child::kill` is
/// already the only available signal, so the graceful step is a no-op
/// there.
fn terminate_gracefully(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        // SIGTERM — give the proxy a chance to clean up.
        let pid = child.id() as libc::pid_t;
        if libc::kill(pid, libc::SIGTERM) == 0 {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if let Ok(Some(_status)) = child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    // Fallback (Unix: didn't exit in 2s, or kill() syscall failed;
    // Windows: only available signal). SIGKILL on Unix, TerminateProcess
    // on Windows.
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for ManagedProxyState {
    fn drop(&mut self) {
        // Best-effort kill on app shutdown. Tauri tears down AppState
        // before the process exits, so this catches the common case.
        self.stop_inner();
    }
}
