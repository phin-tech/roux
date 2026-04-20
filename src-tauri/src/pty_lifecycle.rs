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

/// Events that can occur during a PTY's lifecycle.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Variants reserved for future detach tracking
pub enum PtyLifecycleEvent {
    /// PTY process exited.
    Exited {
        pty_id: String,
        session_id: Option<String>,
        code: Option<u32>,
        reason: ExitReason,
        generation: u64,
    },
    /// Output arrived while PTY was detached.
    OutputWhileDetached { pty_id: String },
    /// Bell (BEL character) arrived while PTY was detached.
    BellWhileDetached { pty_id: String },
}

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
    Detach {
        pty_id: String,
    },
    AttachToPane {
        pty_id: String,
        pane_id: String,
    },
    MarkRead {
        pty_id: String,
    },
    SetName {
        pty_id: String,
        name: Option<String>,
    },
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
            Self::Detach { pty_id } => f.debug_struct("Detach").field("pty_id", pty_id).finish(),
            Self::AttachToPane { pty_id, pane_id } => f
                .debug_struct("AttachToPane")
                .field("pty_id", pty_id)
                .field("pane_id", pane_id)
                .finish(),
            Self::MarkRead { pty_id } => {
                f.debug_struct("MarkRead").field("pty_id", pty_id).finish()
            }
            Self::SetName { pty_id, name } => f
                .debug_struct("SetName")
                .field("pty_id", pty_id)
                .field("name", name)
                .finish(),
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExitReason {
    Exit,
    IoError,
    Killed,
}

impl From<ExitReason> for roux_core::SessionExitReason {
    fn from(r: ExitReason) -> Self {
        match r {
            ExitReason::Exit => roux_core::SessionExitReason::Exit,
            ExitReason::IoError => roux_core::SessionExitReason::IoError,
            ExitReason::Killed => roux_core::SessionExitReason::Killed,
        }
    }
}

/// Sender half of the lifecycle bus. Clone and pass to flusher threads.
pub type LifecycleTx = mpsc::Sender<PtyLifecycleMessage>;

/// Receiver half of the lifecycle bus. Owned by the bus handler thread.
pub type LifecycleRx = mpsc::Receiver<PtyLifecycleMessage>;

/// Create a new lifecycle bus channel pair.
pub fn channel() -> (LifecycleTx, LifecycleRx) {
    mpsc::channel()
}

/// Context needed by the lifecycle handler to dispatch events.
pub struct LifecycleHandlerContext {
    pub pty_manager: Arc<crate::pty::PtyManager>,
    pub agent_registry_tx: mpsc::Sender<crate::agent_registry::RegistryMessage>,
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
        PtyLifecycleCommand::Detach { pty_id } => {
            pty_manager.detach_direct(&pty_id);
        }
        PtyLifecycleCommand::AttachToPane { pty_id, pane_id } => {
            pty_manager.attach_to_pane_direct(&pty_id, &pane_id);
        }
        PtyLifecycleCommand::MarkRead { pty_id } => {
            pty_manager.mark_read_direct(&pty_id);
        }
        PtyLifecycleCommand::SetName { pty_id, name } => {
            pty_manager.set_name_direct(&pty_id, name.as_deref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_reason_converts_to_core_type() {
        assert_eq!(
            roux_core::SessionExitReason::from(ExitReason::Exit),
            roux_core::SessionExitReason::Exit
        );
        assert_eq!(
            roux_core::SessionExitReason::from(ExitReason::IoError),
            roux_core::SessionExitReason::IoError
        );
        assert_eq!(
            roux_core::SessionExitReason::from(ExitReason::Killed),
            roux_core::SessionExitReason::Killed
        );
    }

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
