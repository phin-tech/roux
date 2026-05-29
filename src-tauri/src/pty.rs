use portable_pty::{native_pty_system, CommandBuilder, PtySize};
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
use crate::pty_ready_gate::ShellReadyGate;

pub use roux_core::{PtyInfo, PtyRole, PtyStatus};
pub use roux_runtime::process::cwd_for_pid;
use roux_runtime::pty_lifecycle::{PtyMetadataCommand, PtyMetadataCommandResult};
use roux_runtime::pty_live::{
    PtyError, PtyOutput as RuntimePtyOutput, PtyOutputLogger, PtyOutputSink,
    PtySession as RuntimePtySession, PtyWriter, ReadyGate, WaitedChild,
};
#[cfg(test)]
pub use roux_runtime::pty_output::PTY_BACKLOG_LIMIT_BYTES;
use roux_runtime::pty_output::{
    plan_reader_step, PtyOutputChunk, PtyOutputFlushAction, PtyOutputFlusher, PtyReaderPlan,
    PtyReaderStep,
};
use roux_runtime::pty_pending_output::PtyPendingOutput;
use roux_runtime::pty_registry::PtySessionRegistry;
use roux_runtime::pty_session::{PtySessionMetadata, PtySessionMetadataInputs};
use roux_runtime::pty_spawn::{self, ShellSpawnPlanInputs, TaskSpawnPlanInputs};
use roux_runtime::terminal_env;
pub use roux_runtime::terminal_env::{NonoConfig, NotesEnvInputs};

const GATE_QUIET: Duration = Duration::from_millis(200);
const GATE_TIMEOUT: Duration = Duration::from_secs(5);
const GATE_TICK: Duration = Duration::from_millis(75);

pub(crate) struct TauriOutputChannel(Channel<Response>);

impl PtyOutputSink for TauriOutputChannel {
    fn send_output(&self, bytes: Vec<u8>) -> bool {
        self.0.send(Response::new(bytes)).is_ok()
    }
}

impl PtyOutputLogger for crate::pty_logger::PtyLogger {
    fn write_output(&mut self, bytes: &[u8]) {
        crate::pty_logger::PtyLogger::write(self, bytes);
    }

    fn recent_output(&self, max_bytes: usize) -> Vec<u8> {
        crate::pty_logger::PtyLogger::recent(self, max_bytes)
    }
}

type PtyOutput = RuntimePtyOutput<TauriOutputChannel, crate::pty_logger::PtyLogger>;
pub(crate) type PtySession = RuntimePtySession<TauriOutputChannel, crate::pty_logger::PtyLogger>;

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

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
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

/// Spawn a flusher thread that batches chunks from the reader and sends them to the frontend
/// at ~16ms intervals. Returns the sender for the reader thread to push data into.
/// Optional "let the agent registry know this session is gone"
/// plumbing, bundled alongside the Tauri event emission at EOF.
type ExitRegistryHook = (mpsc::Sender<crate::agent_registry::RegistryMessage>, String);

fn elapsed_ms_since(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis() as u64
}

fn apply_direct_flusher_action(
    output: &PtyOutput,
    action: PtyOutputFlushAction,
    exit_event: &Option<(String, u64)>,
    app: &tauri::AppHandle,
    exit_registry_hook: &Option<ExitRegistryHook>,
) -> bool {
    match action {
        PtyOutputFlushAction::Output(bytes) => {
            output.send(bytes);
            false
        }
        PtyOutputFlushAction::Exit(reason) => {
            if let Some((evt, gen)) = exit_event {
                let _ = app.emit(
                    evt,
                    &roux_core::SessionExitPayload {
                        code: None,
                        generation: *gen,
                        reason: reason.into(),
                    },
                );
            }
            if let Some((tx, sid)) = exit_registry_hook {
                let _ = tx.send(crate::agent_registry::RegistryMessage::SessionEnded {
                    session_id: sid.clone(),
                });
            }
            true
        }
    }
}

