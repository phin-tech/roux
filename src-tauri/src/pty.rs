use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{
    ipc::{Channel, Response},
    Emitter,
};
use thiserror::Error;

use crate::platform;
use crate::pty_ready_gate::ShellReadyGate;

pub use roux_core::{PtyInfo, PtyRole, PtyStatus};

type PtyWriter = Arc<Mutex<Box<dyn std::io::Write + Send>>>;
type ReadyGate = Arc<Mutex<ShellReadyGate>>;

const GATE_QUIET: Duration = Duration::from_millis(200);
const GATE_TIMEOUT: Duration = Duration::from_secs(5);
const GATE_TICK: Duration = Duration::from_millis(75);

/// Fallback PTY size for spawn paths that don't have a measured pane size.
/// Callers should pass the pane's actual `(cols, rows)` whenever possible
/// — starting at the real size avoids a post-spawn SIGWINCH, which
/// otherwise triggers `zle reset-prompt` in zsh and causes async prompt
/// frameworks (oh-my-zsh git, p10k without instant-prompt, etc.) to
/// redraw on top of any keystrokes the user has already typed.
const DEFAULT_PTY_COLS: u16 = 80;
const DEFAULT_PTY_ROWS: u16 = 24;

fn pty_size_from(initial: Option<(u16, u16)>) -> PtySize {
    let (cols, rows) = initial.unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
    PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 }
}

/// Best-effort flush of bytes the gate released. Errors are logged but
/// swallowed: the reader/tick threads can't do anything useful with a
/// broken writer, and panicking would take the PTY thread down.
fn flush_to_writer(writer: &PtyWriter, bytes: &[u8], context: &str) {
    if bytes.is_empty() {
        return;
    }
    use std::io::Write;
    let mut w = match writer.lock() {
        Ok(g) => g,
        Err(e) => {
            rlog!("pty_ready_gate: writer mutex poisoned ({}): {}", context, e);
            return;
        }
    };
    if let Err(e) = w.write_all(bytes).and_then(|_| w.flush()) {
        rlog!("pty_ready_gate: flush failed ({}): {}", context, e);
    }
}

/// Periodic tick until the gate opens via quiescence or timeout. Self-
/// terminates as soon as `poll()` reports open — no long-lived thread
/// per session.
fn spawn_gate_ticker(gate: ReadyGate, writer: PtyWriter, session_id: String) {
    thread::spawn(move || loop {
        thread::sleep(GATE_TICK);
        let (opened_now, bytes) = {
            let mut g = match gate.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if g.is_open() {
                return;
            }
            let flush = g.poll(Instant::now());
            (g.is_open(), flush)
        };
        flush_to_writer(&writer, &bytes, &format!("tick({})", session_id));
        if opened_now {
            return;
        }
    });
}

enum PtyChunk {
    Data(Vec<u8>),
    Eof,
    Error,
}

const PTY_BACKLOG_LIMIT_BYTES: usize = 256 * 1024;

struct PtyOutputState {
    channel: Option<Channel<Response>>,
    backlog: VecDeque<Vec<u8>>,
    backlog_bytes: usize,
    logger: Option<Arc<Mutex<crate::pty_logger::PtyLogger>>>,
}

impl PtyOutputState {
    #[cfg(test)]
    fn new() -> Self {
        Self { channel: None, backlog: VecDeque::new(), backlog_bytes: 0, logger: None }
    }

    fn new_with_logger(logger: Arc<Mutex<crate::pty_logger::PtyLogger>>) -> Self {
        Self {
            channel: None,
            backlog: VecDeque::new(),
            backlog_bytes: 0,
            logger: Some(logger),
        }
    }

    fn buffer(&mut self, bytes: Vec<u8>) {
        self.backlog_bytes += bytes.len();
        self.backlog.push_back(bytes);
        while self.backlog_bytes > PTY_BACKLOG_LIMIT_BYTES {
            let Some(removed) = self.backlog.pop_front() else {
                break;
            };
            self.backlog_bytes = self.backlog_bytes.saturating_sub(removed.len());
        }
    }

    fn send_or_buffer(&mut self, bytes: Vec<u8>) {
        // Log every byte headed to the frontend (best-effort).
        if let Some(ref logger) = self.logger {
            if let Ok(mut l) = logger.lock() {
                l.write(&bytes);
            }
        }
        if let Some(channel) = &self.channel {
            if channel.send(Response::new(bytes.clone())).is_ok() {
                return;
            }
            self.channel = None;
        }
        self.buffer(bytes);
    }

    fn attach(&mut self, channel: Channel<Response>) {
        self.channel = Some(channel);
        while let Some(bytes) = self.backlog.pop_front() {
            self.backlog_bytes = self.backlog_bytes.saturating_sub(bytes.len());
            let Some(channel) = &self.channel else {
                self.buffer(bytes);
                break;
            };
            if channel.send(Response::new(bytes.clone())).is_err() {
                self.channel = None;
                self.buffer(bytes);
                break;
            }
        }
    }
}

#[derive(Clone)]
struct PtyOutput {
    state: Arc<Mutex<PtyOutputState>>,
}

impl PtyOutput {
    #[cfg(test)]
    fn new() -> Self {
        Self { state: Arc::new(Mutex::new(PtyOutputState::new())) }
    }

    fn new_with_logger(logger: Arc<Mutex<crate::pty_logger::PtyLogger>>) -> Self {
        Self { state: Arc::new(Mutex::new(PtyOutputState::new_with_logger(logger))) }
    }

    fn send(&self, bytes: Vec<u8>) {
        self.state.lock().unwrap().send_or_buffer(bytes);
    }

    fn attach(&self, channel: Channel<Response>) {
        self.state.lock().unwrap().attach(channel);
    }
}

/// Spawn a flusher thread that batches chunks from the reader and sends them to the frontend
/// at ~16ms intervals. Returns the sender for the reader thread to push data into.
/// Optional "let the agent registry know this session is gone"
/// plumbing, bundled alongside the Tauri event emission at EOF.
type ExitRegistryHook = (mpsc::Sender<crate::agent_registry::RegistryMessage>, String);

fn spawn_flusher(
    output: PtyOutput,
    exit_event: Option<(String, u64)>, // (event_name, generation)
    app: tauri::AppHandle,
    exit_registry_hook: Option<ExitRegistryHook>,
) -> mpsc::Sender<PtyChunk> {
    let (tx, rx) = mpsc::channel::<PtyChunk>();

    thread::spawn(move || {
        let flush_interval = Duration::from_millis(16);
        let mut batch = Vec::with_capacity(8192);
        let mut last_flush = Instant::now();

        loop {
            // If batch is empty, block until data arrives
            // If batch has data, use timeout to ensure timely flush
            let chunk = if batch.is_empty() {
                match rx.recv() {
                    Ok(c) => c,
                    Err(_) => break,
                }
            } else {
                let remaining = flush_interval.saturating_sub(last_flush.elapsed());
                match rx.recv_timeout(remaining) {
                    Ok(c) => c,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Flush what we have
                        output.send(std::mem::take(&mut batch));
                        batch.clear();
                        last_flush = Instant::now();
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            };

            match chunk {
                PtyChunk::Data(data) => {
                    batch.extend_from_slice(&data);
                    if last_flush.elapsed() >= flush_interval || batch.len() >= 32768 {
                        output.send(std::mem::take(&mut batch));
                        last_flush = Instant::now();
                    }
                }
                PtyChunk::Eof => {
                    if !batch.is_empty() {
                        output.send(std::mem::take(&mut batch));
                    }
                    if let Some((evt, gen)) = &exit_event {
                        let _ = app.emit(
                            evt,
                            &roux_core::SessionExitPayload {
                                code: None,
                                generation: *gen,
                                reason: roux_core::SessionExitReason::Exit,
                            },
                        );
                    }
                    if let Some((tx, sid)) = &exit_registry_hook {
                        let _ = tx.send(
                            crate::agent_registry::RegistryMessage::SessionEnded {
                                session_id: sid.clone(),
                            },
                        );
                    }
                    break;
                }
                PtyChunk::Error => {
                    if !batch.is_empty() {
                        output.send(std::mem::take(&mut batch));
                    }
                    if let Some((evt, gen)) = &exit_event {
                        let _ = app.emit(
                            evt,
                            &roux_core::SessionExitPayload {
                                code: None,
                                generation: *gen,
                                reason: roux_core::SessionExitReason::IoError,
                            },
                        );
                    }
                    if let Some((tx, sid)) = &exit_registry_hook {
                        let _ = tx.send(
                            crate::agent_registry::RegistryMessage::SessionEnded {
                                session_id: sid.clone(),
                            },
                        );
                    }
                    break;
                }
            }
        }
    });

    tx
}

/// Spawn a flusher thread that sends lifecycle events instead of directly emitting Tauri events.
/// This is the new pattern — exit handling is centralized in the lifecycle bus.
fn spawn_flusher_with_lifecycle(
    output: PtyOutput,
    pty_id: String,
    session_id: Option<String>,
    generation: u64,
    lifecycle_tx: crate::pty_lifecycle::LifecycleTx,
    emit_exit_event: bool,
) -> mpsc::Sender<PtyChunk> {
    let (tx, rx) = mpsc::channel::<PtyChunk>();

    thread::spawn(move || {
        let flush_interval = Duration::from_millis(16);
        let mut batch = Vec::with_capacity(8192);
        let mut last_flush = Instant::now();

        loop {
            let chunk = if batch.is_empty() {
                match rx.recv() {
                    Ok(c) => c,
                    Err(_) => break,
                }
            } else {
                let remaining = flush_interval.saturating_sub(last_flush.elapsed());
                match rx.recv_timeout(remaining) {
                    Ok(c) => c,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        output.send(std::mem::take(&mut batch));
                        batch.clear();
                        last_flush = Instant::now();
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            };

            match chunk {
                PtyChunk::Data(data) => {
                    batch.extend_from_slice(&data);
                    if last_flush.elapsed() >= flush_interval || batch.len() >= 32768 {
                        output.send(std::mem::take(&mut batch));
                        last_flush = Instant::now();
                    }
                }
                PtyChunk::Eof => {
                    if !batch.is_empty() {
                        output.send(std::mem::take(&mut batch));
                    }
                    if emit_exit_event {
                        let _ = lifecycle_tx.send(crate::pty_lifecycle::PtyLifecycleMessage::Event(
                            crate::pty_lifecycle::PtyLifecycleEvent::Exited {
                                pty_id: pty_id.clone(),
                                session_id: session_id.clone(),
                                code: None,
                                reason: crate::pty_lifecycle::ExitReason::Exit,
                                generation,
                            },
                        ));
                    }
                    break;
                }
                PtyChunk::Error => {
                    if !batch.is_empty() {
                        output.send(std::mem::take(&mut batch));
                    }
                    if emit_exit_event {
                        let _ = lifecycle_tx.send(crate::pty_lifecycle::PtyLifecycleMessage::Event(
                            crate::pty_lifecycle::PtyLifecycleEvent::Exited {
                                pty_id: pty_id.clone(),
                                session_id: session_id.clone(),
                                code: None,
                                reason: crate::pty_lifecycle::ExitReason::IoError,
                                generation,
                            },
                        ));
                    }
                    break;
                }
            }
        }
    });

    tx
}

/// Spawn a reader thread that blocks on PTY reads and sends chunks to the flusher.
/// If `sniffer` is provided, every chunk is also fed through the OSC parser
/// before being forwarded (non-consuming — bytes pass through unchanged).
fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: mpsc::Sender<PtyChunk>,
    mut sniffer: Option<crate::notifications::OscSniffer>,
    gate: Option<(ReadyGate, PtyWriter, String)>,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(PtyChunk::Eof);
                    break;
                }
                Ok(n) => {
                    if let Some(ref mut s) = sniffer {
                        s.feed(&buf[..n]);
                    }
                    // Feed the readiness gate. If this output opens the
                    // gate and had writes buffered, flush them back into
                    // the PTY so the user's typed command actually runs.
                    // Must not short-circuit the tx.send below — a
                    // poisoned gate mutex would otherwise silently stop
                    // output forwarding for the session.
                    if let Some((ref g, ref w, ref id)) = gate {
                        if let Ok(mut guard) = g.lock() {
                            let flush = guard.on_output(&buf[..n], Instant::now());
                            drop(guard);
                            flush_to_writer(w, &flush, &format!("reader({})", id));
                        } else {
                            rlog!(
                                "pty_ready_gate: reader saw poisoned gate mutex, skipping feed for {}",
                                id,
                            );
                        }
                    }
                    if tx.send(PtyChunk::Data(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(PtyChunk::Error);
                    break;
                }
            }
        }
    });
}

