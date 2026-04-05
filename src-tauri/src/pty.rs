use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::Read;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::Emitter;

enum PtyChunk {
    Data(Vec<u8>),
    Eof,
    Error,
}

/// Spawn a flusher thread that batches chunks from the reader and emits to the frontend
/// at ~16ms intervals. Returns the sender for the reader thread to push data into.
fn spawn_flusher(
    event_name: String,
    exit_event: Option<(String, serde_json::Value)>,
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
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&batch);
                        let _ = app.emit(&event_name, b64);
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
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&batch);
                        let _ = app.emit(&event_name, b64);
                        batch.clear();
                        last_flush = Instant::now();
                    }
                }
                PtyChunk::Eof | PtyChunk::Error => {
                    if !batch.is_empty() {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&batch);
                        let _ = app.emit(&event_name, b64);
                    }
                    if let Some((evt, payload)) = &exit_event {
                        let _ = app.emit(evt, payload.clone());
                    }
                    break;
                }
            }
        }
    });

    tx
}

/// Spawn a reader thread that blocks on PTY reads and sends chunks to the flusher.
fn spawn_reader(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<PtyChunk>) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(PtyChunk::Eof);
                    break;
                }
                Ok(n) => {
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
}

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    #[allow(dead_code)]
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn std::io::Write + Send>,
}

pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()) }
    }

    pub fn spawn(
        &self,
        session_id: &str,
        working_dir: &str,
        model: Option<&str>,
        additional_flags: &[String],
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        // Get the user's login shell PATH so we can find `claude`
        let user_path = std::process::Command::new("/bin/bash")
            .args(["-l", "-c", "echo $PATH"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());

        let mut cmd = CommandBuilder::new("claude");
        cmd.env("PATH", &user_path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(m) = model {
            cmd.arg("--model");
            cmd.arg(m);
        }
        for flag in additional_flags {
            cmd.arg(flag);
        }
        cmd.cwd(working_dir);

        let child =
            pair.slave.spawn_command(cmd).map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let writer =
            pair.master.take_writer().map_err(|e| format!("Failed to get PTY writer: {}", e))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

        let tx = spawn_flusher(
            format!("pty-output:{}", session_id),
            Some((format!("session-exit:{}", session_id), serde_json::json!({"code": null}))),
            app.clone(),
        );
        spawn_reader(reader, tx);

        let session = PtySession { master: pair.master, child, writer };

        self.sessions.lock().unwrap().insert(session_id.to_string(), session);

        Ok(())
    }

    pub fn spawn_shell(
        &self,
        id: &str,
        working_dir: &str,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

        let user_path = std::process::Command::new("/bin/bash")
            .args(["-l", "-c", "echo $PATH"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());

        let mut cmd = CommandBuilder::new(&shell);
        cmd.env("PATH", &user_path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.cwd(working_dir);

        let child =
            pair.slave.spawn_command(cmd).map_err(|e| format!("Failed to spawn shell: {}", e))?;

        let writer =
            pair.master.take_writer().map_err(|e| format!("Failed to get PTY writer: {}", e))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

        let tx = spawn_flusher(
            format!("pty-output:{}", id),
            Some((format!("session-exit:{}", id), serde_json::json!({"code": null}))),
            app.clone(),
        );
        spawn_reader(reader, tx);

        let session = PtySession { master: pair.master, child, writer };

        self.sessions.lock().unwrap().insert(id.to_string(), session);

        Ok(())
    }

    /// Spawn a one-shot command in a PTY. The PTY exits when the command finishes,
    /// and the real exit code is emitted via session-exit:{id}.
    pub fn spawn_task(
        &self,
        id: &str,
        command: &str,
        working_dir: &str,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

        let user_path = std::process::Command::new("/bin/bash")
            .args(["-l", "-c", "echo $PATH"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());

        let mut cmd = CommandBuilder::new(&shell);
        cmd.args(["-c", command]);
        cmd.env("PATH", &user_path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.cwd(working_dir);

        let mut child =
            pair.slave.spawn_command(cmd).map_err(|e| format!("Failed to spawn task: {}", e))?;

        let writer =
            pair.master.take_writer().map_err(|e| format!("Failed to get PTY writer: {}", e))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

        // Flusher without built-in exit event — we'll emit it ourselves after waiting on the child
        let tx = spawn_flusher(
            format!("pty-output:{}", id),
            None,
            app.clone(),
        );
        spawn_reader(reader, tx);

        // Wait for the child process in a background thread and emit exit code
        let exit_event_name = format!("session-exit:{}", id);
        thread::spawn(move || {
            let code = child
                .wait()
                .ok()
                .map(|status| status.exit_code());
            let _ = app.emit(&exit_event_name, serde_json::json!({ "code": code }));
        });

        let session = PtySession { master: pair.master, child: Box::new(WaitedChild), writer };

        self.sessions.lock().unwrap().insert(id.to_string(), session);

        Ok(())
    }

    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        use std::io::Write;
        session.writer.write_all(data).map_err(|e| format!("Write failed: {}", e))?;
        session.writer.flush().map_err(|e| format!("Flush failed: {}", e))
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap();
        let session =
            sessions.get(session_id).ok_or_else(|| format!("Session {} not found", session_id))?;

        session
            .master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Resize failed: {}", e))
    }

    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(mut session) = sessions.remove(session_id) {
            let _ = session.child.kill();
        }
        Ok(())
    }
}
