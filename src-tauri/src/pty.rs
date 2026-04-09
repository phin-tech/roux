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

use crate::platform;

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
                        let _ = app.emit(
                            evt,
                            serde_json::json!({"code": null, "generation": gen, "reason": "exit"}),
                        );
                    }
                    break;
                }
                PtyChunk::Error => {
                    if !batch.is_empty() {
                        output.send(std::mem::take(&mut batch));
                    }
                    if let Some((evt, gen)) = &exit_event {
                        let _ = app.emit(evt, serde_json::json!({"code": null, "generation": gen, "reason": "io_error"}));
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

    pub fn spawn(
        &self,
        session_id: &str,
        working_dir: &str,
        model: Option<&str>,
        additional_flags: &[String],
        nono_profile: Option<&str>,
        claude_binary_path: Option<&str>,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let user_path = get_user_path();
        let claude_cmd = resolve_claude_command(claude_binary_path);
        rlog!(
            "Spawning claude session '{}' in '{}' with cmd='{}'",
            session_id,
            working_dir,
            claude_cmd
        );
        rlog!("  PATH: {}", user_path);

        let mut cmd = if let Some(profile) = nono_profile {
            // Wrap with nono: nono run --profile <profile> --allow-cwd -- claude [args]
            let mut c = CommandBuilder::new("nono");
            c.arg("run");
            c.arg("--profile");
            c.arg(profile);
            c.arg("--allow-cwd");
            c.arg("--");
            c.arg(&claude_cmd);
            c
        } else {
            CommandBuilder::new(&claude_cmd)
        };
        cmd.env("PATH", &user_path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("ROUX_SESSION", "1");
        cmd.env("ROUX_SOCKET", socket_path_str());
        cmd.env("ROUX_SESSION_ID", session_id);
        cmd.env("ROUX_PANE_ID", format!("{}-main", session_id));
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

        let output = PtyOutput::new();
        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        let session = PtySession {
            master: pair.master,
            child,
            writer: Arc::new(Mutex::new(writer)),
            output: output.clone(),
            generation: gen,
        };
        self.sessions.lock().unwrap().insert(session_id.to_string(), session);
        self.attach_pending_output(session_id, &output);

        let tx = spawn_flusher(
            output.clone(),
            Some((format!("session-exit:{}", session_id), gen)),
            app.clone(),
        );
        spawn_reader(reader, tx);

        Ok(())
    }

    pub fn spawn_shell(
        &self,
        id: &str,
        working_dir: &str,
        session_id: Option<&str>,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let shell = resolve_interactive_shell();
        let user_path = get_user_path();
        rlog!("Spawning shell '{}' for pane '{}' in '{}'", shell.program, id, working_dir);

        let mut cmd = CommandBuilder::new(&shell.program);
        for arg in &shell.args {
            cmd.arg(arg);
        }
        cmd.env("PATH", &user_path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("ROUX_SESSION", "1");
        cmd.env("ROUX_SOCKET", socket_path_str());
        if let Some(sid) = session_id {
            cmd.env("ROUX_SESSION_ID", sid);
        }
        cmd.cwd(working_dir);

        let child = pair.slave.spawn_command(cmd).map_err(|e| {
            rlog!("Failed to spawn shell: {}", e);
            format!("Failed to spawn shell: {}", e)
        })?;

        let writer =
            pair.master.take_writer().map_err(|e| format!("Failed to get PTY writer: {}", e))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

        let output = PtyOutput::new();
        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        let session = PtySession {
            master: pair.master,
            child,
            writer: Arc::new(Mutex::new(writer)),
            output: output.clone(),
            generation: gen,
        };
        self.sessions.lock().unwrap().insert(id.to_string(), session);
        self.attach_pending_output(id, &output);

        let tx =
            spawn_flusher(output.clone(), Some((format!("session-exit:{}", id), gen)), app.clone());
        spawn_reader(reader, tx);

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
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let shell = resolve_task_shell(command);
        let user_path = get_user_path();

        let mut cmd = CommandBuilder::new(&shell.program);
        for arg in &shell.args {
            cmd.arg(arg);
        }
        cmd.env("PATH", &user_path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("ROUX_SESSION", "1");
        cmd.env("ROUX_SOCKET", socket_path_str());
        if let Some(sid) = session_id {
            cmd.env("ROUX_SESSION_ID", sid);
        }
        cmd.cwd(working_dir);

        let mut child =
            pair.slave.spawn_command(cmd).map_err(|e| format!("Failed to spawn task: {}", e))?;

        let writer =
            pair.master.take_writer().map_err(|e| format!("Failed to get PTY writer: {}", e))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

        let output = PtyOutput::new();
        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        // Insert session before attaching pending output and starting threads
        let session = PtySession {
            master: pair.master,
            child: Box::new(WaitedChild),
            writer: Arc::new(Mutex::new(writer)),
            output: output.clone(),
            generation: gen,
        };
        self.sessions.lock().unwrap().insert(id.to_string(), session);
        self.attach_pending_output(id, &output);

        let tx = spawn_flusher(output.clone(), None, app.clone());
        spawn_reader(reader, tx);

        // Wait for the child process in a background thread and emit exit code
        let exit_event_name = format!("session-exit:{}", id);
        thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            let _ = app.emit(
                &exit_event_name,
                serde_json::json!({ "code": code, "generation": gen, "reason": "exit" }),
            );
        });

        Ok(())
    }

    pub fn attach_output_channel(
        &self,
        session_id: &str,
        channel: Channel<Response>,
    ) -> Result<(), String> {
        self.cleanup_stale_pending();
        let output =
            self.sessions.lock().unwrap().get(session_id).map(|session| session.output.clone());
        if let Some(output) = output {
            output.attach(channel);
        } else {
            self.pending_outputs.lock().unwrap().insert(session_id.to_string(), channel);
        }
        Ok(())
    }

    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let writer = {
            let sessions = self.sessions.lock().unwrap();
            let session = sessions
                .get(session_id)
                .ok_or_else(|| format!("Session {} not found", session_id))?;
            Arc::clone(&session.writer)
        };
        // Global lock released — only per-session writer lock held
        let mut writer = writer.lock().unwrap();
        use std::io::Write;
        writer.write_all(data).map_err(|e| format!("Write failed: {}", e))?;
        writer.flush().map_err(|e| format!("Flush failed: {}", e))
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let master = {
            let sessions = self.sessions.lock().unwrap();
            let session = sessions
                .get(session_id)
                .ok_or_else(|| format!("Session {} not found", session_id))?;
            // MasterPty doesn't impl Clone, so we need to keep the lock for resize.
            // However, we can at least use try_lock to avoid blocking other sessions.
            session
                .master
                .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                .map_err(|e| format!("Resize failed: {}", e))
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

    pub fn kill(&self, session_id: &str) -> Result<(), String> {
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
        Ok(())
    }

    pub fn get_generation(&self, session_id: &str) -> Option<u64> {
        self.sessions.lock().unwrap().get(session_id).map(|s| s.generation)
    }
}

/// Get the socket path as a string for setting env vars.
fn socket_path_str() -> String {
    platform::resolve_socket_endpoint()
        .unwrap_or_else(|| platform::socket_path().to_string_lossy().to_string())
}

/// Get the user's login shell PATH by invoking their actual shell (from $SHELL)
/// instead of hardcoding /bin/bash. This ensures paths added in .zshrc etc. are found.
pub fn get_user_path() -> String {
    #[cfg(windows)]
    {
        std::env::var("PATH").unwrap_or_default()
    }

    #[cfg(not(windows))]
    {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellSpec {
    program: String,
    args: Vec<String>,
}

fn resolve_interactive_shell() -> ShellSpec {
    #[cfg(windows)]
    {
        return ShellSpec { program: resolve_windows_shell_program(), args: Vec::new() };
    }

    #[cfg(not(windows))]
    ShellSpec {
        program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()),
        args: Vec::new(),
    }
}

fn resolve_task_shell(command: &str) -> ShellSpec {
    #[cfg(windows)]
    {
        let program = resolve_windows_shell_program();
        let args = windows_task_shell_args(&program, command);
        return ShellSpec { program, args };
    }

    #[cfg(not(windows))]
    ShellSpec {
        program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()),
        args: vec!["-c".to_string(), command.to_string()],
    }
}

#[cfg(windows)]
fn resolve_windows_shell_program() -> String {
    if platform::find_executable_on_path("pwsh.exe").is_some()
        || platform::find_executable_on_path("pwsh").is_some()
    {
        "pwsh".to_string()
    } else if platform::find_executable_on_path("powershell.exe").is_some() {
        "powershell.exe".to_string()
    } else {
        "cmd.exe".to_string()
    }
}

fn windows_task_shell_args(program: &str, command: &str) -> Vec<String> {
    if program.eq_ignore_ascii_case("cmd.exe") || program.eq_ignore_ascii_case("cmd") {
        vec!["/C".to_string(), command.to_string()]
    } else {
        let mut args = vec!["-NoProfile".to_string()];
        if program.eq_ignore_ascii_case("powershell.exe")
            || program.eq_ignore_ascii_case("powershell")
        {
            args.extend(["-ExecutionPolicy".to_string(), "Bypass".to_string()]);
        }
        args.extend(["-Command".to_string(), command.to_string()]);
        args
    }
}

/// Resolve the claude binary command. If a custom path is configured, use it directly.
/// Otherwise fall back to "claude" (resolved via PATH).
pub fn resolve_claude_command(custom_path: Option<&str>) -> String {
    match custom_path {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => "claude".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn resolve_claude_command_uses_custom_path_when_set() {
        let result = resolve_claude_command(Some("/usr/local/bin/claude"));
        assert_eq!(result, "/usr/local/bin/claude");
    }

    #[test]
    fn resolve_claude_command_uses_custom_path_with_home_dir() {
        let result = resolve_claude_command(Some("/Users/sam/.node_modules/bin/claude"));
        assert_eq!(result, "/Users/sam/.node_modules/bin/claude");
    }

    #[test]
    fn resolve_claude_command_falls_back_to_claude_when_none() {
        let result = resolve_claude_command(None);
        assert_eq!(result, "claude");
    }

    #[test]
    fn resolve_claude_command_falls_back_to_claude_when_empty() {
        let result = resolve_claude_command(Some(""));
        assert_eq!(result, "claude");
    }

    #[test]
    fn get_user_path_returns_nonempty_string() {
        let path = get_user_path();
        assert!(!path.is_empty(), "PATH should not be empty");
        #[cfg(windows)]
        assert!(
            path.to_ascii_lowercase().contains("\\windows"),
            "PATH should contain standard Windows directories, got: {}",
            path
        );

        #[cfg(not(windows))]
        assert!(
            path.contains("/usr/bin") || path.contains("/bin"),
            "PATH should contain standard bin directories, got: {}",
            path
        );
    }
}