/// Placeholder for a child process that's already being waited on by another thread.
#[derive(Debug)]
struct WaitedChild;

impl portable_pty::ChildKiller for WaitedChild {
    fn kill(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(WaitedChild)
    }
}

impl portable_pty::Child for WaitedChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        Err(std::io::Error::other("child already waited"))
    }
    fn process_id(&self) -> Option<u32> {
        None
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<*mut std::ffi::c_void> {
        None
    }
}

/// Information captured when a PTY exits.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub at_ms: u64,
    pub was_attached: bool,
}

pub(crate) struct PtySession {
    master: Box<dyn MasterPty + Send>,
    #[allow(dead_code)]
    child: Box<dyn portable_pty::Child + Send>,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    output: PtyOutput,
    generation: u64,
    /// Write-side readiness gate. Present only for shell spawns, which
    /// are the only PTYs where early-write races bite (shells take time
    /// to initialise ZLE/readline and drop stdin that arrives before).
    /// `None` means writes pass through unchecked.
    ready_gate: Option<ReadyGate>,

    // --- attach/detach metadata ---
    pub role: PtyRole,
    pub status: PtyStatus,
    pub exit_info: Option<ExitInfo>,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub working_dir: Option<String>,
    pub profile: Option<String>,
    #[allow(dead_code)] // Reserved for future PTY size tracking
    pub last_size: (u16, u16),
    pub last_activity: std::time::Instant,
    pub unread_output: bool,
    pub bell_pending: bool,
    pub logger: Option<Arc<Mutex<crate::pty_logger::PtyLogger>>>,
}

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("Failed to open PTY: {source}")]
    OpenPty {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to spawn shell: {source}")]
    SpawnShell {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to spawn task: {source}")]
    SpawnTask {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to get PTY writer: {source}")]
    GetWriter {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to get PTY reader: {source}")]
    GetReader {
        #[source]
        source: anyhow::Error,
    },
    #[error("Session {session_id} not found")]
    SessionNotFound { session_id: String },
    #[error("Write failed: {source}")]
    WriteFailed {
        #[source]
        source: std::io::Error,
    },
    #[error("Flush failed: {source}")]
    FlushFailed {
        #[source]
        source: std::io::Error,
    },
    #[error("Resize failed: {source}")]
    ResizeFailed {
        #[source]
        source: anyhow::Error,
    },
}

/// Look up the current working directory of an OS process by PID.
///
/// Used to report live cwd for shell panes at save time so reconnecting a
/// session restores the user's actual directory (after `cd`s), not just the
/// directory the shell was originally spawned in. The kernel tracks cwd on
/// the shell process itself, so this pulls directly from OS-level process
/// info — no shell integration or OSC 7 cooperation required.
///
/// Returns `None` if the PID doesn't exist, the caller lacks permission, or
/// the OS refuses.
#[cfg(target_os = "macos")]
pub fn cwd_for_pid(pid: u32) -> Option<String> {
    // proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, &mut info, sizeof(info))
    // fills a proc_vnodepathinfo struct; its pvi_cdir.vip_path is the cwd as
    // a NUL-terminated C string of length MAXPATHLEN (1024 on Darwin).
    //
    // We only need the cwd byte range, not the full struct layout, so we
    // define a minimally-sufficient stand-in whose size matches the real
    // struct. proc_vnodepathinfo is two vnode_info_path structs back-to-back;
    // the first is pvi_cdir (what we want). Each vnode_info_path is
    // sizeof(vnode_info) + MAXPATHLEN; vnode_info is 152 bytes on Darwin.
    const MAXPATHLEN: usize = 1024;
    const VNODE_INFO_SIZE: usize = 152;
    const VNODE_INFO_PATH_SIZE: usize = VNODE_INFO_SIZE + MAXPATHLEN;
    const PROC_PIDVNODEPATHINFO: libc::c_int = 9;

    #[repr(C)]
    struct ProcVnodePathInfo {
        pvi_cdir: [u8; VNODE_INFO_PATH_SIZE],
        pvi_rdir: [u8; VNODE_INFO_PATH_SIZE],
    }

    extern "C" {
        fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    let mut info = ProcVnodePathInfo {
        pvi_cdir: [0u8; VNODE_INFO_PATH_SIZE],
        pvi_rdir: [0u8; VNODE_INFO_PATH_SIZE],
    };
    let size = std::mem::size_of::<ProcVnodePathInfo>() as libc::c_int;

    let ret = unsafe {
        proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if ret <= 0 {
        return None;
    }

    // vip_path starts at offset VNODE_INFO_SIZE within vnode_info_path and is
    // a C string (MAXPATHLEN bytes, NUL-terminated).
    let path_bytes = &info.pvi_cdir[VNODE_INFO_SIZE..];
    let nul = path_bytes.iter().position(|&b| b == 0).unwrap_or(path_bytes.len());
    if nul == 0 {
        return None;
    }
    std::str::from_utf8(&path_bytes[..nul]).ok().map(|s| s.to_string())
}

#[cfg(target_os = "linux")]
pub fn cwd_for_pid(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{}/cwd", pid)).ok().map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn cwd_for_pid(_pid: u32) -> Option<String> {
    None
}

/// Nono sandbox configuration for a shell PTY. When present, the shell
/// is spawned inside `nono run` with the given profile and directory
/// allowances.
#[derive(Debug, Clone)]
pub struct NonoConfig {
    pub profile: String,
    pub allow_dirs: Vec<String>,
}

/// Smolvm exec configuration for a shell PTY. When present, the shell is
/// spawned inside `smolvm machine exec --name <machine_name> -it -- <guest_shell>`
/// instead of directly on the host. Mutually exclusive with `NonoConfig`
/// — nono is host-side and doesn't exist inside a guest VM unless the
/// image installs it; the spawn path silently skips nono when smolvm is
/// set.
#[derive(Debug, Clone)]
pub struct SmolvmExec {
    /// Resolved `smolvm` binary path, from
    /// [`crate::services::smolvm::resolve_smolvm_binary`]. Owning this
    /// (rather than re-resolving in the spawn path) means a smolvm
    /// uninstall after the session was bound surfaces as a clean failure
    /// at the caller, not a confusing "command not found" mid-spawn.
    pub binary: PathBuf,
    pub machine_name: String,
    /// Guest shell path. v1 hardcodes `/bin/sh` at the call site;
    /// future Smolfile-derived override will plug in here.
    pub guest_shell: String,
}

impl NonoConfig {
    /// Resolve `~` to the user's home directory and relative paths
    /// against `working_dir`. Nono receives arguments via CommandBuilder
    /// (no shell expansion), so tilde must be expanded here.
    ///
    /// Uses `Path::is_absolute()` for portability: on Windows this
    /// correctly treats `C:\foo` as absolute. Falls back to silently
    /// dropping `~`-prefixed entries when `HOME` is unavailable rather
    /// than emitting a bogus `--allow-dir` with a relative path.
    pub fn resolved_allow_dirs(&self, working_dir: &str) -> Vec<String> {
        let home = dirs::home_dir();
        self.allow_dirs
            .iter()
            .filter_map(|d| {
                if d == "~" {
                    home.as_ref().map(|h| h.to_string_lossy().into_owned())
                } else if let Some(tail) = d.strip_prefix("~/") {
                    home.as_ref().map(|h| h.join(tail).to_string_lossy().into_owned())
                } else if std::path::Path::new(d).is_absolute() {
                    Some(d.clone())
                } else {
                    Some(std::path::Path::new(working_dir).join(d).to_string_lossy().into_owned())
                }
            })
            .collect()
    }
}

pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
    pending_outputs: Mutex<HashMap<String, Channel<Response>>>,
    generation: AtomicU64,
    /// Set once at app startup. When present, PTY exit triggers a
    /// `RegistryMessage::SessionEnded` broadcast so the agent FSM can
    /// dismiss any lingering attention notifications for this
    /// session's panes. Held in a mutex purely to accommodate the
    /// "construct PtyManager, then later plumb the channel" order that
    /// falls out of Tauri's setup flow.
    agent_sender: Mutex<Option<mpsc::Sender<crate::agent_registry::RegistryMessage>>>,
    /// Lifecycle event bus sender. When present, PTY exit/detach events
    /// are sent to the centralized lifecycle handler instead of being
    /// handled inline. Set via `set_lifecycle_tx` at app startup.
    lifecycle_tx: Mutex<Option<crate::pty_lifecycle::LifecycleTx>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            pending_outputs: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            agent_sender: Mutex::new(None),
            lifecycle_tx: Mutex::new(None),
        }
    }

    /// Install the lifecycle event bus sender. PTYs spawned after this
    /// will send exit events to the centralized handler instead of
    /// emitting Tauri events directly. Safe to call multiple times.
    pub fn set_lifecycle_tx(&self, tx: crate::pty_lifecycle::LifecycleTx) {
        *self.lifecycle_tx.lock().unwrap() = Some(tx);
    }

    /// Install the registry sender. Any PTY spawned after this is
    /// wired to broadcast `SessionEnded` when it exits. Safe to call
    /// multiple times — last writer wins.
    pub fn set_agent_sender(
        &self,
        sender: mpsc::Sender<crate::agent_registry::RegistryMessage>,
    ) {
        *self.agent_sender.lock().unwrap() = Some(sender);
    }

    fn exit_registry_info(
        &self,
        session_id: Option<&str>,
    ) -> Option<(mpsc::Sender<crate::agent_registry::RegistryMessage>, String)> {
        let sid = session_id?.to_string();
        let sender = self.agent_sender.lock().unwrap().clone()?;
        Some((sender, sid))
    }

    fn attach_pending_output(&self, session_id: &str, output: &PtyOutput) {
        if let Some(channel) = self.pending_outputs.lock().unwrap().remove(session_id) {
            output.attach(channel);
        }
    }

    fn send_lifecycle_command(
        &self,
        command: crate::pty_lifecycle::PtyLifecycleCommand,
    ) -> bool {
        let Some(tx) = self.lifecycle_tx.lock().unwrap().clone() else {
            return false;
        };
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        if tx
            .send(crate::pty_lifecycle::PtyLifecycleMessage::Command(Box::new(command), reply_tx))
            .is_err()
        {
            return false;
        }
        reply_rx.recv().is_ok()
    }

    pub(crate) fn register_session_direct(&self, pty_id: String, session: PtySession) {
        self.sessions.lock().unwrap().insert(pty_id, session);
    }

    pub(crate) fn kill_direct(&self, session_id: &str) {
        let session = self.sessions.lock().unwrap().remove(session_id);
        self.pending_outputs.lock().unwrap().remove(session_id);
        if let Some(mut session) = session {
            if let Err(e) = session.child.kill() {
                rlog!("Warning: kill failed for {}: {}", session_id, e);
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match session.child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Ok(None) => {
                        rlog!("Warning: child for {} did not exit within timeout", session_id);
                        break;
                    }
                    Err(e) => {
                        rlog!("Warning: try_wait failed for {}: {}", session_id, e);
                        break;
                    }
                }
            }
        }
    }

    pub(crate) fn detach_direct(&self, pty_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(pty_id) {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            session.status = PtyStatus::RunningDetached { since_ms: now_ms };
            rlog!("PtyManager: detached PTY '{}'", pty_id);
        }
    }

    pub(crate) fn attach_to_pane_direct(&self, pty_id: &str, pane_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(pty_id) {
            session.status = PtyStatus::RunningAttached { pane_id: pane_id.to_string() };
            session.unread_output = false;
            session.bell_pending = false;
            session.last_activity = std::time::Instant::now();
            rlog!("PtyManager: attached PTY '{}' to pane '{}'", pty_id, pane_id);
        }
    }

    pub(crate) fn mark_exited_if_generation_matches_direct(
        &self,
        pty_id: &str,
        generation: u64,
        code: Option<i32>,
    ) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(pty_id) else {
            return false;
        };
        if session.generation != generation {
            return false;
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let was_attached = matches!(session.status, PtyStatus::RunningAttached { .. });
        session.status = PtyStatus::Exited { code, at_ms: now_ms };
        session.exit_info = Some(ExitInfo { code, at_ms: now_ms, was_attached });
        true
    }

    pub(crate) fn mark_read_direct(&self, pty_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(pty_id) {
            session.unread_output = false;
            session.bell_pending = false;
        }
    }

    pub(crate) fn set_unread_output_direct(&self, pty_id: &str, value: bool) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(pty_id) {
            session.unread_output = value;
        }
    }

    pub(crate) fn set_bell_pending_direct(&self, pty_id: &str, value: bool) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(pty_id) {
            session.bell_pending = value;
        }
    }

    pub(crate) fn set_name_direct(&self, pty_id: &str, name: Option<&str>) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(pty_id) {
            session.name = name.map(|s| s.to_string());
        }
    }

    pub(crate) fn kill_session_ptys_direct(&self, session_id: &str) {
        let ids: Vec<String> = {
            let sessions = self.sessions.lock().unwrap();
            sessions
                .iter()
                .filter(|(_, s)| s.session_id.as_deref() == Some(session_id))
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in ids {
            self.kill_direct(&id);
        }
    }

    pub fn spawn_shell(
        &self,
        id: &str,
        working_dir: &str,
        session_id: Option<&str>,
        pane_id: Option<&str>,
        project_id: Option<&str>,
        worktree_path: Option<&str>,
        notes: Option<&NotesEnvInputs>,
        nono: Option<&NonoConfig>,
        smolvm: Option<&SmolvmExec>,
        initial_size: Option<(u16, u16)>,
        role: PtyRole,
        profile: Option<&str>,
        app: tauri::AppHandle,
    ) -> Result<(), PtyError> {
        // When the session is bound to a smol machine, ensure it's
        // running before opening any PTY. Failing here surfaces as a
        // clean PtyError that the caller can render in the dead-pane
        // view rather than a blank pane that silently never connects.
        if let Some(smol) = smolvm {
            ensure_machine_running(&smol.binary, &smol.machine_name)
                .map_err(|err| PtyError::SpawnShell {
                    source: anyhow::anyhow!(
                        "smol machine '{}' could not be made ready: {}",
                        smol.machine_name,
                        err
                    ),
                })?;
        }

        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(pty_size_from(initial_size))
            .map_err(|source| PtyError::OpenPty { source })?;

        let shell = resolve_default_shell();
        let user_path = get_user_path();
        let nono_label = nono.map(|n| format!(" (nono profile={})", n.profile)).unwrap_or_default();
        let smol_label = smolvm.map(|s| format!(" (smol machine={})", s.machine_name)).unwrap_or_default();
        let pane_label = pane_id.map(|p| format!(", pane '{}'", p)).unwrap_or_default();
        let session_label = session_id.map(|s| format!(", session '{}'", s)).unwrap_or_default();
        rlog!(
            "Spawning shell '{}' for PTY '{}'{}{} in '{}'{}{}",
            shell,
            id,
            pane_label,
            session_label,
            working_dir,
            nono_label,
            smol_label
        );

        // Wrap precedence: smolvm wins over nono. nono is host-side and
        // doesn't exist inside a guest VM unless the image installs it.
        // Treat them as mutually exclusive in v1 and silently skip nono
        // in the smolvm branch (the caller has already logged a warning
        // if both were configured).
        let mut cmd = if let Some(smol) = smolvm {
            let mut c = CommandBuilder::new(smol.binary.as_os_str());
            c.arg("machine");
            c.arg("exec");
            c.arg("--name");
            c.arg(&smol.machine_name);
            c.arg("-i");
            c.arg("-t");
            // Phase 2.9: when the session has a worktree path, ask
            // smolvm to start the guest shell there. The path must be
            // covered by a [dev].volumes mount in the Smolfile;
            // otherwise smolvm exits with "workdir not found". The
            // panel's auto-mount UX (bind-time prompt) prevents that
            // case for new bindings; pre-existing bindings on
            // un-mounted machines will surface the smolvm error in the
            // dead-pane view.
            if let Some(wt) = worktree_path.filter(|p| !p.is_empty()) {
                c.arg("--workdir");
                c.arg(wt);
            }
            // Forward the subset of ROUX_* env that's meaningful in the
            // guest. Host paths (PATH, ROUX_SOCKET, ROUX_CLI, notes paths)
            // are filtered out — see `is_guest_safe_env_key`.
            for (k, v) in roux_env_pairs(
                &user_path,
                session_id,
                pane_id,
                project_id,
                worktree_path,
                notes,
            ) {
                if is_guest_safe_env_key(&k) {
                    c.arg("-e");
                    c.arg(format!("{}={}", k, v));
                }
            }
            c.arg("--");
            c.arg(&smol.guest_shell);
            c
        } else if let Some(nono) = nono {
            let mut c = CommandBuilder::new("nono");
            c.arg("run");
            c.arg("--profile");
            c.arg(&nono.profile);
            c.arg("--allow-cwd");
            for dir in &nono.resolved_allow_dirs(working_dir) {
                c.arg("--allow-dir");
                c.arg(dir);
            }
            c.arg("--");
            c.arg(&shell);
            c
        } else {
            CommandBuilder::new(&shell)
        };
        // apply_shell_command_flags is a no-op outside Windows. On
        // Windows + smolvm doesn't combine (smolvm is Linux/macOS-only),
        // so we keep this unconditional — it's a safe no-op for the
        // smolvm branch on the platforms smolvm runs on.
        apply_shell_command_flags(&mut cmd, &shell);
        // apply_roux_env populates the *outer* CommandBuilder's env.
        // For the smolvm branch this decorates the host-side smolvm CLI
        // process (useful for its own logging) but doesn't reach the
        // guest — the `-e` flags above handle that.
        apply_roux_env(&mut cmd, &user_path, session_id, pane_id, project_id, worktree_path, notes);
        cmd.cwd(working_dir);

        let child = pair.slave.spawn_command(cmd).map_err(|source| {
            rlog!("Failed to spawn shell: {}", source);
            PtyError::SpawnShell { source }
        })?;

        let writer = pair.master.take_writer().map_err(|source| PtyError::GetWriter { source })?;

        let reader =
            pair.master.try_clone_reader().map_err(|source| PtyError::GetReader { source })?;

        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        let logger = Arc::new(Mutex::new(crate::pty_logger::PtyLogger::new(
            session_id.unwrap_or(id),
            id,
        )));
        let output = PtyOutput::new_with_logger(Arc::clone(&logger));

        let writer = Arc::new(Mutex::new(writer));
        let gate =
            Arc::new(Mutex::new(ShellReadyGate::new(Instant::now(), GATE_QUIET, GATE_TIMEOUT)));

        let initial_pane = pane_id.map(|p| p.to_string());
        let initial_status = match initial_pane {
            Some(ref p) => PtyStatus::RunningAttached { pane_id: p.clone() },
            None => {
                let since_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                PtyStatus::RunningDetached { since_ms }
            }
        };
        let size = initial_size.unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
        let session = PtySession {
            master: pair.master,
            child,
            writer: Arc::clone(&writer),
            output: output.clone(),
            generation: gen,
            ready_gate: Some(Arc::clone(&gate)),
            role,
            status: initial_status,
            exit_info: None,
            session_id: session_id.map(|s| s.to_string()),
            name: None,
            working_dir: Some(working_dir.to_string()),
            profile: profile.map(|p| p.to_string()),
            last_size: size,
            last_activity: std::time::Instant::now(),
            unread_output: false,
            bell_pending: false,
            logger: Some(logger),
        };
        let lifecycle_tx = self.lifecycle_tx.lock().unwrap().clone();
        if let Some(tx) = lifecycle_tx {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            match tx.send(crate::pty_lifecycle::PtyLifecycleMessage::Command(
                Box::new(crate::pty_lifecycle::PtyLifecycleCommand::Register {
                    pty_id: id.to_string(),
                    session: Box::new(session),
                }),
                reply_tx,
            )) {
                Ok(()) => {
                    if reply_rx.recv().is_err() {
                        return Err(PtyError::SessionNotFound {
                            session_id: format!("lifecycle register ack dropped for {}", id),
                        });
                    }
                }
                Err(mpsc::SendError(crate::pty_lifecycle::PtyLifecycleMessage::Command(
                    command,
                    _,
                ))) => {
                    match *command {
                        crate::pty_lifecycle::PtyLifecycleCommand::Register { pty_id, session } => {
                            self.register_session_direct(pty_id, *session);
                        }
                        _ => unreachable!("register send only emits register commands here"),
                    }
                }
                Err(_) => unreachable!("register send only emits command messages here"),
            }
        } else {
            self.register_session_direct(id.to_string(), session);
        }
        self.attach_pending_output(id, &output);

        // Use lifecycle bus if available, otherwise fall back to direct event emission
        let tx = if let Some(lifecycle_tx) = self.lifecycle_tx.lock().unwrap().clone() {
            spawn_flusher_with_lifecycle(
                output.clone(),
                id.to_string(),
                session_id.map(|s| s.to_string()),
                gen,
                lifecycle_tx,
                true,
            )
        } else {
            spawn_flusher(
                output.clone(),
                Some((format!("session-exit:{}", id), gen)),
                app.clone(),
                self.exit_registry_info(session_id),
            )
        };
        let sniffer =
            crate::notifications::OscSniffer::new(app.clone(), session_id.map(|s| s.to_string()));
        spawn_reader(
            reader,
            tx,
            Some(sniffer),
            Some((Arc::clone(&gate), Arc::clone(&writer), id.to_string())),
        );
        spawn_gate_ticker(gate, writer, id.to_string());

        Ok(())
    }

    /// Spawn a one-shot command in a PTY. The PTY exits when the command finishes,
    /// and the real exit code is emitted via session-exit:{id}.
    pub fn spawn_task(
        &self,
        id: &str,
        command: &str,
        working_dir: &str,
        session_id: Option<&str>,
        pane_id: Option<&str>,
        project_id: Option<&str>,
        worktree_path: Option<&str>,
        notes: Option<&NotesEnvInputs>,
        smolvm: Option<&SmolvmExec>,
        initial_size: Option<(u16, u16)>,
        role: PtyRole,
        profile: Option<&str>,
        app: tauri::AppHandle,
    ) -> Result<(), PtyError> {
        // Same readiness gate as `spawn_shell`: when the session is
        // bound to a smol machine, ensure it's running before opening
        // the PTY so a stopped VM surfaces as a clean error rather
        // than a blank pane.
        if let Some(smol) = smolvm {
            ensure_machine_running(&smol.binary, &smol.machine_name).map_err(|err| {
                PtyError::SpawnTask {
                    source: anyhow::anyhow!(
                        "smol machine '{}' could not be made ready: {}",
                        smol.machine_name,
                        err
                    ),
                }
            })?;
        }

        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(pty_size_from(initial_size))
            .map_err(|source| PtyError::OpenPty { source })?;

        let shell = resolve_default_shell();
        let user_path = get_user_path();

        // When the session is bound to a smol machine, run the command
        // inside the guest via `smolvm machine exec --name <m> -- /bin/sh
        // -c <cmd>`. Mirrors the spawn_shell smolvm wrap (env subset
        // forwarded via `-e KEY=VAL`, optional `--workdir` from the
        // session's worktree) so `roux run` behaves identically to a
        // shell pane on a bound session.
        let mut cmd = if let Some(smol) = smolvm {
            let mut c = CommandBuilder::new(smol.binary.as_os_str());
            c.arg("machine");
            c.arg("exec");
            c.arg("--name");
            c.arg(&smol.machine_name);
            c.arg("-i");
            c.arg("-t");
            if let Some(wt) = worktree_path.filter(|p| !p.is_empty()) {
                c.arg("--workdir");
                c.arg(wt);
            }
            for (k, v) in roux_env_pairs(
                &user_path,
                session_id,
                pane_id,
                project_id,
                worktree_path,
                notes,
            ) {
                if is_guest_safe_env_key(&k) {
                    c.arg("-e");
                    c.arg(format!("{}={}", k, v));
                }
            }
            c.arg("--");
            c.arg(&smol.guest_shell);
            c.arg("-c");
            c.arg(command);
            c
        } else {
            let mut c = CommandBuilder::new(&shell);
            apply_task_command_args(&mut c, &shell, command);
            c
        };
        apply_roux_env(&mut cmd, &user_path, session_id, pane_id, project_id, worktree_path, notes);
        cmd.cwd(working_dir);

        let mut child =
            pair.slave.spawn_command(cmd).map_err(|source| PtyError::SpawnTask { source })?;

        let writer = pair.master.take_writer().map_err(|source| PtyError::GetWriter { source })?;

        let reader =
            pair.master.try_clone_reader().map_err(|source| PtyError::GetReader { source })?;

        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        let logger_task = Arc::new(Mutex::new(crate::pty_logger::PtyLogger::new(
            session_id.unwrap_or(id),
            id,
        )));
        let output = PtyOutput::new_with_logger(Arc::clone(&logger_task));

        // Insert session before attaching pending output and starting threads
        let initial_pane_task = pane_id.map(|p| p.to_string());
        let initial_status_task = match initial_pane_task {
            Some(ref p) => PtyStatus::RunningAttached { pane_id: p.clone() },
            None => {
                let since_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                PtyStatus::RunningDetached { since_ms }
            }
        };
        let size_task = initial_size.unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
        let session = PtySession {
            master: pair.master,
            child: Box::new(WaitedChild),
            writer: Arc::new(Mutex::new(writer)),
            output: output.clone(),
            generation: gen,
            // One-shot tasks run the command as argv to the shell
            // (non-interactive), so there is no ZLE/readline init to race.
            ready_gate: None,
            role,
            status: initial_status_task,
            exit_info: None,
            session_id: session_id.map(|s| s.to_string()),
            name: None,
            working_dir: Some(working_dir.to_string()),
            profile: profile.map(|p| p.to_string()),
            last_size: size_task,
            last_activity: std::time::Instant::now(),
            unread_output: false,
            bell_pending: false,
            logger: Some(logger_task),
        };
        let lifecycle_tx = self.lifecycle_tx.lock().unwrap().clone();
        if let Some(tx) = lifecycle_tx {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            match tx.send(crate::pty_lifecycle::PtyLifecycleMessage::Command(
                Box::new(crate::pty_lifecycle::PtyLifecycleCommand::Register {
                    pty_id: id.to_string(),
                    session: Box::new(session),
                }),
                reply_tx,
            )) {
                Ok(()) => {
                    if reply_rx.recv().is_err() {
                        return Err(PtyError::SessionNotFound {
                            session_id: format!("lifecycle register ack dropped for {}", id),
                        });
                    }
                }
                Err(mpsc::SendError(crate::pty_lifecycle::PtyLifecycleMessage::Command(
                    command,
                    _,
                ))) => {
                    match *command {
                        crate::pty_lifecycle::PtyLifecycleCommand::Register { pty_id, session } => {
                            self.register_session_direct(pty_id, *session);
                        }
                        _ => unreachable!("register send only emits register commands here"),
                    }
                }
                Err(_) => unreachable!("register send only emits command messages here"),
            }
        } else {
            self.register_session_direct(id.to_string(), session);
        }
        self.attach_pending_output(id, &output);

        // One-shot tasks don't carry hooks but we still wire the
        // registry sender so any stray attention notification keyed by
        // the task's session id gets cleared when it finishes.
        // Note: Tasks use the flusher primarily for output buffering, not exit events.
        // The actual exit event comes from the child.wait() thread below.
        let lifecycle_tx_clone = self.lifecycle_tx.lock().unwrap().clone();
        let tx = spawn_flusher(
            output.clone(),
            None,
            app.clone(),
            self.exit_registry_info(session_id),
        );
        let sniffer =
            crate::notifications::OscSniffer::new(app.clone(), session_id.map(|s| s.to_string()));
        spawn_reader(reader, tx, Some(sniffer), None);

        // Wait for the child process in a background thread and emit exit code
        let exit_pty_id = id.to_string();
        let exit_session_id = session_id.map(|s| s.to_string());
        thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            if let Some(lifecycle_tx) = lifecycle_tx_clone {
                let _ = lifecycle_tx.send(crate::pty_lifecycle::PtyLifecycleMessage::Event(
                    crate::pty_lifecycle::PtyLifecycleEvent::Exited {
                        pty_id: exit_pty_id,
                        session_id: exit_session_id,
                        code,
                        reason: crate::pty_lifecycle::ExitReason::Exit,
                        generation: gen,
                    },
                ));
            } else {
                let exit_event_name = format!("session-exit:{}", exit_pty_id);
                let _ = app.emit(
                    &exit_event_name,
                    &roux_core::SessionExitPayload {
                        code,
                        generation: gen,
                        reason: roux_core::SessionExitReason::Exit,
                    },
                );
            }
        });

        Ok(())
    }

    pub fn attach_output_channel(&self, session_id: &str, channel: Channel<Response>) {
        self.cleanup_stale_pending();
        let output =
            self.sessions.lock().unwrap().get(session_id).map(|session| session.output.clone());
        if let Some(output) = output {
            output.attach(channel);
        } else {
            self.pending_outputs.lock().unwrap().insert(session_id.to_string(), channel);
        }
    }

    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        let (writer, gate) = {
            let sessions = self.sessions.lock().unwrap();
            let session = sessions
                .get(session_id)
                .ok_or_else(|| PtyError::SessionNotFound { session_id: session_id.to_string() })?;
            (Arc::clone(&session.writer), session.ready_gate.clone())
        };

        // For shell sessions, run the write through the readiness gate
        // first. While the shell's prompt isn't up yet, the gate buffers
        // the bytes and returns an empty slice; they get flushed later
        // by the reader thread (on prompt detection) or the tick thread
        // (on quiescence / timeout). Once open, it returns bytes through.
        let bytes_to_write: Vec<u8> = match gate {
            Some(g) => {
                let mut guard = g.lock().unwrap();
                guard.on_write(data, Instant::now())
            }
            None => data.to_vec(),
        };

        if bytes_to_write.is_empty() {
            return Ok(());
        }

        let mut writer = writer.lock().unwrap();
        use std::io::Write;
        writer.write_all(&bytes_to_write).map_err(|source| PtyError::WriteFailed { source })?;
        writer.flush().map_err(|source| PtyError::FlushFailed { source })
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        let master = {
            let sessions = self.sessions.lock().unwrap();
            let session = sessions
                .get(session_id)
                .ok_or_else(|| PtyError::SessionNotFound { session_id: session_id.to_string() })?;
            // MasterPty doesn't impl Clone, so we need to keep the lock for resize.
            // However, we can at least use try_lock to avoid blocking other sessions.
            session
                .master
                .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                .map_err(|source| PtyError::ResizeFailed { source })
        };
        master
    }

    fn cleanup_stale_pending(&self) {
        // pending_outputs only accumulate for IDs that never spawned.
        // Since we can't timestamp them easily, just check if the session exists.
        let sessions = self.sessions.lock().unwrap();
        let mut pending = self.pending_outputs.lock().unwrap();
        pending.retain(|id, _| sessions.contains_key(id));
    }

    pub fn kill(&self, session_id: &str) {
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::Kill {
            pty_id: session_id.to_string(),
        }) {
            self.kill_direct(session_id);
        }
    }

    pub fn get_generation(&self, session_id: &str) -> Option<u64> {
        self.sessions.lock().unwrap().get(session_id).map(|s| s.generation)
    }

    pub(crate) fn get_info_direct(&self, pty_id: &str) -> Option<PtyInfo> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(pty_id).map(|s| PtyInfo {
            id: pty_id.to_string(),
            session_id: s.session_id.clone(),
            role: s.role.clone(),
            status: s.status.clone(),
            name: s.name.clone(),
            working_dir: s.working_dir.clone(),
            profile: s.profile.clone(),
            unread_output: s.unread_output,
            bell_pending: s.bell_pending,
        })
    }

    /// List PTY info snapshots for a given session (for picker UI).
    pub fn list_for_session(&self, session_id: &str) -> Vec<PtyInfo> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .iter()
            .filter(|(_, s)| s.session_id.as_deref() == Some(session_id))
            .map(|(id, s)| PtyInfo {
                id: id.clone(),
                session_id: s.session_id.clone(),
                role: s.role.clone(),
                status: s.status.clone(),
                name: s.name.clone(),
                working_dir: s.working_dir.clone(),
                profile: s.profile.clone(),
                unread_output: s.unread_output,
                bell_pending: s.bell_pending,
            })
            .collect()
    }

    /// List all PTY info snapshots in one pass.
    pub fn list_all(&self) -> Vec<PtyInfo> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .iter()
            .map(|(id, s)| PtyInfo {
                id: id.clone(),
                session_id: s.session_id.clone(),
                role: s.role.clone(),
                status: s.status.clone(),
                name: s.name.clone(),
                working_dir: s.working_dir.clone(),
                profile: s.profile.clone(),
                unread_output: s.unread_output,
                bell_pending: s.bell_pending,
            })
            .collect()
    }

    /// Detach a PTY from its pane (PTY keeps running).
    pub fn detach(&self, pty_id: &str) {
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::Detach {
            pty_id: pty_id.to_string(),
        }) {
            self.detach_direct(pty_id);
        }
    }

    /// Mark a PTY as attached to a pane.
    pub fn attach_to_pane(&self, pty_id: &str, pane_id: &str) {
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::AttachToPane {
            pty_id: pty_id.to_string(),
            pane_id: pane_id.to_string(),
        }) {
            self.attach_to_pane_direct(pty_id, pane_id);
        }
    }

    /// Clear unread output and bell flags for a PTY.
    pub fn mark_read(&self, pty_id: &str) {
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::MarkRead {
            pty_id: pty_id.to_string(),
        }) {
            self.mark_read_direct(pty_id);
        }
    }

    /// Set the unread output flag for a PTY.
    pub fn set_unread_output(&self, pty_id: &str, value: bool) {
        self.set_unread_output_direct(pty_id, value);
    }

    /// Set the bell pending flag for a PTY.
    pub fn set_bell_pending(&self, pty_id: &str, value: bool) {
        self.set_bell_pending_direct(pty_id, value);
    }

    /// Set the display name for a PTY.
    pub fn set_name(&self, pty_id: &str, name: Option<&str>) {
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::SetName {
            pty_id: pty_id.to_string(),
            name: name.map(|s| s.to_string()),
        }) {
            self.set_name_direct(pty_id, name);
        }
    }

    /// Kill all PTY sessions for a session ID.
    pub fn kill_session_ptys(&self, session_id: &str) {
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::KillSessionPtys {
            session_id: session_id.to_string(),
        }) {
            self.kill_session_ptys_direct(session_id);
        }
    }

    /// Resolve the current working directory of the shell (or claude) process
    /// attached to `pty_id`, by walking `portable_pty::Child::process_id` →
    /// OS-level cwd lookup. Returns `None` if the session doesn't exist, the
    /// child has exited, or the OS refuses.
    pub fn get_cwd(&self, pty_id: &str) -> Option<String> {
        let pid = self.sessions.lock().unwrap().get(pty_id).and_then(|s| s.child.process_id())?;
        cwd_for_pid(pid)
    }

    /// Get recent output bytes for replay on attach.
    pub fn get_replay(&self, pty_id: &str, max_bytes: usize) -> Vec<u8> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(pty_id) {
            if let Some(ref logger) = session.logger {
                return logger.lock().unwrap().recent(max_bytes);
            }
        }
        Vec::new()
    }

    /// Kill all active PTY sessions. Called during app shutdown.
    pub fn shutdown_all(&self) {
        let ids: Vec<String> = self.sessions.lock().unwrap().keys().cloned().collect();
        for id in &ids {
            self.kill(id);
        }
        rlog!("PtyManager: shut down {} session(s)", ids.len());
    }
}

