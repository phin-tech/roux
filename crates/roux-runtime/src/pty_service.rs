use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use roux_core::{PtyInfo, PtyRole, PtyStatus};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::pty_lifecycle::{apply_metadata_command, PtyMetadataCommand, PtyMetadataCommandResult};
use crate::pty_live::WaitedChild;
use crate::pty_session::{PtySessionMetadata, PtySessionMetadataInputs};
use crate::pty_spawn::{self, ShellSpawnPlanInputs, TaskSpawnPlanInputs};
use crate::terminal_env;

pub const PTY_OUTPUT_LIMIT_BYTES: usize = 256 * 1024;
pub const PTY_OUTPUT_DEFAULT_POLL_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PtyKind {
    Shell,
    Task,
}

#[derive(Debug, Clone)]
pub struct PtySpawnRequest {
    pub id: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub session_id: Option<String>,
    pub pane_id: Option<String>,
    pub profile: Option<String>,
    pub initial_size: Option<(u16, u16)>,
    pub role: PtyRole,
}

impl Default for PtySpawnRequest {
    fn default() -> Self {
        Self {
            id: None,
            working_dir: None,
            session_id: None,
            pane_id: None,
            profile: None,
            initial_size: None,
            role: PtyRole::Secondary,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyRecord {
    pub id: String,
    pub kind: PtyKind,
    pub command: Option<String>,
    pub working_dir: String,
    pub started_at_ms: u64,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub generation: u64,
    pub retained_output_bytes: usize,
    pub output_truncated: bool,
    pub cols: u16,
    pub rows: u16,
    pub info: PtyInfo,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtySnapshot {
    pub record: PtyRecord,
    pub output: String,
    pub output_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PtyOutputFrame {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum PtyOutputEvent {
    Output(PtyOutputFrame),
    Exit { code: Option<i32>, generation: u64 },
}

pub struct PtyAttach {
    pub record: PtyRecord,
    pub replay_offset: u64,
    pub replay_bytes: Vec<u8>,
    pub events: broadcast::Receiver<PtyOutputEvent>,
}

enum PtyMsg {
    SpawnShell {
        request: PtySpawnRequest,
        reply: oneshot::Sender<Result<PtyRecord, String>>,
    },
    SpawnTask {
        command: String,
        request: PtySpawnRequest,
        reply: oneshot::Sender<Result<PtyRecord, String>>,
    },
    Snapshot {
        id: String,
        max_bytes: usize,
        reply: oneshot::Sender<Option<PtySnapshot>>,
    },
    Attach {
        id: String,
        max_replay_bytes: usize,
        reply: oneshot::Sender<Option<PtyAttach>>,
    },
    Metadata {
        command: PtyMetadataCommand,
        reply: oneshot::Sender<Result<Option<PtyRecord>, String>>,
    },
    List {
        reply: oneshot::Sender<Vec<PtyRecord>>,
    },
    Write {
        id: String,
        data: Vec<u8>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Resize {
        id: String,
        cols: u16,
        rows: u16,
        reply: oneshot::Sender<Result<Option<PtyRecord>, String>>,
    },
    Kill {
        id: String,
        reply: oneshot::Sender<Result<Option<PtyRecord>, String>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PtyServiceError {
    #[error("pty service unavailable")]
    Unavailable,
    #[error("{0}")]
    Failed(String),
}

#[derive(Clone)]
pub struct PtyHandle {
    tx: mpsc::UnboundedSender<PtyMsg>,
}

impl PtyHandle {
    fn send(&self, msg: PtyMsg) -> Result<(), PtyServiceError> {
        self.tx.send(msg).map_err(|_| PtyServiceError::Unavailable)
    }

    pub async fn spawn_shell(
        &self,
        request: PtySpawnRequest,
    ) -> Result<PtyRecord, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::SpawnShell { request, reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)?.map_err(PtyServiceError::Failed)
    }

    pub async fn spawn_task(
        &self,
        command: String,
        request: PtySpawnRequest,
    ) -> Result<PtyRecord, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::SpawnTask { command, request, reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)?.map_err(PtyServiceError::Failed)
    }

    pub async fn snapshot(
        &self,
        id: &str,
        max_bytes: usize,
    ) -> Result<Option<PtySnapshot>, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::Snapshot { id: id.to_string(), max_bytes, reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)
    }

    pub async fn attach(
        &self,
        id: &str,
        max_replay_bytes: usize,
    ) -> Result<Option<PtyAttach>, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::Attach { id: id.to_string(), max_replay_bytes, reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)
    }

    pub async fn detach(&self, id: &str) -> Result<Option<PtyRecord>, PtyServiceError> {
        self.apply_metadata(PtyMetadataCommand::Detach { pty_id: id.to_string() }).await
    }

    pub async fn attach_to_pane(
        &self,
        id: &str,
        pane_id: String,
    ) -> Result<Option<PtyRecord>, PtyServiceError> {
        self.apply_metadata(PtyMetadataCommand::AttachToPane { pty_id: id.to_string(), pane_id })
            .await
    }

    pub async fn mark_read(&self, id: &str) -> Result<Option<PtyRecord>, PtyServiceError> {
        self.apply_metadata(PtyMetadataCommand::MarkRead { pty_id: id.to_string() }).await
    }

    pub async fn set_name(
        &self,
        id: &str,
        name: Option<String>,
    ) -> Result<Option<PtyRecord>, PtyServiceError> {
        self.apply_metadata(PtyMetadataCommand::SetName { pty_id: id.to_string(), name }).await
    }

    async fn apply_metadata(
        &self,
        command: PtyMetadataCommand,
    ) -> Result<Option<PtyRecord>, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::Metadata { command, reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)?.map_err(PtyServiceError::Failed)
    }

    pub async fn list(&self) -> Result<Vec<PtyRecord>, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::List { reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)
    }

    pub async fn write(&self, id: &str, data: Vec<u8>) -> Result<(), PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::Write { id: id.to_string(), data, reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)?.map_err(PtyServiceError::Failed)
    }

    pub async fn resize(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Option<PtyRecord>, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::Resize { id: id.to_string(), cols, rows, reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)?.map_err(PtyServiceError::Failed)
    }

    pub async fn kill(&self, id: &str) -> Result<Option<PtyRecord>, PtyServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PtyMsg::Kill { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| PtyServiceError::Unavailable)?.map_err(PtyServiceError::Failed)
    }

    pub async fn shutdown(&self) {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.tx.send(PtyMsg::Shutdown { reply: reply_tx });
        let _ = reply_rx.await;
    }
}

pub fn spawn() -> (PtyHandle, JoinHandle<()>) {
    let (handle, future) = service();
    let join = tokio::spawn(future);
    (handle, join)
}

pub fn service() -> (PtyHandle, impl std::future::Future<Output = ()> + Send + 'static) {
    let (tx, rx) = mpsc::unbounded_channel();
    (PtyHandle { tx }, service_loop(rx))
}

async fn service_loop(mut rx: mpsc::UnboundedReceiver<PtyMsg>) {
    let mut ptys: HashMap<String, PtyEntry> = HashMap::new();
    let mut next_id = 1_u64;
    let mut next_generation = 1_u64;

    while let Some(msg) = rx.recv().await {
        match msg {
            PtyMsg::SpawnShell { request, reply } => {
                let (id, should_increment) = allocate_id(request.id.as_deref(), next_id);
                if ptys.contains_key(&id) {
                    let _ = reply.send(Err(format!("daemon pty {id} already exists")));
                    continue;
                }
                if should_increment {
                    next_id += 1;
                }
                let result = spawn_pty(PtyKind::Shell, id.clone(), None, request, next_generation);
                next_generation += 1;
                match result {
                    Ok(entry) => {
                        let record = entry.record();
                        ptys.insert(id, entry);
                        let _ = reply.send(Ok(record));
                    }
                    Err(err) => {
                        let _ = reply.send(Err(err));
                    }
                }
            }
            PtyMsg::SpawnTask { command, request, reply } => {
                let command = command.trim().to_string();
                if command.is_empty() {
                    let _ = reply.send(Err("command required".to_string()));
                    continue;
                }
                let (id, should_increment) = allocate_id(request.id.as_deref(), next_id);
                if ptys.contains_key(&id) {
                    let _ = reply.send(Err(format!("daemon pty {id} already exists")));
                    continue;
                }
                if should_increment {
                    next_id += 1;
                }
                let result =
                    spawn_pty(PtyKind::Task, id.clone(), Some(command), request, next_generation);
                next_generation += 1;
                match result {
                    Ok(entry) => {
                        let record = entry.record();
                        ptys.insert(id, entry);
                        let _ = reply.send(Ok(record));
                    }
                    Err(err) => {
                        let _ = reply.send(Err(err));
                    }
                }
            }
            PtyMsg::Snapshot { id, max_bytes, reply } => {
                let snapshot = ptys.get_mut(&id).map(|entry| {
                    entry.refresh_status();
                    entry.snapshot(max_bytes)
                });
                let _ = reply.send(snapshot);
            }
            PtyMsg::Attach { id, max_replay_bytes, reply } => {
                let attach = ptys.get_mut(&id).map(|entry| entry.attach(max_replay_bytes));
                let _ = reply.send(attach);
            }
            PtyMsg::Metadata { command, reply } => {
                let result = match ptys.get_mut(command.pty_id()) {
                    Some(entry) => entry.apply_metadata(&command),
                    None => Ok(None),
                };
                let _ = reply.send(result);
            }
            PtyMsg::List { reply } => {
                let mut records: Vec<_> = ptys
                    .values_mut()
                    .map(|entry| {
                        entry.refresh_status();
                        entry.record()
                    })
                    .collect();
                records.sort_by(|a, b| a.id.cmp(&b.id));
                let _ = reply.send(records);
            }
            PtyMsg::Write { id, data, reply } => {
                let result = ptys
                    .get_mut(&id)
                    .ok_or_else(|| "daemon pty not found".to_string())
                    .and_then(|entry| entry.write(&data));
                let _ = reply.send(result);
            }
            PtyMsg::Resize { id, cols, rows, reply } => {
                let result = match ptys.get_mut(&id) {
                    Some(entry) => entry.resize(cols, rows).map(|_| {
                        entry.refresh_status();
                        Some(entry.record())
                    }),
                    None => Ok(None),
                };
                let _ = reply.send(result);
            }
            PtyMsg::Kill { id, reply } => {
                let result = match ptys.get_mut(&id) {
                    Some(entry) => entry.kill().map(|_| {
                        entry.refresh_status();
                        Some(entry.record())
                    }),
                    None => Ok(None),
                };
                let _ = reply.send(result);
            }
            PtyMsg::Shutdown { reply } => {
                for entry in ptys.values_mut() {
                    let _ = entry.kill();
                }
                let _ = reply.send(());
                break;
            }
        }
    }
}

fn allocate_id(requested: Option<&str>, next_id: u64) -> (String, bool) {
    match requested.filter(|id| !id.trim().is_empty()) {
        Some(id) => (id.to_string(), false),
        None => (format!("daemon-pty-{next_id}"), true),
    }
}

struct PtyEntry {
    id: String,
    kind: PtyKind,
    command: Option<String>,
    working_dir: PathBuf,
    started_at_ms: u64,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    output: Arc<Mutex<PtyOutputBuffer>>,
    events: broadcast::Sender<PtyOutputEvent>,
    metadata: PtySessionMetadata,
    generation: u64,
    exit_code: Option<i32>,
}

impl PtyEntry {
    fn refresh_status(&mut self) {
        if matches!(self.metadata.status, PtyStatus::Exited { .. }) {
            return;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.exit_code = Some(status.exit_code() as i32);
                self.metadata.mark_exited(self.exit_code, unix_now_ms());
            }
            Ok(None) => {}
            Err(_) => {
                self.exit_code = None;
                self.metadata.mark_exited(None, unix_now_ms());
            }
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        if matches!(self.metadata.status, PtyStatus::Exited { .. }) {
            return Err("daemon pty has exited".to_string());
        }
        let mut writer =
            self.writer.lock().map_err(|_| "daemon pty writer lock poisoned".to_string())?;
        writer
            .write_all(data)
            .and_then(|_| writer.flush())
            .map_err(|err| format!("write daemon pty {}: {err}", self.id))
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| format!("resize daemon pty {}: {err}", self.id))?;
        self.metadata.last_size = (cols.max(1), rows.max(1));
        Ok(())
    }

