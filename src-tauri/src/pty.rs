use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
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
}

impl PtyOutputState {
    fn new() -> Self {
        Self { channel: None, backlog: VecDeque::new(), backlog_bytes: 0 }
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
    fn new() -> Self {
        Self { state: Arc::new(Mutex::new(PtyOutputState::new())) }
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
fn spawn_flusher(
    output: PtyOutput,
    exit_event: Option<(String, u64)>, // (event_name, generation)
    app: tauri::AppHandle,
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
                        let _ = app.emit(evt, &roux_core::SessionExitPayload {
                            code: None,
                            generation: *gen,
                            reason: roux_core::SessionExitReason::Exit,
                        });
                    }
                    break;
                }
                PtyChunk::Error => {
                    if !batch.is_empty() {
                        output.send(std::mem::take(&mut batch));
                    }
                    if let Some((evt, gen)) = &exit_event {
                        let _ = app.emit(evt, &roux_core::SessionExitPayload {
                            code: None,
                            generation: *gen,
                            reason: roux_core::SessionExitReason::IoError,
                        });
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
        Err(std::io::Error::new(std::io::ErrorKind::Other, "child already waited"))
    }
    fn process_id(&self) -> Option<u32> {
        None
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<*mut std::ffi::c_void> {
        None
    }
}

struct PtySession {
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
    std::fs::read_link(format!("/proc/{}/cwd", pid))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
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
                    home.as_ref()
                        .map(|h| h.join(tail).to_string_lossy().into_owned())
                } else if std::path::Path::new(d).is_absolute() {
                    Some(d.clone())
                } else {
                    Some(
                        std::path::Path::new(working_dir)
                            .join(d)
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
            })
            .collect()
    }
}

pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
    pending_outputs: Mutex<HashMap<String, Channel<Response>>>,
    generation: AtomicU64,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            pending_outputs: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
        }
    }

    fn attach_pending_output(&self, session_id: &str, output: &PtyOutput) {
        if let Some(channel) = self.pending_outputs.lock().unwrap().remove(session_id) {
            output.attach(channel);
        }
    }

    pub fn spawn_shell(
        &self,
        id: &str,
        working_dir: &str,
        session_id: Option<&str>,
        pane_id: Option<&str>,
        nono: Option<&NonoConfig>,
        initial_size: Option<(u16, u16)>,
        app: tauri::AppHandle,
    ) -> Result<(), PtyError> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(pty_size_from(initial_size))
            .map_err(|source| PtyError::OpenPty { source })?;

        let shell = resolve_default_shell();
        let user_path = get_user_path();
        let nono_label = nono.map(|n| format!(" (nono profile={})", n.profile)).unwrap_or_default();
        let pane_label = pane_id.map(|p| format!(", pane '{}'", p)).unwrap_or_default();
        let session_label = session_id.map(|s| format!(", session '{}'", s)).unwrap_or_default();
        rlog!(
            "Spawning shell '{}' for PTY '{}'{}{} in '{}'{}",
            shell, id, pane_label, session_label, working_dir, nono_label
        );

        let mut cmd = if let Some(nono) = nono {
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
        apply_shell_command_flags(&mut cmd, &shell);
        apply_roux_env(&mut cmd, &user_path, session_id, pane_id);
        cmd.cwd(working_dir);

        let child = pair.slave.spawn_command(cmd).map_err(|source| {
            rlog!("Failed to spawn shell: {}", source);
            PtyError::SpawnShell { source }
        })?;

        let writer = pair.master.take_writer().map_err(|source| PtyError::GetWriter { source })?;

        let reader =
            pair.master.try_clone_reader().map_err(|source| PtyError::GetReader { source })?;

        let output = PtyOutput::new();
        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        let writer = Arc::new(Mutex::new(writer));
        let gate = Arc::new(Mutex::new(ShellReadyGate::new(
            Instant::now(),
            GATE_QUIET,
            GATE_TIMEOUT,
        )));

        let session = PtySession {
            master: pair.master,
            child,
            writer: Arc::clone(&writer),
            output: output.clone(),
            generation: gen,
            ready_gate: Some(Arc::clone(&gate)),
        };
        self.sessions.lock().unwrap().insert(id.to_string(), session);
        self.attach_pending_output(id, &output);

        let tx =
            spawn_flusher(output.clone(), Some((format!("session-exit:{}", id), gen)), app.clone());
        let sniffer = crate::notifications::OscSniffer::new(
            app.clone(),
            session_id.map(|s| s.to_string()),
        );
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
        initial_size: Option<(u16, u16)>,
        app: tauri::AppHandle,
    ) -> Result<(), PtyError> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(pty_size_from(initial_size))
            .map_err(|source| PtyError::OpenPty { source })?;

        let shell = resolve_default_shell();
        let user_path = get_user_path();

        let mut cmd = CommandBuilder::new(&shell);
        apply_task_command_args(&mut cmd, &shell, command);
        apply_roux_env(&mut cmd, &user_path, session_id, pane_id);
        cmd.cwd(working_dir);

        let mut child =
            pair.slave.spawn_command(cmd).map_err(|source| PtyError::SpawnTask { source })?;

        let writer = pair.master.take_writer().map_err(|source| PtyError::GetWriter { source })?;

        let reader =
            pair.master.try_clone_reader().map_err(|source| PtyError::GetReader { source })?;

        let output = PtyOutput::new();
        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        // Insert session before attaching pending output and starting threads
        let session = PtySession {
            master: pair.master,
            child: Box::new(WaitedChild),
            writer: Arc::new(Mutex::new(writer)),
            output: output.clone(),
            generation: gen,
            // One-shot tasks run the command as argv to the shell
            // (non-interactive), so there is no ZLE/readline init to race.
            ready_gate: None,
        };
        self.sessions.lock().unwrap().insert(id.to_string(), session);
        self.attach_pending_output(id, &output);

        let tx = spawn_flusher(output.clone(), None, app.clone());
        let sniffer = crate::notifications::OscSniffer::new(
            app.clone(),
            session_id.map(|s| s.to_string()),
        );
        spawn_reader(reader, tx, Some(sniffer), None);

        // Wait for the child process in a background thread and emit exit code
        let exit_event_name = format!("session-exit:{}", id);
        thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            let _ = app.emit(
                &exit_event_name,
                &roux_core::SessionExitPayload {
                    code,
                    generation: gen,
                    reason: roux_core::SessionExitReason::Exit,
                },
            );
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
        let session = self.sessions.lock().unwrap().remove(session_id);
        self.pending_outputs.lock().unwrap().remove(session_id);
        if let Some(mut session) = session {
            if let Err(e) = session.child.kill() {
                rlog!("Warning: kill failed for {}: {}", session_id, e);
            }
            // Give the child up to 2 seconds to exit
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

    pub fn get_generation(&self, session_id: &str) -> Option<u64> {
        self.sessions.lock().unwrap().get(session_id).map(|s| s.generation)
    }

    /// Resolve the current working directory of the shell (or claude) process
    /// attached to `pty_id`, by walking `portable_pty::Child::process_id` →
    /// OS-level cwd lookup. Returns `None` if the session doesn't exist, the
    /// child has exited, or the OS refuses.
    pub fn get_cwd(&self, pty_id: &str) -> Option<String> {
        let pid = self.sessions.lock().unwrap().get(pty_id).and_then(|s| s.child.process_id())?;
        cwd_for_pid(pid)
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
                        let src_modified = std::fs::metadata(&source).and_then(|m| m.modified()).ok();
                        let dst_modified = std::fs::metadata(&target).and_then(|m| m.modified()).ok();
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
fn apply_roux_env(
    cmd: &mut CommandBuilder,
    user_path: &str,
    session_id: Option<&str>,
    pane_id: Option<&str>,
) {
    cmd.env("PATH", build_pty_path(user_path));
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("ROUX_SESSION", "1");
    cmd.env("ROUX_SOCKET", socket_path_str());
    if let Some((_, cli_path)) = roux_cli_shim() {
        cmd.env("ROUX_CLI", cli_path);
    }
    if let Some(sid) = session_id {
        cmd.env("ROUX_SESSION_ID", sid);
    }
    if let Some(pid) = pane_id {
        cmd.env("ROUX_PANE_ID", pid);
    }
}

/// Get the user's login shell PATH by invoking their actual shell (from $SHELL)
/// instead of hardcoding /bin/bash. This ensures paths added in .zshrc etc. are found.
pub fn get_user_path() -> String {
    get_user_path_impl()
}

#[cfg(windows)]
fn get_user_path_impl() -> String {
    std::env::var("PATH").unwrap_or_default()
}

#[cfg(not(windows))]
fn get_user_path_impl() -> String {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
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
    #[cfg(windows)]
    {
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
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
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

    fn raw_channel(store: Arc<Mutex<Vec<Vec<u8>>>>) -> Channel<Response> {
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
mod nono_tests {
    use super::*;

    #[test]
    fn resolved_allow_dirs_expands_tilde() {
        let nono = NonoConfig {
            profile: "test".into(),
            allow_dirs: vec!["~/data".into()],
        };
        let resolved = nono.resolved_allow_dirs("/work");
        assert!(resolved[0].starts_with('/'), "should be absolute: {}", resolved[0]);
        assert!(resolved[0].ends_with("/data"), "should end with /data: {}", resolved[0]);
        assert!(!resolved[0].contains('~'), "should not contain tilde: {}", resolved[0]);
    }

    #[test]
    fn resolved_allow_dirs_resolves_relative() {
        let nono = NonoConfig {
            profile: "test".into(),
            allow_dirs: vec!["local/dir".into()],
        };
        let resolved = nono.resolved_allow_dirs("/work/project");
        assert_eq!(resolved[0], "/work/project/local/dir");
    }

    #[test]
    fn resolved_allow_dirs_passes_absolute_through() {
        let nono = NonoConfig {
            profile: "test".into(),
            allow_dirs: vec!["/tmp/scratch".into()],
        };
        let resolved = nono.resolved_allow_dirs("/work");
        assert_eq!(resolved[0], "/tmp/scratch");
    }

    #[test]
    fn resolved_allow_dirs_handles_bare_tilde() {
        let nono = NonoConfig {
            profile: "test".into(),
            allow_dirs: vec!["~".into()],
        };
        let resolved = nono.resolved_allow_dirs("/work");
        let home = dirs::home_dir().unwrap();
        assert_eq!(resolved[0], home.to_string_lossy());
    }

    #[test]
    fn resolved_allow_dirs_handles_empty() {
        let nono = NonoConfig {
            profile: "test".into(),
            allow_dirs: vec![],
        };
        let resolved = nono.resolved_allow_dirs("/work");
        assert!(resolved.is_empty());
    }
}