/// Get the socket path as a string for setting env vars.
fn socket_path_str() -> String {
    platform::resolve_socket_endpoint()
        .unwrap_or_else(|| platform::socket_path().to_string_lossy().to_string())
}

/// Eager trigger for the roux-cli shim. Called from `main.rs` setup so the
/// symlink dir is ready before any PTY spawns and so we log the result at
/// startup for debugging. Safe to call repeatedly — cached behind a OnceLock.
pub fn ensure_roux_cli_shim() {
    let _ = roux_cli_shim();
}

/// Cached pair of (bin-dir-to-prepend-to-PATH, full-path-to-roux-cli).
/// Set up once at first PTY spawn: creates `~/.config/roux/bin/` and places
/// `roux-cli` + `roux` symlinks there, both pointing at the roux-cli binary
/// built next to the currently running `roux` exe. Returning `None` means
/// we couldn't find the bundled roux-cli (e.g. a dev build where it wasn't
/// compiled yet) — callers skip the PATH injection gracefully.
fn roux_cli_shim() -> Option<(String, String)> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<(String, String)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            // 1. Find the bundled roux-cli next to the currently running exe.
            let source = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(platform::roux_cli_file_name())))?;
            if !source.exists() {
                rlog!("roux_cli_shim: bundled roux-cli not found at {}", source.display());
                return None;
            }

            // 2. Ensure ~/.config/roux/bin/ exists.
            let bin_dir = crate::paths::roux_config_dir().join("bin");
            if let Err(e) = std::fs::create_dir_all(&bin_dir) {
                rlog!("roux_cli_shim: failed to create {}: {}", bin_dir.display(), e);
                return None;
            }

            // 3. Install symlinks: `roux-cli` and short alias `roux` both
            //    pointing at the bundled source. We re-create the links every
            //    startup so the PTY always sees the freshest binary, even
            //    after a version bump.
            #[cfg(unix)]
            {
                use std::os::unix::fs as unix_fs;
                for alias in ["roux-cli", "roux"] {
                    let link = bin_dir.join(alias);
                    // Remove any existing symlink/file so we can re-point it.
                    let _ = std::fs::remove_file(&link);
                    if let Err(e) = unix_fs::symlink(&source, &link) {
                        rlog!(
                            "roux_cli_shim: failed to symlink {} -> {}: {}",
                            link.display(),
                            source.display(),
                            e
                        );
                        return None;
                    }
                }
            }

            #[cfg(windows)]
            {
                for alias in ["roux-cli.exe", "roux.exe"] {
                    let target = bin_dir.join(alias);
                    let should_copy = if target.exists() {
                        let src_modified =
                            std::fs::metadata(&source).and_then(|m| m.modified()).ok();
                        let dst_modified =
                            std::fs::metadata(&target).and_then(|m| m.modified()).ok();
                        match (src_modified, dst_modified) {
                            (Some(src), Some(dst)) => src > dst,
                            _ => true,
                        }
                    } else {
                        true
                    };
                    if should_copy && std::fs::copy(&source, &target).is_err() {
                        rlog!(
                            "roux_cli_shim: failed to copy {} -> {}",
                            source.display(),
                            target.display()
                        );
                        return None;
                    }
                }
            }

            let bin_dir_str = bin_dir.to_string_lossy().to_string();
            let source_str = source.to_string_lossy().to_string();
            rlog!("roux_cli_shim: installed {} (source: {})", bin_dir_str, source_str);
            Some((bin_dir_str, source_str))
        })
        .clone()
}

