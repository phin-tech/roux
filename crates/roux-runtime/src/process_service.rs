use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

pub const PROCESS_OUTPUT_LIMIT_BYTES: usize = 256 * 1024;
pub const PROCESS_OUTPUT_DEFAULT_POLL_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRecord {
    pub id: String,
    pub command: String,
    pub working_dir: String,
    pub started_at_ms: u64,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub retained_output_bytes: usize,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSnapshot {
    pub record: ProcessRecord,
    pub output: String,
}

enum ProcessMsg {
    Start {
        command: String,
        working_dir: Option<PathBuf>,
        reply: oneshot::Sender<Result<ProcessRecord, String>>,
    },
    Snapshot {
        id: String,
        max_bytes: usize,
        reply: oneshot::Sender<Option<ProcessSnapshot>>,
    },
    List {
        reply: oneshot::Sender<Vec<ProcessRecord>>,
    },
    Kill {
        id: String,
        reply: oneshot::Sender<Result<Option<ProcessRecord>, String>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessServiceError {
    #[error("process service unavailable")]
    Unavailable,
    #[error("{0}")]
    Failed(String),
}

#[derive(Clone)]
pub struct ProcessHandle {
    tx: mpsc::UnboundedSender<ProcessMsg>,
}

impl ProcessHandle {
    fn send(&self, msg: ProcessMsg) -> Result<(), ProcessServiceError> {
        self.tx.send(msg).map_err(|_| ProcessServiceError::Unavailable)
    }

    pub async fn start(
        &self,
        command: String,
        working_dir: Option<PathBuf>,
    ) -> Result<ProcessRecord, ProcessServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProcessMsg::Start { command, working_dir, reply: reply_tx })?;
        reply_rx
            .await
            .map_err(|_| ProcessServiceError::Unavailable)?
            .map_err(ProcessServiceError::Failed)
    }

    pub async fn snapshot(
        &self,
        id: &str,
        max_bytes: usize,
    ) -> Result<Option<ProcessSnapshot>, ProcessServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProcessMsg::Snapshot { id: id.to_string(), max_bytes, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ProcessServiceError::Unavailable)
    }

    pub async fn list(&self) -> Result<Vec<ProcessRecord>, ProcessServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProcessMsg::List { reply: reply_tx })?;
        reply_rx.await.map_err(|_| ProcessServiceError::Unavailable)
    }

    pub async fn kill(&self, id: &str) -> Result<Option<ProcessRecord>, ProcessServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProcessMsg::Kill { id: id.to_string(), reply: reply_tx })?;
        reply_rx
            .await
            .map_err(|_| ProcessServiceError::Unavailable)?
            .map_err(ProcessServiceError::Failed)
    }

    pub async fn shutdown(&self) {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.tx.send(ProcessMsg::Shutdown { reply: reply_tx });
        let _ = reply_rx.await;
    }
}

pub fn spawn() -> (ProcessHandle, JoinHandle<()>) {
    let (handle, future) = service();
    let join = tokio::spawn(future);
    (handle, join)
}

pub fn service() -> (ProcessHandle, impl std::future::Future<Output = ()> + Send + 'static) {
    let (tx, rx) = mpsc::unbounded_channel();
    (ProcessHandle { tx }, service_loop(rx))
}

async fn service_loop(mut rx: mpsc::UnboundedReceiver<ProcessMsg>) {
    let mut processes: HashMap<String, ProcessEntry> = HashMap::new();
    let mut next_id = 1_u64;

    while let Some(msg) = rx.recv().await {
        match msg {
            ProcessMsg::Start { command, working_dir, reply } => {
                let id = format!("daemon-process-{next_id}");
                next_id += 1;
                let result = ProcessEntry::spawn(id.clone(), command, working_dir);
                match result {
                    Ok(entry) => {
                        let record = entry.record();
                        processes.insert(id, entry);
                        let _ = reply.send(Ok(record));
                    }
                    Err(err) => {
                        let _ = reply.send(Err(err));
                    }
                }
            }
            ProcessMsg::Snapshot { id, max_bytes, reply } => {
                let snapshot = processes.get_mut(&id).map(|entry| {
                    entry.refresh_status();
                    entry.snapshot(max_bytes)
                });
                let _ = reply.send(snapshot);
            }
            ProcessMsg::List { reply } => {
                let mut records: Vec<_> = processes
                    .values_mut()
                    .map(|entry| {
                        entry.refresh_status();
                        entry.record()
                    })
                    .collect();
                records.sort_by(|a, b| a.id.cmp(&b.id));
                let _ = reply.send(records);
            }
            ProcessMsg::Kill { id, reply } => {
                let result = if let Some(entry) = processes.get_mut(&id) {
                    entry.kill().map(|_| Some(entry.record()))
                } else {
                    Ok(None)
                };
                let _ = reply.send(result);
            }
            ProcessMsg::Shutdown { reply } => {
                for entry in processes.values_mut() {
                    let _ = entry.kill();
                }
                let _ = reply.send(());
                break;
            }
        }
    }
}

