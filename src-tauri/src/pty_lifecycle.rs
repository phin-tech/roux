//! PTY lifecycle event bus.
//!
//! Centralizes handling of PTY state transitions (exit, output-while-detached,
//! bell-while-detached) so the flusher thread doesn't need to know about
//! PtyManager internals, frontend events, or agent registry.
//!
//! The bus runs in its own thread, receiving events from flushers and
//! dispatching to the appropriate handlers.

use std::sync::{mpsc, Arc};
use std::thread;

pub use roux_runtime::pty_lifecycle::{ExitReason, PtyLifecycleEvent, PtyMetadataCommand};

pub enum PtyLifecycleCommand {
    Register {
        pty_id: String,
        session: Box<crate::pty::PtySession>,
    },
    Kill {
        pty_id: String,
    },
    KillSessionPtys {
        session_id: String,
    },
    Metadata(PtyMetadataCommand),
}

pub enum PtyLifecycleMessage {
    Event(PtyLifecycleEvent),
    Command(Box<PtyLifecycleCommand>, mpsc::SyncSender<()>),
}

impl std::fmt::Debug for PtyLifecycleCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Register { pty_id, .. } => {
                f.debug_struct("Register").field("pty_id", pty_id).finish()
            }
            Self::Kill { pty_id } => f.debug_struct("Kill").field("pty_id", pty_id).finish(),
            Self::KillSessionPtys { session_id } => f
                .debug_struct("KillSessionPtys")
                .field("session_id", session_id)
                .finish(),
            Self::Metadata(command) => f.debug_tuple("Metadata").field(command).finish(),
        }
    }
}

impl std::fmt::Debug for PtyLifecycleMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Event(event) => f.debug_tuple("Event").field(event).finish(),
            Self::Command(command, _) => f.debug_tuple("Command").field(command).finish(),
        }
    }
}

/// Sender half of the lifecycle bus. Clone and pass to flusher threads.
pub type LifecycleTx = mpsc::Sender<PtyLifecycleMessage>;

/// Receiver half of the lifecycle bus. Owned by the bus handler thread.
pub type LifecycleRx = mpsc::Receiver<PtyLifecycleMessage>;

/// Create a new lifecycle bus channel pair.
pub fn channel() -> (LifecycleTx, LifecycleRx) {
    roux_runtime::pty_lifecycle::channel()
}

/// Context needed by the lifecycle handler to dispatch events.
pub struct LifecycleHandlerContext {
    pub pty_manager: Arc<crate::pty::PtyManager>,
    pub agent_registry_tx: mpsc::Sender<crate::agent_registry::RegistryMessage>,
    pub automation_hooks: crate::automation_hooks::AutomationHookManager,
    pub app: tauri::AppHandle,
}

/// Spawn the lifecycle handler thread. Returns the sender for submitting events.
///
/// The handler thread runs until the sender is dropped (all clones gone).
pub fn spawn_handler(ctx: LifecycleHandlerContext) -> LifecycleTx {
    let (tx, rx) = channel();

    thread::spawn(move || {
        while let Ok(message) = rx.recv() {
            match message {
                PtyLifecycleMessage::Event(event) => handle_event(&ctx, event),
                PtyLifecycleMessage::Command(command, reply) => {
                    handle_command(&ctx.pty_manager, *command);
                    let _ = reply.send(());
                }
            }
        }
        rlog!("PTY lifecycle handler shutting down");
    });

    tx
}