/// Build the PATH value to hand to a PTY child. Prepends the roux-cli shim
/// directory (if available) to the user's login-shell PATH so scripts
/// running inside any Roux pane can invoke `roux notify`, `roux-cli focus`,
/// etc. without a separate install step.
fn build_pty_path(user_path: &str) -> String {
    let Some((bin_dir, _)) = roux_cli_shim() else {
        return user_path.to_string();
    };

    let mut paths: Vec<_> = std::env::split_paths(user_path).collect();
    let bin_dir_path = std::path::PathBuf::from(&bin_dir);
    if paths.iter().any(|path| path == &bin_dir_path) {
        return user_path.to_string();
    }

    paths.insert(0, bin_dir_path);
    std::env::join_paths(paths)
        .ok()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| user_path.to_string())
}

/// Apply the common Roux env vars to a `CommandBuilder`: PATH with shim dir
/// prepended, `ROUX_CLI` pointing at the absolute roux-cli path, and the
/// standard `ROUX_*` markers. Called by every `spawn_*` method so the three
/// paths stay in sync.
///
/// `session_id` and `pane_id` are threaded through so every shell/task/agent
/// PTY hosts `ROUX_SESSION_ID` and `ROUX_PANE_ID` in its env unconditionally.
/// Hooks and `roux notify` read them to route events back to the correct
/// pane without cwd heuristics.
/// Pre-computed inputs for the `ROUX_*_NOTES_*` env vars. Built by the
/// session-creation layer (which has access to `NotesService` + settings)
/// and threaded through the PTY spawn calls. Every string has already been
/// resolved through `NotesService::resolve_target` so the slugs here are
/// the same ones the Tauri commands and the frontend panel will see.
#[derive(Debug, Clone, Default)]
pub(crate) struct NotesEnvInputs {
    pub(crate) vault_root: String,
    pub(crate) session_slug: String,
    pub(crate) repo_slug: String,
    pub(crate) project_slug: Option<String>,
    /// External docs/specs paths attached to this session's project.
    /// Surfaced to the PTY child as `ROUX_PROJECT_CONTEXT_PATHS` (colon-
    /// separated like `PATH`), and only when the session is associated
    /// with a project that has at least one path configured.
    pub(crate) context_paths: Vec<String>,
    /// Free-form text exposed as `ROUX_PROJECT_PROMPT`. The frontend
    /// profile runner additionally splices this into the agent CLI's
    /// startup command (`--append-system-prompt` for Claude,
    /// `-c instructions=…` for Codex). Empty string = unset.
    pub(crate) project_prompt: String,
}