struct ProcessEntry {
    id: String,
    command: String,
    working_dir: PathBuf,
    started_at_ms: u64,
    child: Option<Child>,
    reader_threads: Vec<thread::JoinHandle<()>>,
    exit_code: Option<i32>,
    output: Arc<Mutex<ProcessOutputBuffer>>,
}

impl ProcessEntry {
    fn spawn(id: String, command: String, working_dir: Option<PathBuf>) -> Result<Self, String> {
        let command = command.trim().to_string();
        if command.is_empty() {
            return Err("command required".to_string());
        }

        let working_dir = resolve_working_dir(working_dir)?;
        let output = Arc::new(Mutex::new(ProcessOutputBuffer::new(PROCESS_OUTPUT_LIMIT_BYTES)));
        if let Ok(mut output) = output.lock() {
            output.append(process_startup_log(&command, &working_dir).as_bytes());
        }

        let mut child = shell_command(&command)
            .current_dir(&working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("spawn daemon process: {err}"))?;

        let mut reader_threads = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            reader_threads.push(spawn_output_reader(stdout, Arc::clone(&output)));
        }
        if let Some(stderr) = child.stderr.take() {
            reader_threads.push(spawn_output_reader(stderr, Arc::clone(&output)));
        }

        Ok(Self {
            id,
            command,
            working_dir,
            started_at_ms: unix_now_ms(),
            child: Some(child),
            reader_threads,
            exit_code: None,
            output,
        })
    }

    fn refresh_status(&mut self) {
        let status = match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => {
                self.reap_finished_readers();
                return;
            }
        };
        match status {
            Ok(Some(status)) => {
                self.exit_code = status.code();
                self.child = None;
            }
            Ok(None) => {}
            Err(_) => {
                self.exit_code = None;
                self.child = None;
            }
        }
        self.reap_finished_readers();
    }

    fn kill(&mut self) -> Result<(), String> {
        let Some(child) = self.child.as_mut() else {
            self.reap_finished_readers();
            return Ok(());
        };
        terminate_child(child, &self.id)?;
        let status = wait_for_child_exit(child, &self.id)?;
        self.exit_code = status.code();
        self.child = None;
        self.reap_finished_readers();
        Ok(())
    }

    fn record(&self) -> ProcessRecord {
        let (retained_output_bytes, output_truncated) = self
            .output
            .lock()
            .map(|output| (output.len_bytes(), output.is_truncated()))
            .unwrap_or((0, false));
        ProcessRecord {
            id: self.id.clone(),
            command: self.command.clone(),
            working_dir: self.working_dir.to_string_lossy().to_string(),
            started_at_ms: self.started_at_ms,
            running: self.child.is_some() || !self.reader_threads.is_empty(),
            exit_code: self.exit_code,
            retained_output_bytes,
            output_truncated,
        }
    }

    fn snapshot(&self, max_bytes: usize) -> ProcessSnapshot {
        let output =
            self.output.lock().map(|output| output.snapshot_text(max_bytes)).unwrap_or_default();
        ProcessSnapshot { record: self.record(), output }
    }

    fn reap_finished_readers(&mut self) {
        let mut pending = Vec::new();
        for thread in self.reader_threads.drain(..) {
            if thread.is_finished() {
                let _ = thread.join();
            } else {
                pending.push(thread);
            }
        }
        self.reader_threads = pending;
    }
}

fn process_startup_log(command: &str, working_dir: &std::path::Path) -> String {
    format!("command: {command}\ncwd: {}\n\n", working_dir.to_string_lossy())
}

struct ProcessOutputBuffer {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    limit_bytes: usize,
    truncated: bool,
}

impl ProcessOutputBuffer {
    fn new(limit_bytes: usize) -> Self {
        Self { chunks: VecDeque::new(), bytes: 0, limit_bytes, truncated: false }
    }

    fn append(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
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
    }

