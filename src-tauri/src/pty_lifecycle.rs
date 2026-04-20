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
pub type LifecycleTx = mpsc::Sender<PtyLifecycleEvent>;

/// Receiver half of the lifecycle bus. Owned by the bus handler thread.
pub type LifecycleRx = mpsc::Receiver<PtyLifecycleEvent>;

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
        while let Ok(event) = rx.recv() {
            handle_event(&ctx, event);
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
            // 1. Mark PTY as exited in PtyManager (convert u32 -> i32 for internal storage)
            ctx.pty_manager.mark_exited(&pty_id, code.map(|c| c as i32));

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

        tx.send(PtyLifecycleEvent::Exited {
            pty_id: "pty-1".to_string(),
            session_id: Some("session-1".to_string()),
            code: Some(0),
            reason: ExitReason::Exit,
            generation: 1,
        })
        .unwrap();

        tx.send(PtyLifecycleEvent::OutputWhileDetached {
            pty_id: "pty-2".to_string(),
        })
        .unwrap();

        let evt1 = rx.recv().unwrap();
        assert!(matches!(evt1, PtyLifecycleEvent::Exited { pty_id, .. } if pty_id == "pty-1"));

        let evt2 = rx.recv().unwrap();
        assert!(
            matches!(evt2, PtyLifecycleEvent::OutputWhileDetached { pty_id } if pty_id == "pty-2")
        );
    }

    #[test]
    fn lifecycle_tx_is_clone() {
        let (tx, rx) = channel();
        let tx2 = tx.clone();

        tx.send(PtyLifecycleEvent::BellWhileDetached {
            pty_id: "pty-1".to_string(),
        })
        .unwrap();

        tx2.send(PtyLifecycleEvent::BellWhileDetached {
            pty_id: "pty-2".to_string(),
        })
        .unwrap();

        drop(tx);
        drop(tx2);

        // Should receive both events
        let mut ids = vec![];
        while let Ok(evt) = rx.recv() {
            if let PtyLifecycleEvent::BellWhileDetached { pty_id } = evt {
                ids.push(pty_id);
            }
        }
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"pty-1".to_string()));
        assert!(ids.contains(&"pty-2".to_string()));
    }
}