fn apply_roux_env(
    cmd: &mut CommandBuilder,
    user_path: &str,
    session_id: Option<&str>,
    pane_id: Option<&str>,
    project_id: Option<&str>,
    worktree_path: Option<&str>,
    notes: Option<&NotesEnvInputs>,
) {
    for (k, v) in
        roux_env_pairs(user_path, session_id, pane_id, project_id, worktree_path, notes)
    {
        cmd.env(k, v);
    }
}

/// Pure pair-emitting variant of [`apply_roux_env`]. Used by both the
/// host-side `CommandBuilder` path and the smolvm-wrap branch (which
/// folds a filtered subset into `-e KEY=VAL` flags).
fn roux_env_pairs(
    user_path: &str,
    session_id: Option<&str>,
    pane_id: Option<&str>,
    project_id: Option<&str>,
    worktree_path: Option<&str>,
    notes: Option<&NotesEnvInputs>,
) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = vec![
        ("PATH".to_string(), build_pty_path(user_path)),
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("ROUX_SESSION".to_string(), "1".to_string()),
        ("ROUX_SOCKET".to_string(), socket_path_str()),
    ];
    if let Some((_, cli_path)) = roux_cli_shim() {
        pairs.push(("ROUX_CLI".to_string(), cli_path));
    }
    if let Some(sid) = session_id {
        pairs.push(("ROUX_SESSION_ID".to_string(), sid.to_string()));
    }
    if let Some(pid) = pane_id {
        pairs.push(("ROUX_PANE_ID".to_string(), pid.to_string()));
        // Snapshot any alias bound to this pane at spawn time. The lookup
        // hits the persisted `aliases.json` directly so we don't have to
        // plumb `AliasManager` through the PtyManager. If the alias is
        // auto-claimed AFTER spawn (pane rename / auto-claim from name),
        // the env stays stale until the next respawn — agents should
        // prefer `roux alias whoami` for live state.
        if let Some(alias) = lookup_pane_alias(pid) {
            pairs.push(("ROUX_AGENT_ALIAS".to_string(), alias));
        }
    }
    if let Some(pid) = project_id {
        pairs.push(("ROUX_PROJECT_ID".to_string(), pid.to_string()));
    }
    if let Some(wt) = worktree_path {
        pairs.push(("ROUX_WORKTREE_PATH".to_string(), wt.to_string()));
    }
    if let Some(n) = notes {
        notes_env_pairs(n, &mut pairs);
    }
    pairs
}