    fn snapshot_text(&self, max_bytes: usize) -> String {
        if max_bytes == 0 {
            return String::new();
        }
        let mut bytes = Vec::with_capacity(self.bytes.min(max_bytes));
        for chunk in &self.chunks {
            bytes.extend_from_slice(chunk);
        }
        if bytes.len() > max_bytes {
            let start = bytes.len() - max_bytes;
            bytes = bytes[start..].to_vec();
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn len_bytes(&self) -> usize {
        self.bytes
    }

    fn is_truncated(&self) -> bool {
        self.truncated
    }
}

fn spawn_output_reader<R>(
    mut reader: R,
    output: Arc<Mutex<ProcessOutputBuffer>>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut output) = output.lock() {
                        output.append(&buf[..n]);
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn resolve_working_dir(working_dir: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = match working_dir {
        Some(path) if path.is_absolute() => path,
        Some(path) => std::env::current_dir()
            .map_err(|err| format!("resolve current directory: {err}"))?
            .join(path),
        None => {
            std::env::current_dir().map_err(|err| format!("resolve current directory: {err}"))?
        }
    };
    Ok(path)
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let shell = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string());
        let mut cmd = Command::new(shell);
        cmd.arg("/C").arg(command);
        cmd
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;

        let shell = std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.trim().is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());
        let mut cmd = Command::new(shell);
        cmd.arg("-lc").arg(command);
        cmd.process_group(0);
        cmd
    }
}

#[cfg(not(windows))]
fn terminate_child(child: &mut Child, id: &str) -> Result<(), String> {
    signal_child_group(child, id, libc::SIGTERM, "terminate")
}

#[cfg(windows)]
fn terminate_child(child: &mut Child, id: &str) -> Result<(), String> {
    child.kill().map_err(|err| format!("kill daemon process {id}: {err}"))
}

#[cfg(not(windows))]
fn force_terminate_child(child: &mut Child, id: &str) -> Result<(), String> {
    signal_child_group(child, id, libc::SIGKILL, "force kill")
}

#[cfg(windows)]
fn force_terminate_child(child: &mut Child, id: &str) -> Result<(), String> {
    child.kill().map_err(|err| format!("force kill daemon process {id}: {err}"))
}

#[cfg(not(windows))]
fn signal_child_group(
    child: &Child,
    id: &str,
    signal: libc::c_int,
    verb: &str,
) -> Result<(), String> {
    let pid = i32::try_from(child.id()).map_err(|_| format!("daemon process {id} pid overflow"))?;
    let result = unsafe { libc::kill(-pid, signal) };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ESRCH) {
            return Err(format!("{verb} daemon process group {id}: {err}"));
        }
    }
    Ok(())
}

fn wait_for_child_exit(child: &mut Child, id: &str) -> Result<std::process::ExitStatus, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() < Duration::from_millis(750) => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                force_terminate_child(child, id)?;
                return child.wait().map_err(|err| format!("wait daemon process {id}: {err}"));
            }
            Err(err) => return Err(format!("wait daemon process {id}: {err}")),
        }
    }
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
    async fn start_and_snapshot_captures_shell_output() {
        let dir = tempfile::tempdir().unwrap();
        let (handle, join) = spawn();

        let record = handle
            .start("printf daemon-runtime".to_string(), Some(dir.path().to_path_buf()))
            .await
            .unwrap();
        assert_eq!(record.command, "printf daemon-runtime");
        assert_eq!(record.working_dir, dir.path().to_string_lossy());
        assert!(record.running);

        let mut snapshot = None;
        for _ in 0..50 {
            let next = handle.snapshot(&record.id, 1024).await.unwrap().unwrap();
            if next.output.contains("daemon-runtime") && !next.record.running {
                snapshot = Some(next);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let snapshot = snapshot.expect("process output and exit should become visible");
        assert_eq!(snapshot.record.id, record.id);
        assert_eq!(snapshot.record.exit_code, Some(0));
        assert!(snapshot.output.contains("command: printf daemon-runtime"));
        assert!(snapshot.output.contains(&format!("cwd: {}", dir.path().to_string_lossy())));
        assert!(snapshot.output.ends_with("daemon-runtime"));

        handle.shutdown().await;
        join.await.unwrap();
    }

    #[test]
    fn output_buffer_retains_the_tail() {
        let mut output = ProcessOutputBuffer::new(5);
        output.append(b"abc");
        output.append(b"def");

        assert_eq!(output.snapshot_text(10), "def");
        assert!(output.is_truncated());
        assert!(output.len_bytes() <= 5);
    }

    #[test]
    fn record_remains_running_until_output_readers_are_reaped() {
        let output = Arc::new(Mutex::new(ProcessOutputBuffer::new(PROCESS_OUTPUT_LIMIT_BYTES)));
        output.lock().unwrap().append(b"ready");
        let reader = std::thread::spawn(|| std::thread::sleep(std::time::Duration::from_millis(5)));
        let mut entry = ProcessEntry {
            id: "daemon-process-test".to_string(),
            command: "printf ready".to_string(),
            working_dir: PathBuf::from("/tmp"),
            started_at_ms: 0,
            child: None,
            reader_threads: vec![reader],
            exit_code: Some(0),
            output,
        };

        assert!(entry.record().running);
        for _ in 0..50 {
            entry.refresh_status();
            if !entry.record().running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        assert!(!entry.record().running);
        assert_eq!(entry.snapshot(1024).output, "ready");
    }
}