fn spawn_flusher(
    output: PtyOutput,
    exit_event: Option<(String, u64)>, // (event_name, generation)
    app: tauri::AppHandle,
    exit_registry_hook: Option<ExitRegistryHook>,
) -> mpsc::Sender<PtyOutputChunk> {
    let (tx, rx) = mpsc::channel::<PtyOutputChunk>();

    thread::spawn(move || {
        let started_at = Instant::now();
        let mut flusher = PtyOutputFlusher::new(0);

        loop {
            let chunk = match flusher.recv_timeout_ms(elapsed_ms_since(started_at)) {
                None => match rx.recv() {
                    Ok(c) => c,
                    Err(_) => break,
                },
                Some(timeout_ms) => match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
                    Ok(c) => c,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        for action in flusher.on_timeout(elapsed_ms_since(started_at)) {
                            apply_direct_flusher_action(
                                &output,
                                action,
                                &exit_event,
                                &app,
                                &exit_registry_hook,
                            );
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
            };

            for action in flusher.on_chunk(chunk, elapsed_ms_since(started_at)) {
                if apply_direct_flusher_action(
                    &output,
                    action,
                    &exit_event,
                    &app,
                    &exit_registry_hook,
                ) {
                    break;
                }
            }
            if flusher.is_finished() {
                break;
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
) -> mpsc::Sender<PtyOutputChunk> {
    let (tx, rx) = mpsc::channel::<PtyOutputChunk>();

    thread::spawn(move || {
        let started_at = Instant::now();
        let mut flusher = PtyOutputFlusher::new(0);

        loop {
            let chunk = match flusher.recv_timeout_ms(elapsed_ms_since(started_at)) {
                None => match rx.recv() {
                    Ok(c) => c,
                    Err(_) => break,
                },
                Some(timeout_ms) => match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
                    Ok(c) => c,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        for action in flusher.on_timeout(elapsed_ms_since(started_at)) {
                            if let PtyOutputFlushAction::Output(bytes) = action {
                                output.send(bytes);
                            }
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
            };

            for action in flusher.on_chunk(chunk, elapsed_ms_since(started_at)) {
                match action {
                    PtyOutputFlushAction::Output(bytes) => output.send(bytes),
                    PtyOutputFlushAction::Exit(reason) => {
                        if emit_exit_event {
                            let _ = lifecycle_tx.send(
                                crate::pty_lifecycle::PtyLifecycleMessage::Event(
                                    crate::pty_lifecycle::PtyLifecycleEvent::Exited {
                                        pty_id: pty_id.clone(),
                                        session_id: session_id.clone(),
                                        code: None,
                                        reason,
                                        generation,
                                    },
                                ),
                            );
                        }
                    }
                }
            }
            if flusher.is_finished() {
                break;
            }
        }
    });

    tx
}

fn apply_reader_plan(
    plan: PtyReaderPlan,
    tx: &mpsc::Sender<PtyOutputChunk>,
    sniffer: &mut Option<crate::notifications::OscSniffer>,
    gate: &Option<(ReadyGate, PtyWriter, String)>,
) -> bool {
    if let Some(bytes) = plan.observer_bytes {
        if let Some(s) = sniffer {
            s.feed(bytes);
        }
        // Feed the readiness gate. If this output opens the gate and had
        // writes buffered, flush them back into the PTY so the user's typed
        // command actually runs. Must not short-circuit the output send below:
        // a poisoned gate mutex would otherwise silently stop output forwarding.
        if let Some((g, w, id)) = gate {
            if let Ok(mut guard) = g.lock() {
                let flush = guard.on_output(bytes, Instant::now());
                drop(guard);
                flush_to_writer(w, &flush, &format!("reader({})", id));
            } else {
                rlog!("pty_ready_gate: reader saw poisoned gate mutex, skipping feed for {}", id,);
            }
        }
    }

    tx.send(plan.output_chunk).is_err() || plan.stop
}

/// Spawn a reader thread that blocks on PTY reads and sends chunks to the flusher.
/// If `sniffer` is provided, every chunk is also fed through the OSC parser
/// before being forwarded (non-consuming — bytes pass through unchanged).
fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: mpsc::Sender<PtyOutputChunk>,
    mut sniffer: Option<crate::notifications::OscSniffer>,
    gate: Option<(ReadyGate, PtyWriter, String)>,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            let plan = match reader.read(&mut buf) {
                Ok(0) => plan_reader_step(PtyReaderStep::Eof),
                Ok(n) => plan_reader_step(PtyReaderStep::Data(&buf[..n])),
                Err(_) => plan_reader_step(PtyReaderStep::Error),
            };
            if apply_reader_plan(plan, &tx, &mut sniffer, &gate) {
                break;
            }
        }
    });
}

pub struct PtyManager {
    sessions: Mutex<PtySessionRegistry<PtySession>>,
    pending_outputs: Mutex<PtyPendingOutput<TauriOutputChannel>>,
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
            sessions: Mutex::new(PtySessionRegistry::new()),
            pending_outputs: Mutex::new(PtyPendingOutput::new()),
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
    pub fn set_agent_sender(&self, sender: mpsc::Sender<crate::agent_registry::RegistryMessage>) {
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

    fn send_lifecycle_command(&self, command: crate::pty_lifecycle::PtyLifecycleCommand) -> bool {
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

    pub(crate) fn apply_metadata_command_direct(
        &self,
        command: &PtyMetadataCommand,
    ) -> PtyMetadataCommandResult {
        let mut sessions = self.sessions.lock().unwrap();
        let result = sessions.apply_metadata_command(command);
        if matches!(result, PtyMetadataCommandResult::Applied) {
            match command {
                PtyMetadataCommand::AttachToPane { pty_id, pane_id } => {
                    rlog!("PtyManager: attached PTY '{}' to pane '{}'", pty_id, pane_id);
                }
                PtyMetadataCommand::Detach { pty_id } => {
                    rlog!("PtyManager: detached PTY '{}'", pty_id);
                }
                _ => {}
            }
        }
        result
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

    pub(crate) fn kill_session_ptys_direct(&self, session_id: &str) {
        let ids = self.sessions.lock().unwrap().ids_for_session(session_id);
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
        initial_size: Option<(u16, u16)>,
        role: PtyRole,
        profile: Option<&str>,
        app: tauri::AppHandle,
    ) -> Result<(), PtyError> {
        let shell = resolve_default_shell();
        let user_path = get_user_path();
        let roux_env =
            roux_env_pairs(&user_path, session_id, pane_id, project_id, worktree_path, notes);
        let spawn_plan = pty_spawn::shell_spawn_plan(ShellSpawnPlanInputs {
            working_dir,
            shell: &shell,
            roux_env: &roux_env,
            worktree_path,
            nono,
            initial_size,
        });

        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(pty_size_from_dimensions(spawn_plan.size))
            .map_err(|source| PtyError::OpenPty { source })?;

        let nono_label = nono.map(|n| format!(" (nono profile={})", n.profile)).unwrap_or_default();
        let pane_label = pane_id.map(|p| format!(", pane '{}'", p)).unwrap_or_default();
        let session_label = session_id.map(|s| format!(", session '{}'", s)).unwrap_or_default();
        rlog!(
            "Spawning shell '{}' for PTY '{}'{}{} in '{}'{}",
            shell,
            id,
            pane_label,
            session_label,
            working_dir,
            nono_label
        );

        let cmd = command_builder_from_plan(&spawn_plan.command);

        let child = pair.slave.spawn_command(cmd).map_err(|source| {
            rlog!("Failed to spawn shell: {}", source);
            PtyError::SpawnShell { source }
        })?;

        let writer = pair.master.take_writer().map_err(|source| PtyError::GetWriter { source })?;

        let reader =
            pair.master.try_clone_reader().map_err(|source| PtyError::GetReader { source })?;

        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        let logger =
            Arc::new(Mutex::new(crate::pty_logger::PtyLogger::new(session_id.unwrap_or(id), id)));
        let output = PtyOutput::new_with_logger(Arc::clone(&logger));

        let writer = Arc::new(Mutex::new(writer));
        let gate =
            Arc::new(Mutex::new(ShellReadyGate::new(Instant::now(), GATE_QUIET, GATE_TIMEOUT)));

        let size = spawn_plan.size.as_tuple();
        let session = PtySession {
            master: pair.master,
            child,
            writer: Arc::clone(&writer),
            output: output.clone(),
            generation: gen,
            ready_gate: Some(Arc::clone(&gate)),
            metadata: PtySessionMetadata::new(PtySessionMetadataInputs {
                role,
                pane_id,
                detached_since_ms: unix_now_ms(),
                session_id,
                working_dir: Some(working_dir),
                profile,
                last_size: size,
            }),
            last_activity: std::time::Instant::now(),
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
                ))) => match *command {
                    crate::pty_lifecycle::PtyLifecycleCommand::Register { pty_id, session } => {
                        self.register_session_direct(pty_id, *session);
                    }
                    _ => unreachable!("register send only emits register commands here"),
                },
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
        initial_size: Option<(u16, u16)>,
        role: PtyRole,
        profile: Option<&str>,
        app: tauri::AppHandle,
    ) -> Result<(), PtyError> {
        let shell = resolve_default_shell();
        let user_path = get_user_path();
        let roux_env =
            roux_env_pairs(&user_path, session_id, pane_id, project_id, worktree_path, notes);
        let spawn_plan = pty_spawn::task_spawn_plan(TaskSpawnPlanInputs {
            command,
            working_dir,
            shell: &shell,
            roux_env: &roux_env,
            worktree_path,
            initial_size,
        });

        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(pty_size_from_dimensions(spawn_plan.size))
            .map_err(|source| PtyError::OpenPty { source })?;

        let cmd = command_builder_from_plan(&spawn_plan.command);

        let mut child =
            pair.slave.spawn_command(cmd).map_err(|source| PtyError::SpawnTask { source })?;
        let waited_child = WaitedChild::new(child.clone_killer());
        let waited_child_exit = waited_child.exit_state();

        let writer = pair.master.take_writer().map_err(|source| PtyError::GetWriter { source })?;

        let reader =
            pair.master.try_clone_reader().map_err(|source| PtyError::GetReader { source })?;

        let gen = self.generation.fetch_add(1, Ordering::Relaxed);

        let logger_task =
            Arc::new(Mutex::new(crate::pty_logger::PtyLogger::new(session_id.unwrap_or(id), id)));
        let output = PtyOutput::new_with_logger(Arc::clone(&logger_task));

        // Insert session before attaching pending output and starting threads
        let size_task = spawn_plan.size.as_tuple();
        let session = PtySession {
            master: pair.master,
            child: Box::new(waited_child),
            writer: Arc::new(Mutex::new(writer)),
            output: output.clone(),
            generation: gen,
            // One-shot tasks run the command as argv to the shell
            // (non-interactive), so there is no ZLE/readline init to race.
            ready_gate: None,
            metadata: PtySessionMetadata::new(PtySessionMetadataInputs {
                role,
                pane_id,
                detached_since_ms: unix_now_ms(),
                session_id,
                working_dir: Some(working_dir),
                profile,
                last_size: size_task,
            }),
            last_activity: std::time::Instant::now(),
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
                ))) => match *command {
                    crate::pty_lifecycle::PtyLifecycleCommand::Register { pty_id, session } => {
                        self.register_session_direct(pty_id, *session);
                    }
                    _ => unreachable!("register send only emits register commands here"),
                },
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
        let tx =
            spawn_flusher(output.clone(), None, app.clone(), self.exit_registry_info(session_id));
        let sniffer =
            crate::notifications::OscSniffer::new(app.clone(), session_id.map(|s| s.to_string()));
        spawn_reader(reader, tx, Some(sniffer), None);

        // Wait for the child process in a background thread and emit exit code
        let exit_pty_id = id.to_string();
        let exit_session_id = session_id.map(|s| s.to_string());
        thread::spawn(move || {
            let wait_result = child.wait();
            let code = wait_result.as_ref().ok().map(|status| status.exit_code());
            waited_child_exit.record_wait_result(wait_result);
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
        let channel = TauriOutputChannel(channel);
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
        pending.retain_existing(|id| sessions.contains_key(id));
    }

    pub fn kill(&self, session_id: &str) {
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::Kill {
            pty_id: session_id.to_string(),
        }) {
            self.kill_direct(session_id);
        }
    }

    pub fn get_generation(&self, session_id: &str) -> Option<u64> {
        self.sessions.lock().unwrap().generation(session_id)
    }

    pub(crate) fn get_info_direct(&self, pty_id: &str) -> Option<PtyInfo> {
        self.sessions.lock().unwrap().get_info(pty_id)
    }

    /// List PTY info snapshots for a given session (for picker UI).
    pub fn list_for_session(&self, session_id: &str) -> Vec<PtyInfo> {
        self.sessions.lock().unwrap().list_for_session(session_id)
    }

    /// List all PTY info snapshots in one pass.
    pub fn list_all(&self) -> Vec<PtyInfo> {
        self.sessions.lock().unwrap().list_all()
    }

    /// Detach a PTY from its pane (PTY keeps running).
    pub fn detach(&self, pty_id: &str) {
        let command = PtyMetadataCommand::Detach { pty_id: pty_id.to_string() };
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::Metadata(
            command.clone(),
        )) {
            self.apply_metadata_command_direct(&command);
        }
    }

    /// Mark a PTY as attached to a pane.
    pub fn attach_to_pane(&self, pty_id: &str, pane_id: &str) {
        let command = PtyMetadataCommand::AttachToPane {
            pty_id: pty_id.to_string(),
            pane_id: pane_id.to_string(),
        };
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::Metadata(
            command.clone(),
        )) {
            self.apply_metadata_command_direct(&command);
        }
    }

    /// Clear unread output and bell flags for a PTY.
    pub fn mark_read(&self, pty_id: &str) {
        let command = PtyMetadataCommand::MarkRead { pty_id: pty_id.to_string() };
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::Metadata(
            command.clone(),
        )) {
            self.apply_metadata_command_direct(&command);
        }
    }

    /// Set the display name for a PTY.
    pub fn set_name(&self, pty_id: &str, name: Option<&str>) {
        let command = PtyMetadataCommand::SetName {
            pty_id: pty_id.to_string(),
            name: name.map(str::to_string),
        };
        if !self.send_lifecycle_command(crate::pty_lifecycle::PtyLifecycleCommand::Metadata(
            command.clone(),
        )) {
            self.apply_metadata_command_direct(&command);
        }
    }

    /// Kill all PTY sessions for a session ID.
    pub fn kill_session_ptys(&self, session_id: &str) {
        if !self.send_lifecycle_command(
            crate::pty_lifecycle::PtyLifecycleCommand::KillSessionPtys {
                session_id: session_id.to_string(),
            },
        ) {
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

/// Eager trigger for the Roux CLI shim. Called from `main.rs` setup so the
/// symlink dir is ready before any PTY spawns and so we log the result at
/// startup for debugging. Safe to call repeatedly — cached behind a OnceLock.
pub fn ensure_roux_cli_shim() {
    let _ = roux_cli_shim();
}

/// Cached pair of (bin-dir-to-prepend-to-PATH, full-path-to-Roux-CLI).
/// Set up once at first PTY spawn: creates `~/.config/roux/bin/` and places
/// `roux` plus compatibility `roux-cli` symlinks there, both pointing at the
/// bundled CLI binary built next to the currently running desktop exe. Returning
/// `None` means we couldn't find the bundled CLI (e.g. a dev build where it wasn't
/// compiled yet) — callers skip the PATH injection gracefully.
fn roux_cli_shim() -> Option<(String, String)> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<(String, String)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            // 1. Find the bundled Roux CLI next to the currently running exe.
            let source = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(platform::roux_cli_file_name())))?;
            if !source.exists() {
                rlog!("roux_cli_shim: bundled CLI not found at {}", source.display());
                return None;
            }

            // 2. Ensure ~/.config/roux/bin/ exists.
            let bin_dir = crate::paths::roux_config_dir().join("bin");
            if let Err(e) = std::fs::create_dir_all(&bin_dir) {
                rlog!("roux_cli_shim: failed to create {}: {}", bin_dir.display(), e);
                return None;
            }

            // 3. Install symlinks: `roux` and legacy alias `roux-cli` both
            //    pointing at the bundled source. We re-create the links every
            //    startup so the PTY always sees the freshest binary, even
            //    after a version bump.
            #[cfg(unix)]
            {
                use std::os::unix::fs as unix_fs;
                for alias in ["roux", "roux-cli"] {
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
                for alias in ["roux.exe", "roux-cli.exe"] {
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

/// Build common Roux env vars with the Tauri-owned socket, CLI shim, and
/// persisted alias lookup.
fn roux_env_pairs(
    user_path: &str,
    session_id: Option<&str>,
    pane_id: Option<&str>,
    project_id: Option<&str>,
    worktree_path: Option<&str>,
    notes: Option<&NotesEnvInputs>,
) -> Vec<(String, String)> {
    let socket_path = socket_path_str();
    let cli_shim = roux_cli_shim();
    // Snapshot any alias bound to this pane at spawn time. The lookup hits
    // persisted `aliases.json`; live alias state remains available through
    // `roux alias whoami`.
    let pane_alias = pane_id.and_then(lookup_pane_alias);

    let output = terminal_env::roux_env_pairs_with_warnings(terminal_env::RouxEnvInputs {
        user_path,
        socket_path: &socket_path,
        cli_shim: cli_shim
            .as_ref()
            .map(|(bin_dir, cli_path)| (bin_dir.as_str(), cli_path.as_str())),
        session_id,
        pane_id,
        pane_alias: pane_alias.as_deref(),
        project_id,
        worktree_path,
        notes,
    });
    for warning in &output.warnings {
        let terminal_env::TerminalEnvWarning::ProjectContextPathsJoinFailed { path_count, error } =
            warning;
        rlog!(
            "notes_env_pairs: failed to encode ROUX_PROJECT_CONTEXT_PATHS ({} paths): {}",
            path_count,
            error
        );
    }
    output.pairs
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
        terminal_env::resolve_default_shell_from_sources(
            shell_binary_path.as_deref(),
            terminal_env::login_shell_for_current_user().as_deref(),
            std::env::var("SHELL").ok().as_deref(),
        )
    }
}

pub(crate) fn set_shell_binary_path_override(path: Option<String>) {
    let cache = shell_binary_path_cache();
    *cache.lock().unwrap() =
        Some(path.as_deref().and_then(terminal_env::nonempty_trimmed).map(str::to_string));
}

fn shell_binary_path_override() -> Option<String> {
    let cache = shell_binary_path_cache();
    let mut guard = cache.lock().unwrap();
    if guard.is_none() {
        *guard = Some(
            crate::settings::load_settings()
                .shell_binary_path
                .as_deref()
                .and_then(terminal_env::nonempty_trimmed)
                .map(str::to_string),
        );
    }
    guard.clone().flatten()
}

fn shell_binary_path_cache() -> &'static Mutex<Option<Option<String>>> {
    static CACHE: std::sync::OnceLock<Mutex<Option<Option<String>>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
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
        let logger =
            Arc::new(Mutex::new(crate::pty_logger::PtyLogger::new("test-sess", "test-pty")));
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
        output.attach(TauriOutputChannel(raw_channel(received.clone())));

        assert_eq!(*received.lock().unwrap(), vec![vec![1, 2, 3], vec![4, 5, 6]]);
    }

    #[test]
    fn trims_oldest_backlog_when_limit_is_exceeded() {
        let output = PtyOutput::new();
        output.send(vec![1; PTY_BACKLOG_LIMIT_BYTES / 2]);
        output.send(vec![2; PTY_BACKLOG_LIMIT_BYTES / 2]);
        output.send(vec![3; PTY_BACKLOG_LIMIT_BYTES / 2]);

        let received = Arc::new(Mutex::new(Vec::new()));
        output.attach(TauriOutputChannel(raw_channel(received.clone())));

        assert_eq!(
            *received.lock().unwrap(),
            vec![vec![2; PTY_BACKLOG_LIMIT_BYTES / 2], vec![3; PTY_BACKLOG_LIMIT_BYTES / 2]]
        );
    }

    #[test]
    fn sends_directly_once_channel_is_attached() {
        let output = PtyOutput::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        output.attach(TauriOutputChannel(raw_channel(received.clone())));

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
        let shell = terminal_env::resolve_default_shell_from_sources(
            Some(" /custom/fish "),
            Some("/bin/zsh"),
            Some("/bin/bash"),
        );

        assert_eq!(shell, "/custom/fish");
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_prefers_login_shell_over_env_shell() {
        let shell = terminal_env::resolve_default_shell_from_sources(
            None,
            Some("/opt/homebrew/bin/fish"),
            Some("/bin/zsh"),
        );

        assert_eq!(shell, "/opt/homebrew/bin/fish");
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_uses_env_shell_when_login_shell_is_unavailable() {
        let shell = terminal_env::resolve_default_shell_from_sources(None, None, Some("/bin/bash"));

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
        tx.send(PtyOutputChunk::Eof).unwrap();

        // Verify lifecycle event was sent
        let event = lifecycle_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            event,
            PtyLifecycleMessage::Event(PtyLifecycleEvent::Exited {
                pty_id,
                session_id,
                code: None,
                reason: ExitReason::Exit,
                generation: 42,
            }) if pty_id == "pty-123" && session_id.as_deref() == Some("session-456")
        ));
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
        tx.send(PtyOutputChunk::Error).unwrap();

        // Verify lifecycle event was sent with IoError reason
        let event = lifecycle_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            event,
            PtyLifecycleMessage::Event(PtyLifecycleEvent::Exited {
                pty_id,
                session_id: None,
                code: None,
                reason: ExitReason::IoError,
                generation: 99,
            }) if pty_id == "pty-err"
        ));
    }

    #[test]
    fn flusher_flushes_batch_before_sending_exit() {
        let output = PtyOutput::new();
        let (lifecycle_tx, lifecycle_rx) = crate::pty_lifecycle::channel();
        let received = Arc::new(Mutex::new(Vec::new()));

        // Attach channel to capture output
        output.attach(TauriOutputChannel(super::tests::raw_channel(received.clone())));

        let tx = spawn_flusher_with_lifecycle(
            output,
            "pty-flush".to_string(),
            None,
            1,
            lifecycle_tx,
            true,
        );

        // Send data then EOF
        tx.send(PtyOutputChunk::Data(vec![1, 2, 3])).unwrap();
        tx.send(PtyOutputChunk::Eof).unwrap();

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

        tx.send(PtyOutputChunk::Eof).unwrap();

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

        let mut metadata = PtySessionMetadata::new(PtySessionMetadataInputs {
            role: PtyRole::Secondary,
            pane_id: None,
            detached_since_ms: 1,
            session_id,
            working_dir: Some("/tmp"),
            profile: Some("plain-shell"),
            last_size: (80, 24),
        });
        metadata.status = status;

        let session = PtySession {
            master: pair.master,
            child,
            writer: Arc::new(Mutex::new(writer)),
            output: PtyOutput::new(),
            generation,
            ready_gate: None,
            metadata,
            last_activity: Instant::now(),
            logger: None,
        };

        (session, kill_count)
    }

    fn spawn_command_only_handler(manager: Arc<PtyManager>) -> crate::pty_lifecycle::LifecycleTx {
        let (tx, rx) = crate::pty_lifecycle::channel();
        std::thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                if let crate::pty_lifecycle::PtyLifecycleMessage::Command(command, reply) = message
                {
                    crate::pty_lifecycle::handle_command(&manager, *command);
                    let _ = reply.send(());
                }
            }
        });
        tx
    }

    fn register_via_bus(tx: &crate::pty_lifecycle::LifecycleTx, pty_id: &str, session: PtySession) {
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

        assert_eq!(
            manager.apply_metadata_command_direct(&PtyMetadataCommand::SetUnreadOutput {
                pty_id: "pty-a".to_string(),
                value: true,
            }),
            PtyMetadataCommandResult::Applied
        );
        assert_eq!(
            manager.apply_metadata_command_direct(&PtyMetadataCommand::SetBellPending {
                pty_id: "pty-a".to_string(),
                value: true,
            }),
            PtyMetadataCommandResult::Applied
        );
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
        let (session_b, kill_b) =
            make_test_session(Some("session-a"), PtyStatus::RunningDetached { since_ms: 1 });
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
        let (old_secondary, secondary_kill) =
            make_test_session(Some("session-a"), PtyStatus::RunningDetached { since_ms: 1 });
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
        restored_primary.metadata.role = PtyRole::SessionPrimary;
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

        assert_eq!(
            manager.apply_metadata_command_direct(
                &PtyMetadataCommand::MarkExitedIfGenerationMatches {
                    pty_id: "pty-a".to_string(),
                    generation: 1,
                    code: Some(1),
                    at_ms: 123,
                }
            ),
            PtyMetadataCommandResult::StaleGeneration
        );

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

        assert_eq!(
            manager.apply_metadata_command_direct(
                &PtyMetadataCommand::MarkExitedIfGenerationMatches {
                    pty_id: "pty-a".to_string(),
                    generation: 2,
                    code: Some(0),
                    at_ms: 123,
                }
            ),
            PtyMetadataCommandResult::Applied
        );

        let snapshot = manager.list_for_session("session-a");
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(snapshot[0].status, PtyStatus::Exited { code: Some(0), .. }));
    }
}