fn handle_event(ctx: &LifecycleHandlerContext, event: PtyLifecycleEvent) {
    match event {
        PtyLifecycleEvent::Exited {
            pty_id,
            session_id,
            code,
            reason,
            generation,
        } => {
            // Drop stale exit events from a previous PTY generation before they
            // mutate state or notify the frontend for a reused PTY id.
            if !ctx
                .pty_manager
                .mark_exited_if_generation_matches_direct(&pty_id, generation, code.map(|c| c as i32))
            {
                rlog!(
                    "PTY lifecycle: dropping stale exit for {} generation {}",
                    pty_id,
                    generation
                );
                return;
            }
            let pty_info = ctx.pty_manager.get_info_direct(&pty_id);

            // 2. Emit frontend event
            use tauri::Emitter;
            let event_name = format!("session-exit:{}", pty_id);
            let _ = ctx.app.emit(
                &event_name,
                &roux_core::SessionExitPayload {
                    code,
                    generation,
                    reason: reason.into(),
                },
            );

            // 3. Notify agent registry if session_id present
            if let Some(sid) = session_id {
                let _ = ctx.agent_registry_tx.send(
                    crate::agent_registry::RegistryMessage::SessionEnded {
                        session_id: sid,
                    },
                );
            }

            if let Some(info) = pty_info {
                if info.profile.as_deref() == Some("task") {
                    let event = if code == Some(0) {
                        crate::automation_hooks::HookEvent::PostTaskSuccess
                    } else {
                        crate::automation_hooks::HookEvent::PostTaskFailure
                    };
                    let context = crate::automation_hooks::HookContext {
                        repo_path: info.working_dir.clone(),
                        worktree_path: info.working_dir.clone(),
                        task_id: Some(pty_id.clone()),
                        session_id: info.session_id.clone(),
                        scope: info.session_id.as_ref().map(|_| "session".to_string()),
                        cwd: info.working_dir,
                        ..crate::automation_hooks::HookContext::new(event)
                    };
                    ctx.automation_hooks.spawn_background(event, context);
                }
            }

            rlog!(
                "PTY lifecycle: {} exited (code={:?}, reason={:?})",
                pty_id,
                code,
                reason
            );
        }

        PtyLifecycleEvent::OutputWhileDetached { pty_id } => {
            ctx.pty_manager.set_unread_output(&pty_id, true);
        }

        PtyLifecycleEvent::BellWhileDetached { pty_id } => {
            ctx.pty_manager.set_bell_pending(&pty_id, true);
        }
    }
}

pub(crate) fn handle_command(pty_manager: &crate::pty::PtyManager, command: PtyLifecycleCommand) {
    match command {
        PtyLifecycleCommand::Register { pty_id, session } => {
            pty_manager.register_session_direct(pty_id, *session);
        }
        PtyLifecycleCommand::Kill { pty_id } => {
            pty_manager.kill_direct(&pty_id);
        }
        PtyLifecycleCommand::KillSessionPtys { session_id } => {
            pty_manager.kill_session_ptys_direct(&session_id);
        }
        PtyLifecycleCommand::Metadata(command) => {
            pty_manager.apply_metadata_command_direct(&command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_can_send_and_receive_events() {
        let (tx, rx) = channel();

        tx.send(PtyLifecycleMessage::Event(PtyLifecycleEvent::Exited {
            pty_id: "pty-1".to_string(),
            session_id: Some("session-1".to_string()),
            code: Some(0),
            reason: ExitReason::Exit,
            generation: 1,
        }))
        .unwrap();

        tx.send(PtyLifecycleMessage::Event(PtyLifecycleEvent::OutputWhileDetached {
            pty_id: "pty-2".to_string(),
        }))
        .unwrap();

        let evt1 = rx.recv().unwrap();
        assert!(
            matches!(evt1, PtyLifecycleMessage::Event(PtyLifecycleEvent::Exited { pty_id, .. }) if pty_id == "pty-1")
        );

        let evt2 = rx.recv().unwrap();
        assert!(
            matches!(evt2, PtyLifecycleMessage::Event(PtyLifecycleEvent::OutputWhileDetached { pty_id }) if pty_id == "pty-2")
        );
    }

    #[test]
    fn lifecycle_tx_is_clone() {
        let (tx, rx) = channel();
        let tx2 = tx.clone();

        tx.send(PtyLifecycleMessage::Event(PtyLifecycleEvent::BellWhileDetached {
            pty_id: "pty-1".to_string(),
        }))
        .unwrap();

        tx2.send(PtyLifecycleMessage::Event(PtyLifecycleEvent::BellWhileDetached {
            pty_id: "pty-2".to_string(),
        }))
        .unwrap();

        drop(tx);
        drop(tx2);

        // Should receive both events
        let mut ids = vec![];
        while let Ok(evt) = rx.recv() {
            if let PtyLifecycleMessage::Event(PtyLifecycleEvent::BellWhileDetached { pty_id }) = evt {
                ids.push(pty_id);
            }
        }
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"pty-1".to_string()));
        assert!(ids.contains(&"pty-2".to_string()));
    }
}
