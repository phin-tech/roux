//! Per-agent FSM registry and effect dispatch.
//!
//! Owns `AgentFsm` instances keyed by `AgentIdentity`. Sources (file
//! watcher, session lifecycle, future socket / PTY heuristics) push
//! `AgentInput` values through a channel; the registry's worker thread
//! routes each input to the matching FSM, runs the transition, and
//! dispatches resulting effects to a `NotificationSink`. The registry
//! itself is synchronous and transport-agnostic — `tokio` / Tauri
//! spawning lives entirely inside the sink impls.
//!
//! See `crates/roux-core/src/agent_fsm.rs` for the pure state machine.

use std::collections::HashMap;
use std::sync::{mpsc, Arc};
use std::thread;

use serde_json::Value;

use roux_core::agent_fsm::{AgentEffect, AgentEvent, AgentFsm, AgentIdentity, AttentionKey};

/// Per-event context that travels alongside `AgentEvent` but is never
/// inspected by the FSM. Sinks use it to build rich notifications
/// (session lookup via `cwd`, tool-input humanization, etc.) without the
/// FSM having to model every per-tool field.
#[derive(Debug, Clone, Default)]
pub struct EventContext {
    pub cwd: String,
    pub provider: String,
    pub roux_session_id: Option<String>,
    pub roux_pane_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub message: Option<String>,
}

/// A single unit of work for the registry. Sources produce these and
/// send them through the shared channel.
#[derive(Debug, Clone)]
pub struct AgentInput {
    pub identity: AgentIdentity,
    pub event: AgentEvent,
    pub context: EventContext,
}

/// Multiplex over the shared channel: hook-driven `Input` events
/// routed to a specific FSM by identity, and lifecycle broadcasts
/// that fan out to every FSM belonging to a given session.
#[derive(Debug, Clone)]
pub enum RegistryMessage {
    Input(Box<AgentInput>),
    /// A session (PTY) has exited. Dispatch `SessionEnded` to every
    /// FSM whose identity records this session id — its notifications
    /// get auto-dismissed without the hook needing to fire any more
    /// status files (which it won't, because the agent is gone).
    SessionEnded {
        session_id: String,
    },
}

/// Effect-dispatch boundary. Trait-object so production and test code
/// can plug different implementations without the registry knowing
/// about Tauri.
pub trait NotificationSink: Send + Sync + 'static {
    /// Called when an agent enters `Attention`. The `context` carries
    /// everything needed to build the notification (session lookup via
    /// cwd, tool-input for humanization, pane id for focus actions).
    fn push_attention(&self, key: AttentionKey, context: &EventContext);

    /// Called when an agent leaves `Attention` for any reason (user
    /// answered, status file removed, session ended). Idempotent: safe
    /// to call with a key that has no live notification. `context`
    /// carries identity info sinks may need for follow-up signals
    /// (e.g., emitting `agent-attention-cleared` to the frontend).
    fn dismiss_attention(&self, key: AttentionKey, context: &EventContext);
}

pub struct AgentRegistry {
    fsms: HashMap<AgentIdentity, AgentFsm>,
    sink: Arc<dyn NotificationSink>,
}

impl AgentRegistry {
    pub fn new(sink: Arc<dyn NotificationSink>) -> Self {
        Self { fsms: HashMap::new(), sink }
    }

    /// Route one input to the matching FSM (creating it lazily), run
    /// its transition, and dispatch the resulting effects.
    pub fn dispatch(&mut self, input: AgentInput) {
        let fsm = self
            .fsms
            .entry(input.identity.clone())
            .or_insert_with(|| AgentFsm::new(input.identity.clone()));
        let effects = fsm.handle(input.event);
        for effect in effects {
            match effect {
                AgentEffect::PushAttention { key } => {
                    self.sink.push_attention(key, &input.context);
                }
                AgentEffect::DismissAttention { key } => {
                    self.sink.dismiss_attention(key, &input.context);
                }
                AgentEffect::StateChanged { .. } => {
                    // Reserved for future subscribers (e.g. an
                    // `agent-state-changed` Tauri event consumed by the
                    // frontend to clear `permissionInfo` when Attention
                    // exits — see Phase 5b of the plan).
                }
            }
        }
    }