/// True for env keys that are meaningful inside a smolvm guest. Host
/// paths (PATH, ROUX_SOCKET, ROUX_CLI, ROUX_NOTES_*) are excluded — the
/// guest has its own filesystem and they'd point at non-existent
/// locations. Forwarding them would mislead shell-rc scripts that test
/// for them.
fn is_guest_safe_env_key(key: &str) -> bool {
    matches!(
        key,
        "TERM"
            | "COLORTERM"
            | "ROUX_SESSION"
            | "ROUX_SESSION_ID"
            | "ROUX_PANE_ID"
            | "ROUX_PROJECT_ID"
            | "ROUX_AGENT_ALIAS"
    )
}

/// Best-effort lookup: which alias is bound to `pane_id` right now?
/// Reads `aliases.json` directly via the lib crate. Returns `None` for
/// unknown panes or when the file is missing/malformed (the env var is
/// just a hint — agents have `roux alias whoami` for authoritative state).
fn lookup_pane_alias(pane_id: &str) -> Option<String> {
    roux_lib::aliases::load_aliases()
        .into_iter()
        .find(|a| a.pane_id.as_deref() == Some(pane_id))
        .map(|a| a.alias)
}

/// Make sure a smol machine is running before we exec into it.
///
/// Lists machines, finds the named entry, and runs `smolvm machine
/// start --name <n>` when its state isn't already running/starting.
/// `start` is idempotent — running it on a live machine is a no-op —
/// so the only failure cases are "machine doesn't exist" or the
/// underlying CLI itself failing. Both surface as a typed error so the
/// caller can render a clean dead-pane message instead of opening a
/// blank PTY that quietly disconnects.
fn ensure_machine_running(binary: &std::path::Path, name: &str) -> Result<(), String> {
    let machines = roux_smolvm::list_machines(binary).map_err(|e| e.to_string())?;
    let m = machines
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("smol machine '{name}' not found (was it deleted?)"))?;
    let state = m.state.to_lowercase();
    if state.contains("running") || state.contains("starting") {
        return Ok(());
    }
    rlog!("smol machine '{}' is '{}', auto-starting before exec", name, m.state);
    roux_smolvm::start_machine(binary, name).map_err(|e| e.to_string())
}

/// Collect notes-related env pairs for forwarding into a child process.
/// Used by `roux_env_pairs` (host-side `cmd.env`) and the smolvm wrap
/// (`-e KEY=VAL` flags). Project-context vars are deliberately omitted
/// when their inputs are empty so shell idioms like
/// `${ROUX_SESSION_PROJECT:-no-project}` keep working.
fn notes_env_pairs(n: &NotesEnvInputs, pairs: &mut Vec<(String, String)>) {
    use std::path::Path;
    let root = Path::new(&n.vault_root);
    let global_dir = root.join("global");
    let repo_dir = root.join("repos").join(&n.repo_slug);
    let session_dir = root.join("sessions").join(&n.session_slug);

    pairs.push(("ROUX_NOTES_ROOT".to_string(), root.to_string_lossy().to_string()));
    pairs.push((
        "ROUX_GLOBAL_NOTES_DIR".to_string(),
        global_dir.to_string_lossy().to_string(),
    ));
    pairs.push((
        "ROUX_GLOBAL_NOTES_FILE".to_string(),
        global_dir.join("notes.md").to_string_lossy().to_string(),
    ));
    pairs.push(("ROUX_REPO_SLUG".to_string(), n.repo_slug.clone()));
    pairs.push(("ROUX_REPO_NOTES_DIR".to_string(), repo_dir.to_string_lossy().to_string()));
    pairs.push((
        "ROUX_REPO_NOTES_FILE".to_string(),
        repo_dir.join("notes.md").to_string_lossy().to_string(),
    ));
    pairs.push(("ROUX_SESSION_DIR".to_string(), session_dir.to_string_lossy().to_string()));
    pairs.push((
        "ROUX_SESSION_NOTES_FILE".to_string(),
        session_dir.join("notes.md").to_string_lossy().to_string(),
    ));
    if let Some(project_slug) = n.project_slug.as_deref() {
        let project_dir = root.join("projects").join(project_slug);
        pairs.push(("ROUX_SESSION_PROJECT".to_string(), project_slug.to_string()));
        pairs.push((
            "ROUX_SESSION_PROJECT_NOTES_DIR".to_string(),
            project_dir.to_string_lossy().to_string(),
        ));
        pairs.push((
            "ROUX_SESSION_PROJECT_NOTES_FILE".to_string(),
            project_dir.join("notes.md").to_string_lossy().to_string(),
        ));
    }

    if !n.context_paths.is_empty() {
        match std::env::join_paths(n.context_paths.iter().map(std::path::Path::new)) {
            Ok(joined) => {
                pairs.push((
                    "ROUX_PROJECT_CONTEXT_PATHS".to_string(),
                    joined.to_string_lossy().to_string(),
                ));
            }
            Err(e) => {
                rlog!(
                    "notes_env_pairs: failed to encode ROUX_PROJECT_CONTEXT_PATHS ({} paths): {}",
                    n.context_paths.len(),
                    e
                );
            }
        }
    }
    if !n.project_prompt.is_empty() {
        pairs.push(("ROUX_PROJECT_PROMPT".to_string(), n.project_prompt.clone()));
    }
}

/// Get the user's login shell PATH by invoking the same shell Roux would use
/// for terminal panes. This keeps Homebrew and other shell-managed prefixes
/// visible to GUI launches.
pub fn get_user_path() -> String {
    get_user_path_impl()
}

#[cfg(windows)]
fn get_user_path_impl() -> String {
    std::env::var("PATH").unwrap_or_default()
}

#[cfg(not(windows))]
fn get_user_path_impl() -> String {
    let shell = resolve_default_shell();
    // Fish outputs $PATH as a space-separated list; other shells use colons.
    // Use fish's `string join` to get colon-separated output.
    let path_cmd = if shell.contains("fish") { "string join : $PATH" } else { "echo $PATH" };
    rlog!("Resolving PATH via login shell: {} -l -c '{}'", shell, path_cmd);
    let result = std::process::Command::new(&shell).args(["-l", "-c", path_cmd]).output();
    match &result {
        Ok(o) => {
            if !o.status.success() {
                rlog!("  shell exited with status: {}", o.status);
                let stderr = String::from_utf8_lossy(&o.stderr);
                if !stderr.is_empty() {
                    rlog!("  stderr: {}", stderr.chars().take(500).collect::<String>());
                }
            }
        }
        Err(e) => rlog!("  failed to run shell: {}", e),
    }
    let path = result
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    rlog!("  resolved PATH: {}", path);
    path
}

