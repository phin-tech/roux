use std::sync::mpsc;

use crate::pty_session::PtySessionMetadata;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyMetadataCommand {
    Detach { pty_id: String, since_ms: u64 },
    AttachToPane { pty_id: String, pane_id: String },
    MarkRead { pty_id: String },
    SetUnreadOutput { pty_id: String, value: bool },
    SetBellPending { pty_id: String, value: bool },
    SetName { pty_id: String, name: Option<String> },
    MarkExitedIfGenerationMatches {
        pty_id: String,
        generation: u64,
        code: Option<i32>,
        at_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyMetadataCommandResult {
    Applied,
    Missing,
    StaleGeneration,
}

impl PtyMetadataCommand {
    pub fn pty_id(&self) -> &str {
        match self {
            Self::Detach { pty_id, .. }
            | Self::AttachToPane { pty_id, .. }
            | Self::MarkRead { pty_id }
            | Self::SetUnreadOutput { pty_id, .. }
            | Self::SetBellPending { pty_id, .. }
            | Self::SetName { pty_id, .. }
            | Self::MarkExitedIfGenerationMatches { pty_id, .. } => pty_id,
        }
    }
}

pub fn apply_metadata_command(
    metadata: &mut PtySessionMetadata,
    session_generation: u64,
    command: &PtyMetadataCommand,
) -> PtyMetadataCommandResult {
    match command {
        PtyMetadataCommand::Detach { since_ms, .. } => {
            metadata.detach(*since_ms);
            PtyMetadataCommandResult::Applied
        }
        PtyMetadataCommand::AttachToPane { pane_id, .. } => {
            metadata.attach_to_pane(pane_id);
            PtyMetadataCommandResult::Applied
        }
        PtyMetadataCommand::MarkRead { .. } => {
            metadata.mark_read();
            PtyMetadataCommandResult::Applied
        }
        PtyMetadataCommand::SetUnreadOutput { value, .. } => {
            metadata.set_unread_output(*value);
            PtyMetadataCommandResult::Applied
        }
        PtyMetadataCommand::SetBellPending { value, .. } => {
            metadata.set_bell_pending(*value);
            PtyMetadataCommandResult::Applied
        }
        PtyMetadataCommand::SetName { name, .. } => {
            metadata.set_name(name.as_deref());
            PtyMetadataCommandResult::Applied
        }
        PtyMetadataCommand::MarkExitedIfGenerationMatches {
            generation,
            code,
            at_ms,
            ..
        } => {
            if *generation != session_generation {
                return PtyMetadataCommandResult::StaleGeneration;
            }
            metadata.mark_exited(*code, *at_ms);
            PtyMetadataCommandResult::Applied
        }
    }
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
    use crate::pty_session::{PtyExitInfo, PtySessionMetadata, PtySessionMetadataInputs};
    use roux_core::{PtyRole, PtyStatus};

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

    fn metadata() -> PtySessionMetadata {
        PtySessionMetadata::new(PtySessionMetadataInputs {
            role: PtyRole::Secondary,
            pane_id: Some("pane-a"),
            detached_since_ms: 1,
            session_id: Some("session-a"),
            working_dir: Some("/repo"),
            profile: Some("plain-shell"),
            last_size: (80, 24),
        })
    }

    #[test]
    fn metadata_command_exposes_target_pty_id() {
        let command = PtyMetadataCommand::AttachToPane {
            pty_id: "pty-a".to_string(),
            pane_id: "pane-b".to_string(),
        };

        assert_eq!(command.pty_id(), "pty-a");
    }

    #[test]
    fn apply_metadata_command_handles_attach_detach_and_flags() {
        let mut metadata = metadata();

        assert_eq!(
            apply_metadata_command(
                &mut metadata,
                7,
                &PtyMetadataCommand::SetUnreadOutput {
                    pty_id: "pty-a".to_string(),
                    value: true,
                },
            ),
            PtyMetadataCommandResult::Applied
        );
        assert!(metadata.unread_output);

        apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::SetBellPending {
                pty_id: "pty-a".to_string(),
                value: true,
            },
        );
        assert!(metadata.bell_pending);

        apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::AttachToPane {
                pty_id: "pty-a".to_string(),
                pane_id: "pane-b".to_string(),
            },
        );
        assert!(matches!(
            metadata.status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-b"
        ));
        assert!(!metadata.unread_output);
        assert!(!metadata.bell_pending);

        apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::Detach { pty_id: "pty-a".to_string(), since_ms: 44 },
        );
        assert!(matches!(metadata.status, PtyStatus::RunningDetached { since_ms: 44 }));

        metadata.set_unread_output(true);
        metadata.set_bell_pending(true);
        apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::MarkRead { pty_id: "pty-a".to_string() },
        );
        assert!(!metadata.unread_output);
        assert!(!metadata.bell_pending);
    }

    #[test]
    fn apply_metadata_command_sets_name() {
        let mut metadata = metadata();

        apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::SetName {
                pty_id: "pty-a".to_string(),
                name: Some("build".to_string()),
            },
        );
        assert_eq!(metadata.name.as_deref(), Some("build"));

        apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::SetName { pty_id: "pty-a".to_string(), name: None },
        );
        assert_eq!(metadata.name, None);
    }

    #[test]
    fn apply_metadata_command_marks_matching_generation_exited() {
        let mut metadata = metadata();

        let result = apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::MarkExitedIfGenerationMatches {
                pty_id: "pty-a".to_string(),
                generation: 7,
                code: Some(2),
                at_ms: 99,
            },
        );

        assert_eq!(result, PtyMetadataCommandResult::Applied);
        assert!(matches!(
            metadata.status,
            PtyStatus::Exited { code: Some(2), at_ms: 99 }
        ));
        assert_eq!(
            metadata.exit_info,
            Some(PtyExitInfo { code: Some(2), at_ms: 99, was_attached: true })
        );
    }

    #[test]
    fn apply_metadata_command_rejects_stale_exit_generation() {
        let mut metadata = metadata();

        let result = apply_metadata_command(
            &mut metadata,
            7,
            &PtyMetadataCommand::MarkExitedIfGenerationMatches {
                pty_id: "pty-a".to_string(),
                generation: 6,
                code: Some(1),
                at_ms: 99,
            },
        );

        assert_eq!(result, PtyMetadataCommandResult::StaleGeneration);
        assert!(matches!(metadata.status, PtyStatus::RunningAttached { .. }));
        assert_eq!(metadata.exit_info, None);
    }
}