    /// Fan out `SessionEnded` to every FSM whose identity records this
    /// session id. Uses a default `EventContext` because abandonment
    /// paths don't carry fresh hook data. Only `DismissAttention`
    /// effects need servicing here — `StateChanged` is no-op until a
    /// frontend subscriber is added.
    pub fn on_session_ended(&mut self, session_id: &str) {
        let identities: Vec<AgentIdentity> = self
            .fsms
            .keys()
            .filter(|id| id.session_id.as_deref() == Some(session_id))
            .cloned()
            .collect();
        let context = EventContext::default();
        for identity in identities {
            if let Some(fsm) = self.fsms.get_mut(&identity) {
                for effect in fsm.handle(AgentEvent::SessionEnded) {
                    match effect {
                        AgentEffect::DismissAttention { key } => {
                            self.sink.dismiss_attention(key, &context);
                        }
                        AgentEffect::PushAttention { .. } => {
                            // Unreachable under SessionEnded: transitioning
                            // *into* Attention requires a HookStatus event.
                        }
                        AgentEffect::StateChanged { .. } => {}
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub fn fsm_count(&self) -> usize {
        self.fsms.len()
    }
}

/// Spawn a worker thread that drains `rx` into a fresh `AgentRegistry`.
/// Returns the join handle so callers can wait on shutdown. The thread
/// exits cleanly when all senders are dropped.
pub fn spawn_worker(
    rx: mpsc::Receiver<RegistryMessage>,
    sink: Arc<dyn NotificationSink>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut registry = AgentRegistry::new(sink);
        while let Ok(msg) = rx.recv() {
            match msg {
                RegistryMessage::Input(input) => registry.dispatch(*input),
                RegistryMessage::SessionEnded { session_id } => {
                    registry.on_session_ended(&session_id);
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::agent_fsm::MappedStatus;
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink {
        pushes: Mutex<Vec<(AttentionKey, EventContext)>>,
        dismisses: Mutex<Vec<(AttentionKey, EventContext)>>,
    }

    impl RecordingSink {
        fn pushes(&self) -> Vec<(AttentionKey, EventContext)> {
            self.pushes.lock().unwrap().clone()
        }
        fn dismisses(&self) -> Vec<AttentionKey> {
            self.dismisses.lock().unwrap().iter().map(|(k, _)| k.clone()).collect()
        }
    }

    impl NotificationSink for RecordingSink {
        fn push_attention(&self, key: AttentionKey, context: &EventContext) {
            self.pushes.lock().unwrap().push((key, context.clone()));
        }
        fn dismiss_attention(&self, key: AttentionKey, context: &EventContext) {
            self.dismisses.lock().unwrap().push((key, context.clone()));
        }
    }

    fn pane_identity(pane: &str) -> AgentIdentity {
        AgentIdentity { pane_id: Some(pane.into()), session_id: None, cwd: None }
    }

    fn input(identity: AgentIdentity, event: AgentEvent, cwd: &str) -> AgentInput {
        AgentInput {
            identity,
            event,
            context: EventContext { cwd: cwd.into(), ..EventContext::default() },
        }
    }

    #[test]
    fn dispatch_creates_fsm_lazily_on_first_input() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());
        assert_eq!(registry.fsm_count(), 0);
        registry.dispatch(input(
            pane_identity("p-1"),
            AgentEvent::HookStatus(MappedStatus::Generating),
            "/repo",
        ));
        assert_eq!(registry.fsm_count(), 1);
    }

    #[test]
    fn dispatch_routes_same_identity_to_same_fsm() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());

        registry.dispatch(input(
            pane_identity("p-1"),
            AgentEvent::HookStatus(MappedStatus::Generating),
            "/repo",
        ));
        registry.dispatch(input(
            pane_identity("p-1"),
            AgentEvent::HookStatus(MappedStatus::Attention),
            "/repo",
        ));
        registry.dispatch(input(
            pane_identity("p-1"),
            AgentEvent::HookStatus(MappedStatus::Idle),
            "/repo",
        ));

        assert_eq!(registry.fsm_count(), 1);
        assert_eq!(sink.pushes().len(), 1, "one attention entered");
        assert_eq!(sink.dismisses().len(), 1, "one attention exited");
    }

    #[test]
    fn dispatch_routes_different_identities_to_different_fsms() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());

        registry.dispatch(input(
            pane_identity("p-1"),
            AgentEvent::HookStatus(MappedStatus::Attention),
            "/repo",
        ));
        registry.dispatch(input(
            pane_identity("p-2"),
            AgentEvent::HookStatus(MappedStatus::Attention),
            "/repo",
        ));
        assert_eq!(registry.fsm_count(), 2);
        assert_eq!(sink.pushes().len(), 2);
        let keys: Vec<_> = sink.pushes().into_iter().map(|(k, _)| k).collect();
        assert!(keys.contains(&AttentionKey::Pane("p-1".into())));
        assert!(keys.contains(&AttentionKey::Pane("p-2".into())));
    }

    /// End-to-end registry-level bug fix: Claude enters attention, user
    /// answers, Claude resumes — sink sees push then dismiss with
    /// matching keys.
    #[test]
    fn bug_fix_user_answers_question_registry_dismisses_notification() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());

        let identity = pane_identity("p-1");

        registry.dispatch(input(
            identity.clone(),
            AgentEvent::HookStatus(MappedStatus::Attention),
            "/repo",
        ));
        registry.dispatch(input(
            identity,
            AgentEvent::HookStatus(MappedStatus::Generating),
            "/repo",
        ));

        let pushes = sink.pushes();
        let dismisses = sink.dismisses();
        assert_eq!(pushes.len(), 1);
        assert_eq!(pushes[0].0, AttentionKey::Pane("p-1".into()));
        assert_eq!(dismisses.len(), 1);
        assert_eq!(dismisses[0], AttentionKey::Pane("p-1".into()));
    }

