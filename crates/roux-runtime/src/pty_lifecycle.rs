use std::sync::mpsc;

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
pub type LifecycleTx<Message> = mpsc::Sender<Message>;

/// Receiver half of the lifecycle bus. Owned by the bus handler thread.
pub type LifecycleRx<Message> = mpsc::Receiver<Message>;

/// Create a new lifecycle bus channel pair.
pub fn channel<Message>() -> (LifecycleTx<Message>, LifecycleRx<Message>) {
    mpsc::channel()
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
        let (tx, rx) = channel::<PtyLifecycleEvent>();

        tx.send(PtyLifecycleEvent::Exited {
            pty_id: "pty-1".to_string(),
            session_id: Some("session-1".to_string()),
            code: Some(0),
            reason: ExitReason::Exit,
            generation: 1,
        })
        .unwrap();

        tx.send(PtyLifecycleEvent::OutputWhileDetached { pty_id: "pty-2".to_string() })
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
        let (tx, rx) = channel::<PtyLifecycleEvent>();
        let tx2 = tx.clone();

        tx.send(PtyLifecycleEvent::BellWhileDetached { pty_id: "pty-1".to_string() })
            .unwrap();

        tx2.send(PtyLifecycleEvent::BellWhileDetached { pty_id: "pty-2".to_string() })
            .unwrap();

        drop(tx);
        drop(tx2);

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