fn resolve_default_shell() -> String {
    let shell_binary_path = shell_binary_path_override();

    #[cfg(windows)]
    {
        if let Some(shell) = shell_binary_path {
            return shell;
        }
        if platform::find_executable_on_path("pwsh").is_some()
            || platform::find_executable_on_path("pwsh.exe").is_some()
        {
            return "pwsh".to_string();
        }
        if platform::find_executable_on_path("powershell.exe").is_some() {
            return "powershell.exe".to_string();
        }
        return std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
    }

    #[cfg(not(windows))]
    {
        resolve_default_shell_from_sources(
            shell_binary_path.as_deref(),
            login_shell_for_current_user().as_deref(),
            std::env::var("SHELL").ok().as_deref(),
        )
    }
}

pub(crate) fn set_shell_binary_path_override(path: Option<String>) {
    let cache = shell_binary_path_cache();
    *cache.lock().unwrap() = Some(path.as_deref().and_then(nonempty_trimmed).map(str::to_string));
}

fn shell_binary_path_override() -> Option<String> {
    let cache = shell_binary_path_cache();
    let mut guard = cache.lock().unwrap();
    if guard.is_none() {
        *guard = Some(
            crate::settings::load_settings()
                .shell_binary_path
                .as_deref()
                .and_then(nonempty_trimmed)
                .map(str::to_string),
        );
    }
    guard.clone().flatten()
}

fn shell_binary_path_cache() -> &'static Mutex<Option<Option<String>>> {
    static CACHE: std::sync::OnceLock<Mutex<Option<Option<String>>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

#[cfg(not(windows))]
fn resolve_default_shell_from_sources(
    setting_shell: Option<&str>,
    login_shell: Option<&str>,
    env_shell: Option<&str>,
) -> String {
    setting_shell
        .and_then(nonempty_trimmed)
        .or_else(|| login_shell.and_then(nonempty_trimmed))
        .or_else(|| env_shell.and_then(nonempty_trimmed))
        .unwrap_or("/bin/zsh")
        .to_string()
}

fn nonempty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(unix)]
fn login_shell_for_current_user() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    if buf_size < 1024 {
        buf_size = 16 * 1024;
    }
    let mut buf = vec![0 as libc::c_char; buf_size as usize];

    loop {
        let mut passwd = std::mem::MaybeUninit::<libc::passwd>::zeroed();
        let rc = unsafe {
            libc::getpwuid_r(uid, passwd.as_mut_ptr(), buf.as_mut_ptr(), buf.len(), &mut result)
        };
        if rc == libc::ERANGE {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if rc != 0 || result.is_null() {
            return None;
        }

        let passwd = unsafe { passwd.assume_init() };
        if passwd.pw_shell.is_null() {
            return None;
        }
        let shell = unsafe { std::ffi::CStr::from_ptr(passwd.pw_shell) };
        return shell.to_str().ok().and_then(nonempty_trimmed).map(str::to_string);
    }
}

fn apply_shell_command_flags(cmd: &mut CommandBuilder, shell: &str) {
    #[cfg(windows)]
    {
        let shell_lower = shell.to_ascii_lowercase();
        if shell_lower.contains("pwsh") || shell_lower.contains("powershell") {
            cmd.arg("-NoLogo");
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (cmd, shell);
    }
}

fn apply_task_command_args(cmd: &mut CommandBuilder, shell: &str, command: &str) {
    #[cfg(windows)]
    {
        let shell_lower = shell.to_ascii_lowercase();
        if shell_lower.contains("pwsh") {
            cmd.args(["-NoLogo", "-NoProfile", "-Command", command]);
            return;
        }
        if shell_lower.contains("powershell") {
            cmd.args(["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command]);
            return;
        }
        cmd.args(["/C", command]);
        return;
    }

    #[cfg(not(windows))]
    {
        cmd.args(["-c", command]);
        let _ = shell;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::{Arc, Mutex};
    use tauri::ipc::{Channel, InvokeResponseBody, Response};

    pub(super) fn raw_channel(store: Arc<Mutex<Vec<Vec<u8>>>>) -> Channel<Response> {
        Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                store.lock().unwrap().push(bytes);
                Ok(())
            } else {
                panic!("expected raw bytes");
            }
        })
    }

    #[test]
    fn logger_receives_bytes_sent_to_output() {
        let logger = Arc::new(Mutex::new(crate::pty_logger::PtyLogger::new("test-sess", "test-pty")));
        let output = PtyOutput::new_with_logger(Arc::clone(&logger));

        output.send(vec![1, 2, 3]);
        output.send(vec![4, 5, 6]);

        let recent = logger.lock().unwrap().recent(1024);
        assert_eq!(recent, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn buffers_output_until_channel_attaches() {
        let output = PtyOutput::new();
        output.send(vec![1, 2, 3]);
        output.send(vec![4, 5, 6]);

        let received = Arc::new(Mutex::new(Vec::new()));
        output.attach(raw_channel(received.clone()));

        assert_eq!(*received.lock().unwrap(), vec![vec![1, 2, 3], vec![4, 5, 6]]);
    }

    #[test]
    fn trims_oldest_backlog_when_limit_is_exceeded() {
        let output = PtyOutput::new();
        output.send(vec![1; PTY_BACKLOG_LIMIT_BYTES / 2]);
        output.send(vec![2; PTY_BACKLOG_LIMIT_BYTES / 2]);
        output.send(vec![3; PTY_BACKLOG_LIMIT_BYTES / 2]);

        let received = Arc::new(Mutex::new(Vec::new()));
        output.attach(raw_channel(received.clone()));

        assert_eq!(
            *received.lock().unwrap(),
            vec![vec![2; PTY_BACKLOG_LIMIT_BYTES / 2], vec![3; PTY_BACKLOG_LIMIT_BYTES / 2]]
        );
    }

    #[test]
    fn sends_directly_once_channel_is_attached() {
        let output = PtyOutput::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        output.attach(raw_channel(received.clone()));

        output.send(vec![9, 8, 7]);

        assert_eq!(*received.lock().unwrap(), vec![vec![9, 8, 7]]);
    }

    #[test]
    fn get_user_path_returns_nonempty_string() {
        let path = get_user_path();
        assert!(!path.is_empty(), "PATH should not be empty");
        // Should contain at least /usr/bin which is always on PATH
        assert!(
            path.contains("/usr/bin") || path.contains("/bin"),
            "PATH should contain standard bin directories, got: {}",
            path
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_prefers_explicit_setting_over_login_shell_and_env() {
        let shell = resolve_default_shell_from_sources(
            Some(" /custom/fish "),
            Some("/bin/zsh"),
            Some("/bin/bash"),
        );

        assert_eq!(shell, "/custom/fish");
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_prefers_login_shell_over_env_shell() {
        let shell = resolve_default_shell_from_sources(
            None,
            Some("/opt/homebrew/bin/fish"),
            Some("/bin/zsh"),
        );

        assert_eq!(shell, "/opt/homebrew/bin/fish");
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_uses_env_shell_when_login_shell_is_unavailable() {
        let shell = resolve_default_shell_from_sources(None, None, Some("/bin/bash"));

        assert_eq!(shell, "/bin/bash");
    }

    #[test]
    fn pty_error_display_keeps_existing_messages() {
        let error = PtyError::SessionNotFound { session_id: "session-123".to_string() };
        assert_eq!(error.to_string(), "Session session-123 not found");

        let io_error = PtyError::WriteFailed { source: io::Error::other("broken pipe") };
        assert_eq!(io_error.to_string(), "Write failed: broken pipe");
    }

    #[test]
    fn cwd_for_pid_returns_current_process_cwd() {
        let pid = std::process::id();
        let cwd = cwd_for_pid(pid).expect("cwd_for_pid should resolve for self");
        let expected = std::env::current_dir().expect("current_dir");
        assert_eq!(
            std::fs::canonicalize(&cwd).unwrap(),
            std::fs::canonicalize(&expected).unwrap(),
            "cwd_for_pid(self) should match std::env::current_dir()"
        );
    }

    #[test]
    fn cwd_for_pid_returns_none_for_nonexistent_pid() {
        // PID 0 is never a real process on macOS or Linux.
        assert!(cwd_for_pid(0).is_none());
    }
}

#[cfg(test)]
mod flusher_lifecycle_tests {
    use super::*;
    use crate::pty_lifecycle::{ExitReason, PtyLifecycleEvent, PtyLifecycleMessage};

    #[test]
    fn flusher_sends_exited_event_on_eof() {
        let output = PtyOutput::new();
        let (lifecycle_tx, lifecycle_rx) = crate::pty_lifecycle::channel();

        let tx = spawn_flusher_with_lifecycle(
            output,
            "pty-123".to_string(),
            Some("session-456".to_string()),
            42,
            lifecycle_tx,
            true,
        );

        // Send EOF to trigger exit
        tx.send(PtyChunk::Eof).unwrap();

        // Verify lifecycle event was sent
        let event = lifecycle_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(
                event,
                PtyLifecycleMessage::Event(PtyLifecycleEvent::Exited {
                    pty_id,
                    session_id,
                    code: None,
                    reason: ExitReason::Exit,
                    generation: 42,
                }) if pty_id == "pty-123" && session_id.as_deref() == Some("session-456")
            )
        );
    }

    #[test]
    fn flusher_sends_exited_event_on_error() {
        let output = PtyOutput::new();
        let (lifecycle_tx, lifecycle_rx) = crate::pty_lifecycle::channel();

        let tx = spawn_flusher_with_lifecycle(
            output,
            "pty-err".to_string(),
            None,
            99,
            lifecycle_tx,
            true,
        );

        // Send Error to trigger exit
        tx.send(PtyChunk::Error).unwrap();

        // Verify lifecycle event was sent with IoError reason
        let event = lifecycle_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(
                event,
                PtyLifecycleMessage::Event(PtyLifecycleEvent::Exited {
                    pty_id,
                    session_id: None,
                    code: None,
                    reason: ExitReason::IoError,
                    generation: 99,
                }) if pty_id == "pty-err"
            )
        );
    }

    #[test]
    fn flusher_flushes_batch_before_sending_exit() {
        let output = PtyOutput::new();
        let (lifecycle_tx, lifecycle_rx) = crate::pty_lifecycle::channel();
        let received = Arc::new(Mutex::new(Vec::new()));

        // Attach channel to capture output
        output.attach(super::tests::raw_channel(received.clone()));

        let tx = spawn_flusher_with_lifecycle(
            output,
            "pty-flush".to_string(),
            None,
            1,
            lifecycle_tx,
            true,
        );

        // Send data then EOF
        tx.send(PtyChunk::Data(vec![1, 2, 3])).unwrap();
        tx.send(PtyChunk::Eof).unwrap();

        // Wait for exit event
        let event = lifecycle_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(event, PtyLifecycleMessage::Event(PtyLifecycleEvent::Exited { .. })));

        // Data should have been flushed before exit
        let data = received.lock().unwrap();
        assert!(!data.is_empty(), "data should be flushed before exit");
        assert_eq!(data[0], vec![1, 2, 3]);
    }

    #[test]
    fn flusher_can_skip_exit_event_when_disabled() {
        let output = PtyOutput::new();
        let (lifecycle_tx, lifecycle_rx) = crate::pty_lifecycle::channel();

        let tx = spawn_flusher_with_lifecycle(
            output,
            "pty-no-exit".to_string(),
            None,
            7,
            lifecycle_tx,
            false,
        );

        tx.send(PtyChunk::Eof).unwrap();

        assert!(matches!(
            lifecycle_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}

#[cfg(test)]
mod nono_tests {
    use super::*;

    #[test]
    fn resolved_allow_dirs_expands_tilde() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["~/data".into()] };
        let resolved = nono.resolved_allow_dirs("/work");
        assert!(resolved[0].starts_with('/'), "should be absolute: {}", resolved[0]);
        assert!(resolved[0].ends_with("/data"), "should end with /data: {}", resolved[0]);
        assert!(!resolved[0].contains('~'), "should not contain tilde: {}", resolved[0]);
    }

    #[test]
    fn resolved_allow_dirs_resolves_relative() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["local/dir".into()] };
        let resolved = nono.resolved_allow_dirs("/work/project");
        assert_eq!(resolved[0], "/work/project/local/dir");
    }

    #[test]
    fn resolved_allow_dirs_passes_absolute_through() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["/tmp/scratch".into()] };
        let resolved = nono.resolved_allow_dirs("/work");
        assert_eq!(resolved[0], "/tmp/scratch");
    }

    #[test]
    fn resolved_allow_dirs_handles_bare_tilde() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["~".into()] };
        let resolved = nono.resolved_allow_dirs("/work");
        let home = dirs::home_dir().unwrap();
        assert_eq!(resolved[0], home.to_string_lossy());
    }

    #[test]
    fn resolved_allow_dirs_handles_empty() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec![] };
        let resolved = nono.resolved_allow_dirs("/work");
        assert!(resolved.is_empty());
    }
}