    /// Abandonment: PTY dies mid-attention. Registry must dismiss even
    /// though the source event doesn't carry attention context.
    #[test]
    fn session_ended_mid_attention_dismisses_notification() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());

        let identity = pane_identity("p-1");

        registry.dispatch(input(
            identity.clone(),
            AgentEvent::HookStatus(MappedStatus::Attention),
            "/repo",
        ));
        registry.dispatch(input(identity, AgentEvent::SessionEnded, "/repo"));

        assert_eq!(sink.pushes().len(), 1);
        assert_eq!(sink.dismisses().len(), 1);
    }

    #[test]
    fn context_reaches_sink_for_push_attention() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());

        let context = EventContext {
            cwd: "/home/user/repo".into(),
            tool_name: Some("Bash".into()),
            roux_pane_id: Some("p-1".into()),
            ..EventContext::default()
        };

        registry.dispatch(AgentInput {
            identity: pane_identity("p-1"),
            event: AgentEvent::HookStatus(MappedStatus::Attention),
            context: context.clone(),
        });

        let pushes = sink.pushes();
        assert_eq!(pushes.len(), 1);
        assert_eq!(pushes[0].1.cwd, "/home/user/repo");
        assert_eq!(pushes[0].1.tool_name.as_deref(), Some("Bash"));
    }

    #[test]
    fn worker_thread_drains_channel_until_senders_drop() {
        let sink = Arc::new(RecordingSink::default());
        let (tx, rx) = mpsc::channel::<RegistryMessage>();
        let handle = spawn_worker(rx, sink.clone());

        tx.send(RegistryMessage::Input(Box::new(input(
            pane_identity("p-1"),
            AgentEvent::HookStatus(MappedStatus::Attention),
            "/repo",
        ))))
        .unwrap();
        tx.send(RegistryMessage::Input(Box::new(input(
            pane_identity("p-1"),
            AgentEvent::HookStatus(MappedStatus::Idle),
            "/repo",
        ))))
        .unwrap();
        drop(tx);
        handle.join().expect("worker joined");

        assert_eq!(sink.pushes().len(), 1);
        assert_eq!(sink.dismisses().len(), 1);
    }

    /// Session-lifecycle abandonment: Claude crashes mid-question.
    /// The `SessionEnded` broadcast reaches every FSM whose identity
    /// carries the exiting session id (even though they're keyed on
    /// `pane_id`), and dismisses each pane's attention notification.
    #[test]
    fn session_ended_broadcast_dismisses_matching_pane_notifications() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());

        // Two panes in session "s-1", one in "s-2", all currently in Attention.
        fn ident(pane: &str, session: &str) -> AgentIdentity {
            AgentIdentity {
                pane_id: Some(pane.into()),
                session_id: Some(session.into()),
                cwd: None,
            }
        }
        fn ctx() -> EventContext {
            EventContext::default()
        }

        for id in [ident("p-1", "s-1"), ident("p-2", "s-1"), ident("p-3", "s-2")] {
            registry.dispatch(AgentInput {
                identity: id,
                event: AgentEvent::HookStatus(MappedStatus::Attention),
                context: ctx(),
            });
        }
        assert_eq!(sink.pushes().len(), 3);

        // Session s-1 exits; its two panes should dismiss.
        registry.on_session_ended("s-1");
        let dismissed = sink.dismisses();
        assert_eq!(dismissed.len(), 2);
        assert!(dismissed.contains(&AttentionKey::Pane("p-1".into())));
        assert!(dismissed.contains(&AttentionKey::Pane("p-2".into())));
        assert!(
            !dismissed.contains(&AttentionKey::Pane("p-3".into())),
            "s-2's pane must be unaffected"
        );
    }

    #[test]
    fn session_ended_for_unknown_session_is_noop() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());
        registry.on_session_ended("does-not-exist");
        assert!(sink.dismisses().is_empty());
    }

    #[test]
    fn session_ended_via_worker_message() {
        let sink = Arc::new(RecordingSink::default());
        let (tx, rx) = mpsc::channel::<RegistryMessage>();
        let handle = spawn_worker(rx, sink.clone());

        let id = AgentIdentity {
            pane_id: Some("p-1".into()),
            session_id: Some("s-42".into()),
            cwd: None,
        };
        tx.send(RegistryMessage::Input(Box::new(AgentInput {
            identity: id,
            event: AgentEvent::HookStatus(MappedStatus::Attention),
            context: EventContext::default(),
        })))
        .unwrap();
        tx.send(RegistryMessage::SessionEnded { session_id: "s-42".into() }).unwrap();
        drop(tx);
        handle.join().unwrap();

        assert_eq!(sink.pushes().len(), 1);
        assert_eq!(sink.dismisses().len(), 1);
    }

    /// Identity equality uses the canonical key (pane > session > cwd),
    /// so two inputs that share a pane_id route to the same FSM even
    /// if their cwd fields differ. Regression guard for the registry
    /// accidentally keying on a less-stable subset of fields.
    #[test]
    fn identities_sharing_pane_id_route_to_same_fsm_across_cwds() {
        let sink = Arc::new(RecordingSink::default());
        let mut registry = AgentRegistry::new(sink.clone());

        let id_a = AgentIdentity {
            pane_id: Some("p-1".into()),
            session_id: None,
            cwd: Some(PathBuf::from("/one")),
        };
        let id_b = AgentIdentity {
            pane_id: Some("p-1".into()),
            session_id: None,
            cwd: Some(PathBuf::from("/two")),
        };

        registry.dispatch(input(id_a, AgentEvent::HookStatus(MappedStatus::Attention), "/one"));
        registry.dispatch(input(id_b, AgentEvent::HookStatus(MappedStatus::Idle), "/two"));

        assert_eq!(registry.fsm_count(), 1);
        assert_eq!(sink.dismisses().len(), 1, "cwd drift must not hide the dismiss");
    }
}