    fn kill(&mut self) -> Result<(), String> {
        self.refresh_status();
        if matches!(self.metadata.status, PtyStatus::Exited { .. }) {
            return Ok(());
        }
        self.child.kill().map_err(|err| format!("kill daemon pty {}: {err}", self.id))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.refresh_status();
            if matches!(self.metadata.status, PtyStatus::Exited { .. }) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn record(&self) -> PtyRecord {
        let (retained_output_bytes, output_truncated) = self
            .output
            .lock()
            .map(|output| (output.len_bytes(), output.is_truncated()))
            .unwrap_or((0, false));
        let (cols, rows) = self.metadata.last_size;
        PtyRecord {
            id: self.id.clone(),
            kind: self.kind,
            command: self.command.clone(),
            working_dir: self.working_dir.to_string_lossy().to_string(),
            started_at_ms: self.started_at_ms,
            running: !matches!(self.metadata.status, PtyStatus::Exited { .. }),
            exit_code: self.exit_code,
            generation: self.generation,
            retained_output_bytes,
            output_truncated,
            cols,
            rows,
            info: self.metadata.to_info(&self.id),
        }
    }

    fn snapshot(&self, max_bytes: usize) -> PtySnapshot {
        let output_bytes =
            self.output.lock().map(|output| output.snapshot_bytes(max_bytes)).unwrap_or_default();
        let output = String::from_utf8_lossy(&output_bytes).into_owned();
        PtySnapshot { record: self.record(), output, output_bytes }
    }

    fn attach(&mut self, max_replay_bytes: usize) -> PtyAttach {
        let events = self.events.subscribe();
        self.refresh_status();
        let (replay_offset, replay_bytes) = self
            .output
            .lock()
            .map(|output| output.snapshot_bytes_with_offset(max_replay_bytes))
            .unwrap_or_else(|_| (0, Vec::new()));
        PtyAttach { record: self.record(), replay_offset, replay_bytes, events }
    }

    fn apply_metadata(
        &mut self,
        command: &PtyMetadataCommand,
    ) -> Result<Option<PtyRecord>, String> {
        let result = apply_metadata_command(&mut self.metadata, self.generation, command);
        match result {
            PtyMetadataCommandResult::Applied => {
                self.refresh_status();
                Ok(Some(self.record()))
            }
            PtyMetadataCommandResult::StaleGeneration => {
                Err(format!("stale daemon pty generation for {}", self.id))
            }
            PtyMetadataCommandResult::Missing => Ok(None),
        }
    }
}

fn spawn_pty(
    kind: PtyKind,
    id: String,
    command: Option<String>,
    request: PtySpawnRequest,
    generation: u64,
) -> Result<PtyEntry, String> {
    let working_dir = resolve_working_dir(request.working_dir)?;
    let working_dir_str = working_dir.to_string_lossy().to_string();
    let shell = resolve_default_shell();
    let roux_env: Vec<(String, String)> = std::env::vars().collect();

    let spawn_plan = match kind {
        PtyKind::Shell => pty_spawn::shell_spawn_plan(ShellSpawnPlanInputs {
            working_dir: &working_dir_str,
            shell: &shell,
            roux_env: &roux_env,
            worktree_path: None,
            nono: None,
            smolvm: None,
            initial_size: request.initial_size,
        }),
        PtyKind::Task => pty_spawn::task_spawn_plan(TaskSpawnPlanInputs {
            command: command.as_deref().unwrap_or_default(),
            working_dir: &working_dir_str,
            shell: &shell,
            roux_env: &roux_env,
            worktree_path: None,
            smolvm: None,
            initial_size: request.initial_size,
        }),
    };

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size_from_dimensions(spawn_plan.size))
        .map_err(|err| format!("open daemon pty: {err}"))?;
    let cmd = command_builder_from_plan(&spawn_plan.command);
    let child =
        pair.slave.spawn_command(cmd).map_err(|err| format!("spawn daemon pty {}: {err}", id))?;
    let (events, _) = broadcast::channel(1024);
    let child = wrap_child_with_waiter(child, events.clone(), generation);
    let writer =
        pair.master.take_writer().map_err(|err| format!("get daemon pty writer: {err}"))?;
    let reader =
        pair.master.try_clone_reader().map_err(|err| format!("get daemon pty reader: {err}"))?;
    let output = Arc::new(Mutex::new(PtyOutputBuffer::new(PTY_OUTPUT_LIMIT_BYTES)));
    spawn_output_reader(reader, Arc::clone(&output), events.clone());