#[cfg(test)]
mod lifecycle_command_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Arc;

    #[derive(Debug)]
    struct FakeChild {
        kill_count: Arc<AtomicUsize>,
    }

    impl portable_pty::ChildKiller for FakeChild {
        fn kill(&mut self) -> std::io::Result<()> {
            self.kill_count.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            Box::new(FakeChild { kill_count: Arc::clone(&self.kill_count) })
        }
    }

    impl portable_pty::Child for FakeChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            Ok(Some(portable_pty::ExitStatus::with_exit_code(0)))
        }

        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            Ok(portable_pty::ExitStatus::with_exit_code(0))
        }

        fn process_id(&self) -> Option<u32> {
            None
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<*mut std::ffi::c_void> {
            None
        }
    }

    fn make_test_session(
        session_id: Option<&str>,
        status: PtyStatus,
    ) -> (PtySession, Arc<AtomicUsize>) {
        make_test_session_with_generation(session_id, status, 1)
    }

    fn make_test_session_with_generation(
        session_id: Option<&str>,
        status: PtyStatus,
        generation: u64,
    ) -> (PtySession, Arc<AtomicUsize>) {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize::default()).expect("openpty");
        let writer = pair.master.take_writer().expect("writer");
        let kill_count = Arc::new(AtomicUsize::new(0));
        let child: Box<dyn portable_pty::Child + Send> =
            Box::new(FakeChild { kill_count: Arc::clone(&kill_count) });

        let session = PtySession {
            master: pair.master,
            child,
            writer: Arc::new(Mutex::new(writer)),
            output: PtyOutput::new(),
            generation,
            ready_gate: None,
            role: PtyRole::Secondary,
            status,
            exit_info: None,
            session_id: session_id.map(ToString::to_string),
            name: None,
            working_dir: Some("/tmp".to_string()),
            profile: Some("plain-shell".to_string()),
            last_size: (80, 24),
            last_activity: Instant::now(),
            unread_output: false,
            bell_pending: false,
            logger: None,
        };

        (session, kill_count)
    }

    fn spawn_command_only_handler(manager: Arc<PtyManager>) -> crate::pty_lifecycle::LifecycleTx {
        let (tx, rx) = crate::pty_lifecycle::channel();
        std::thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                if let crate::pty_lifecycle::PtyLifecycleMessage::Command(command, reply) = message {
                    crate::pty_lifecycle::handle_command(&manager, *command);
                    let _ = reply.send(());
                }
            }
        });
        tx
    }

    fn register_via_bus(
        tx: &crate::pty_lifecycle::LifecycleTx,
        pty_id: &str,
        session: PtySession,
    ) {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        tx.send(crate::pty_lifecycle::PtyLifecycleMessage::Command(
            Box::new(crate::pty_lifecycle::PtyLifecycleCommand::Register {
                pty_id: pty_id.to_string(),
                session: Box::new(session),
            }),
            reply_tx,
        ))
        .expect("register command");
        reply_rx.recv().expect("register ack");
    }

    #[test]
    fn bus_backed_commands_update_pty_state_synchronously() {
        let manager = Arc::new(PtyManager::new());
        let lifecycle_tx = spawn_command_only_handler(Arc::clone(&manager));
        manager.set_lifecycle_tx(lifecycle_tx.clone());

        let (session, _) = make_test_session(
            Some("session-a"),
            PtyStatus::RunningAttached { pane_id: "pane-a".to_string() },
        );
        register_via_bus(&lifecycle_tx, "pty-a", session);

        manager.set_unread_output_direct("pty-a", true);
        manager.set_bell_pending_direct("pty-a", true);
        manager.detach("pty-a");
        manager.attach_to_pane("pty-a", "pane-b");
        manager.set_name("pty-a", Some("Renamed"));
        manager.mark_read("pty-a");

        let snapshot = manager.list_for_session("session-a");
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(
            snapshot[0].status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-b"
        ));
        assert_eq!(snapshot[0].name.as_deref(), Some("Renamed"));
        assert!(!snapshot[0].unread_output);
        assert!(!snapshot[0].bell_pending);
    }

    #[test]
    fn bus_backed_kill_session_ptys_removes_all_matching_ptys() {
        let manager = Arc::new(PtyManager::new());
        let lifecycle_tx = spawn_command_only_handler(Arc::clone(&manager));
        manager.set_lifecycle_tx(lifecycle_tx.clone());

        let (session_a, kill_a) = make_test_session(
            Some("session-a"),
            PtyStatus::RunningAttached { pane_id: "pane-a".to_string() },
        );
        let (session_b, kill_b) = make_test_session(
            Some("session-a"),
            PtyStatus::RunningDetached { since_ms: 1 },
        );
        let (session_other, kill_other) = make_test_session(
            Some("session-b"),
            PtyStatus::RunningAttached { pane_id: "pane-other".to_string() },
        );

        register_via_bus(&lifecycle_tx, "pty-a", session_a);
        register_via_bus(&lifecycle_tx, "pty-b", session_b);
        register_via_bus(&lifecycle_tx, "pty-other", session_other);

        manager.kill_session_ptys("session-a");

        assert!(manager.list_for_session("session-a").is_empty());
        assert_eq!(manager.list_for_session("session-b").len(), 1);
        assert_eq!(kill_a.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(kill_b.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(kill_other.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn session_primary_can_reregister_after_archive_kills_old_ptys() {
        let manager = Arc::new(PtyManager::new());
        let lifecycle_tx = spawn_command_only_handler(Arc::clone(&manager));
        manager.set_lifecycle_tx(lifecycle_tx.clone());

        let (old_primary, old_kill) = make_test_session(
            Some("session-a"),
            PtyStatus::RunningAttached { pane_id: "session-a-main".to_string() },
        );
        let (old_secondary, secondary_kill) = make_test_session(
            Some("session-a"),
            PtyStatus::RunningDetached { since_ms: 1 },
        );
        register_via_bus(&lifecycle_tx, "session-a", old_primary);
        register_via_bus(&lifecycle_tx, "pty-secondary", old_secondary);

        manager.kill_session_ptys("session-a");

        assert!(manager.list_for_session("session-a").is_empty());
        assert_eq!(old_kill.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(secondary_kill.load(AtomicOrdering::SeqCst), 1);

        let (mut restored_primary, restored_kill) = make_test_session(
            Some("session-a"),
            PtyStatus::RunningAttached { pane_id: "session-a-main".to_string() },
        );
        restored_primary.role = PtyRole::SessionPrimary;
        register_via_bus(&lifecycle_tx, "session-a", restored_primary);

        let snapshot = manager.list_for_session("session-a");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].id, "session-a");
        assert!(matches!(snapshot[0].role, PtyRole::SessionPrimary));
        assert_eq!(restored_kill.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn mark_exited_if_generation_matches_ignores_stale_exit() {
        let manager = Arc::new(PtyManager::new());
        let lifecycle_tx = spawn_command_only_handler(Arc::clone(&manager));
        manager.set_lifecycle_tx(lifecycle_tx.clone());

        let (session, _) = make_test_session_with_generation(
            Some("session-a"),
            PtyStatus::RunningAttached { pane_id: "pane-a".to_string() },
            2,
        );
        register_via_bus(&lifecycle_tx, "pty-a", session);

        assert!(!manager.mark_exited_if_generation_matches_direct("pty-a", 1, Some(1)));

        let snapshot = manager.list_for_session("session-a");
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(
            snapshot[0].status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-a"
        ));
    }

    #[test]
    fn mark_exited_if_generation_matches_marks_active_generation_exited() {
        let manager = Arc::new(PtyManager::new());
        let lifecycle_tx = spawn_command_only_handler(Arc::clone(&manager));
        manager.set_lifecycle_tx(lifecycle_tx.clone());

        let (session, _) = make_test_session_with_generation(
            Some("session-a"),
            PtyStatus::RunningAttached { pane_id: "pane-a".to_string() },
            2,
        );
        register_via_bus(&lifecycle_tx, "pty-a", session);

        assert!(manager.mark_exited_if_generation_matches_direct("pty-a", 2, Some(0)));

        let snapshot = manager.list_for_session("session-a");
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(snapshot[0].status, PtyStatus::Exited { code: Some(0), .. }));
    }
}