    let size = spawn_plan.size.as_tuple();
    let metadata = PtySessionMetadata::new(PtySessionMetadataInputs {
        role: request.role,
        pane_id: request.pane_id.as_deref(),
        detached_since_ms: unix_now_ms(),
        session_id: request.session_id.as_deref(),
        working_dir: Some(&working_dir_str),
        profile: request.profile.as_deref(),
        last_size: size,
    });

    Ok(PtyEntry {
        id,
        kind,
        command,
        working_dir,
        started_at_ms: unix_now_ms(),
        master: pair.master,
        child,
        writer: Arc::new(Mutex::new(writer)),
        output,
        events,
        metadata,
        generation,
        exit_code: None,
    })
}

fn wrap_child_with_waiter(
    mut child: Box<dyn portable_pty::Child + Send>,
    events: broadcast::Sender<PtyOutputEvent>,
    generation: u64,
) -> Box<dyn portable_pty::Child + Send> {
    let waited_child = WaitedChild::new(child.clone_killer());
    let waited_child_exit = waited_child.exit_state();
    thread::spawn(move || {
        let wait_result = child.wait();
        let code = wait_result.as_ref().ok().map(|status| status.exit_code() as i32);
        waited_child_exit.record_wait_result(wait_result);
        let _ = events.send(PtyOutputEvent::Exit { code, generation });
    });
    Box::new(waited_child)
}

fn pty_size_from_dimensions(size: pty_spawn::PtyDimensions) -> PtySize {
    PtySize { rows: size.rows, cols: size.cols, pixel_width: 0, pixel_height: 0 }
}

fn command_builder_from_plan(plan: &pty_spawn::PtyCommandPlan) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(plan.program.as_os_str());
    for arg in &plan.args {
        cmd.arg(arg);
    }
    for (key, value) in &plan.env {
        cmd.env(key, value);
    }
    cmd.cwd(&plan.cwd);
    cmd
}

fn resolve_working_dir(working_dir: Option<PathBuf>) -> Result<PathBuf, String> {
    match working_dir {
        Some(path) if path.is_absolute() => Ok(path),
        Some(path) => std::env::current_dir()
            .map_err(|err| format!("resolve current directory: {err}"))
            .map(|cwd| cwd.join(path)),
        None => std::env::current_dir().map_err(|err| format!("resolve current directory: {err}")),
    }
}

fn resolve_default_shell() -> String {
    #[cfg(windows)]
    {
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
    }

    #[cfg(not(windows))]
    {
        terminal_env::resolve_default_shell_from_sources(
            None,
            terminal_env::login_shell_for_current_user().as_deref(),
            std::env::var("SHELL").ok().as_deref(),
        )
    }
}

struct PtyOutputBuffer {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    total_bytes: u64,
    limit_bytes: usize,
    truncated: bool,
}

impl PtyOutputBuffer {
    fn new(limit_bytes: usize) -> Self {
        Self { chunks: VecDeque::new(), bytes: 0, total_bytes: 0, limit_bytes, truncated: false }
    }

    fn append(&mut self, bytes: &[u8]) -> Option<PtyOutputFrame> {
        if bytes.is_empty() {
            return None;
        }
        let offset = self.total_bytes;
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        self.bytes += bytes.len();
        self.chunks.push_back(bytes.to_vec());
        while self.bytes > self.limit_bytes {
            let Some(removed) = self.chunks.pop_front() else {
                self.bytes = 0;
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.len());
            self.truncated = true;
        }
        Some(PtyOutputFrame { offset, bytes: bytes.to_vec() })
    }

    fn snapshot_bytes(&self, max_bytes: usize) -> Vec<u8> {
        self.snapshot_bytes_with_offset(max_bytes).1
    }

    fn snapshot_bytes_with_offset(&self, max_bytes: usize) -> (u64, Vec<u8>) {
        if max_bytes == 0 {
            return (self.total_bytes, Vec::new());
        }
        let mut bytes = Vec::with_capacity(self.bytes.min(max_bytes));
        for chunk in &self.chunks {
            bytes.extend_from_slice(chunk);
        }
        let retained_offset = self.total_bytes.saturating_sub(bytes.len() as u64);
        if bytes.len() > max_bytes {
            let start = bytes.len() - max_bytes;
            return (retained_offset.saturating_add(start as u64), bytes[start..].to_vec());
        }
        (retained_offset, bytes)
    }

    fn len_bytes(&self) -> usize {
        self.bytes
    }

    fn is_truncated(&self) -> bool {
        self.truncated
    }
}

fn spawn_output_reader(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<PtyOutputBuffer>>,
    events: broadcast::Sender<PtyOutputEvent>,
) {
    thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut output) = output.lock() {
                        if let Some(frame) = output.append(&buf[..n]) {
                            let _ = events.send(PtyOutputEvent::Output(frame));
                        }
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[tokio::test]
    async fn spawn_task_and_snapshot_captures_pty_output() {
        let dir = tempfile::tempdir().unwrap();
        let (handle, join) = spawn();

        let record = handle
            .spawn_task(
                "printf daemon-runtime-pty".to_string(),
                PtySpawnRequest {
                    working_dir: Some(dir.path().to_path_buf()),
                    session_id: Some("session-a".to_string()),
                    pane_id: Some("pane-a".to_string()),
                    profile: Some("task".to_string()),
                    initial_size: Some((100, 30)),
                    ..PtySpawnRequest::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(record.command.as_deref(), Some("printf daemon-runtime-pty"));
        assert_eq!(record.working_dir, dir.path().to_string_lossy());
        assert_eq!(record.info.session_id.as_deref(), Some("session-a"));
        assert_eq!(record.cols, 100);
        assert_eq!(record.rows, 30);

        let mut snapshot = None;
        for _ in 0..50 {
            let next = handle.snapshot(&record.id, 1024).await.unwrap().unwrap();
            if next.output.contains("daemon-runtime-pty") && !next.record.running {
                snapshot = Some(next);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let snapshot = snapshot.expect("PTY output should be retained");
        assert_eq!(snapshot.record.exit_code, Some(0));
        assert_eq!(snapshot.output_bytes, snapshot.output.as_bytes());

        handle.shutdown().await;
        join.await.unwrap();
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn duplicate_requested_ids_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (handle, join) = spawn();

        let request = PtySpawnRequest {
            id: Some("pty-fixed".to_string()),
            working_dir: Some(dir.path().to_path_buf()),
            ..PtySpawnRequest::default()
        };
        let first = handle.spawn_task("printf first".to_string(), request.clone()).await.unwrap();
        assert_eq!(first.id, "pty-fixed");

        let err =
            handle.spawn_task("printf second".to_string(), request).await.unwrap_err().to_string();
        assert!(err.contains("already exists"), "got: {err}");

        handle.shutdown().await;
        join.await.unwrap();
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn metadata_commands_update_daemon_pty_info() {
        let dir = tempfile::tempdir().unwrap();
        let (handle, join) = spawn();

        let record = handle
            .spawn_task(
                "sleep 1".to_string(),
                PtySpawnRequest {
                    id: Some("pty-meta".to_string()),
                    working_dir: Some(dir.path().to_path_buf()),
                    session_id: Some("session-a".to_string()),
                    pane_id: Some("pane-a".to_string()),
                    ..PtySpawnRequest::default()
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            record.info.status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-a"
        ));

        let detached = handle.detach("pty-meta").await.unwrap().unwrap();
        assert!(matches!(detached.info.status, PtyStatus::RunningDetached { .. }));

        let attached =
            handle.attach_to_pane("pty-meta", "pane-b".to_string()).await.unwrap().unwrap();
        assert!(matches!(
            attached.info.status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-b"
        ));

        let named =
            handle.set_name("pty-meta", Some("Build shell".to_string())).await.unwrap().unwrap();
        assert_eq!(named.info.name.as_deref(), Some("Build shell"));

        let unnamed = handle.set_name("pty-meta", None).await.unwrap().unwrap();
        assert_eq!(unnamed.info.name, None);

        assert!(handle.mark_read("missing").await.unwrap().is_none());

        let _ = handle.kill("pty-meta").await;
        handle.shutdown().await;
        join.await.unwrap();
    }

    #[test]
    fn output_buffer_reports_offsets_for_retained_bytes() {
        let mut output = PtyOutputBuffer::new(5);

        let first = output.append(b"abc").unwrap();
        assert_eq!(first.offset, 0);
        assert_eq!(first.bytes, b"abc");

        let second = output.append(b"de").unwrap();
        assert_eq!(second.offset, 3);

        let third = output.append(b"fg").unwrap();
        assert_eq!(third.offset, 5);

        let (offset, bytes) = output.snapshot_bytes_with_offset(10);
        assert_eq!(offset, 3);
        assert_eq!(bytes, b"defg");
        assert!(output.is_truncated());

        let (offset, bytes) = output.snapshot_bytes_with_offset(2);
        assert_eq!(offset, 5);
        assert_eq!(bytes, b"fg");
    }
}
