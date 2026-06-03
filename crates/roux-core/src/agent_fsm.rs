//! Per-agent lifecycle state machine.
//!
//! Pure state transducer: `(state, event) -> (state, effects)`. No I/O, no
//! Tauri, no transport knowledge. Sources (file watcher, session lifecycle,
//! future socket / PTY heuristics) translate their own events into
//! `AgentEvent` and hand them to this FSM via a registry. Effects are
//! returned as a `Vec<AgentEffect>` for the caller to dispatch.
//!
//! The immediate user-visible consequence of this FSM is the
//! auto-dismissal of "attention" notifications: when an agent leaves the
//! `Attention` state (user answered, crashed, or pane closed), the FSM
//! emits `AgentEffect::DismissAttention` so the notification store can
//! drop the stale entry. Before this module existed, attention was a
//! one-way entry with no transition awareness.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Observable lifecycle state of a single agent instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    Idle,
    Generating,
    Attention,
    Disconnected,
}

/// Routing key for an agent. The most-specific field present wins for
/// equality, hashing, and `AttentionKey` derivation so two events that
/// share a `pane_id` route to the same FSM instance regardless of which
/// other fields they happen to carry.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentity {
    pub pane_id: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<PathBuf>,
}

/// Mapped hook status. Kept narrow on purpose — string parsing lives in
/// the source adapter, not in the FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedStatus {
    Idle,
    Generating,
    Attention,
}

/// Transport-agnostic input to the FSM.
///
/// The FSM deliberately does **not** carry per-event rich context (tool
/// names, cwd, message, etc.). Sources feed events at this granularity;
/// callers that need to build rich notifications or emit frontend events
/// carry that context separately alongside the event (see
/// `AgentInput`/`EventContext` in the `agent_registry` module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEvent {
    HookStatus(MappedStatus),
    StatusFileRemoved,
    SessionEnded,
}

/// Typed replacement for the legacy `"attention:pane:{id}"` dedup strings.
/// Doubles as the canonical routing key for `AgentIdentity`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AttentionKey {
    Pane(String),
    Session(String),
    Cwd(PathBuf),
}

impl AttentionKey {
    /// Render to the legacy dedup-key string so notifications pushed by
    /// older code paths remain dismissible by the FSM.
    pub fn to_dedup_key(&self) -> String {
        match self {
            AttentionKey::Pane(id) => format!("attention:pane:{id}"),
            AttentionKey::Session(id) => format!("attention:session:{id}"),
            AttentionKey::Cwd(path) => format!("attention:cwd:{}", path.display()),
        }
    }
}

/// Side-effects the FSM asks its caller to perform after a transition.
/// Effects carry only FSM-derived data; any external context needed to
/// service them (session lookup, tool-input humanization, etc.) travels
/// through the surrounding `AgentInput` rather than through the effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEffect {
    PushAttention { key: AttentionKey },
    DismissAttention { key: AttentionKey },
    StateChanged { from: AgentState, to: AgentState },
}

impl AgentIdentity {
    /// Most-specific-field-wins: pane > session > cwd. `None` only when the
    /// identity is completely empty (should never happen in practice since
    /// a FSM instance is always constructed with at least one populated
    /// field).
    pub fn to_attention_key(&self) -> Option<AttentionKey> {
        if let Some(pane) = &self.pane_id {
            return Some(AttentionKey::Pane(pane.clone()));
        }
        if let Some(session) = &self.session_id {
            return Some(AttentionKey::Session(session.clone()));
        }
        if let Some(cwd) = &self.cwd {
            return Some(AttentionKey::Cwd(cwd.clone()));
        }
        None
    }
}

/// Routing equality: two identities that canonicalize to the same
/// `AttentionKey` are equal, so HashMap lookup routes events with matching
/// pane/session/cwd precedence to the same FSM instance regardless of
/// which less-specific fields the caller happened to fill in. Two
/// all-empty identities are also equal (both canonicalize to `None`).
impl PartialEq for AgentIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.to_attention_key() == other.to_attention_key()
    }
}
impl Eq for AgentIdentity {}

impl Hash for AgentIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_attention_key().hash(state);
    }
}

pub struct AgentFsm {
    state: AgentState,
    identity: AgentIdentity,
}

impl AgentFsm {
    pub fn new(identity: AgentIdentity) -> Self {
        Self { state: AgentState::Idle, identity }
    }

    pub fn state(&self) -> AgentState {
        self.state
    }

    pub fn identity(&self) -> &AgentIdentity {
        &self.identity
    }

    pub fn handle(&mut self, event: AgentEvent) -> Vec<AgentEffect> {
        let from = self.state;
        let to = match event {
            AgentEvent::HookStatus(MappedStatus::Idle) => AgentState::Idle,
            AgentEvent::HookStatus(MappedStatus::Generating) => AgentState::Generating,
            AgentEvent::HookStatus(MappedStatus::Attention) => AgentState::Attention,
            AgentEvent::StatusFileRemoved | AgentEvent::SessionEnded => AgentState::Disconnected,
        };

        let mut effects = Vec::new();

        if from == AgentState::Attention && to != AgentState::Attention {
            if let Some(key) = self.identity.to_attention_key() {
                effects.push(AgentEffect::DismissAttention { key });
            }
        }

        if from != AgentState::Attention && to == AgentState::Attention {
            if let Some(key) = self.identity.to_attention_key() {
                effects.push(AgentEffect::PushAttention { key });
            }
        }

        if from != to {
            effects.push(AgentEffect::StateChanged { from, to });
        }

        self.state = to;
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_cwd(cwd: &str) -> AgentIdentity {
        AgentIdentity { pane_id: None, session_id: None, cwd: Some(PathBuf::from(cwd)) }
    }

    fn identity_pane(pane: &str) -> AgentIdentity {
        AgentIdentity { pane_id: Some(pane.into()), session_id: None, cwd: None }
    }

    fn hook(mapped: MappedStatus) -> AgentEvent {
        AgentEvent::HookStatus(mapped)
    }

    #[test]
    fn new_fsm_starts_in_idle() {
        let fsm = AgentFsm::new(identity_cwd("/tmp/x"));
        assert_eq!(fsm.state(), AgentState::Idle);
    }

    #[test]
    fn attention_key_pane_renders_legacy_string() {
        let key = AttentionKey::Pane("p-123".into());
        assert_eq!(key.to_dedup_key(), "attention:pane:p-123");
    }

    #[test]
    fn attention_key_session_renders_legacy_string() {
        let key = AttentionKey::Session("s-abc".into());
        assert_eq!(key.to_dedup_key(), "attention:session:s-abc");
    }

    #[test]
    fn attention_key_cwd_renders_legacy_string() {
        let key = AttentionKey::Cwd(PathBuf::from("/home/me/project"));
        assert_eq!(key.to_dedup_key(), "attention:cwd:/home/me/project");
    }

    #[test]
    fn idle_to_generating_emits_state_changed_only() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        let effects = fsm.handle(hook(MappedStatus::Generating));
        assert_eq!(fsm.state(), AgentState::Generating);
        assert_eq!(
            effects,
            vec![AgentEffect::StateChanged { from: AgentState::Idle, to: AgentState::Generating }]
        );
    }

    #[test]
    fn generating_to_attention_emits_push_and_state_changed() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Generating));
        let effects = fsm.handle(hook(MappedStatus::Attention));
        assert_eq!(fsm.state(), AgentState::Attention);
        assert_eq!(
            effects,
            vec![
                AgentEffect::PushAttention { key: AttentionKey::Pane("p-1".into()) },
                AgentEffect::StateChanged {
                    from: AgentState::Generating,
                    to: AgentState::Attention
                }
            ]
        );
    }

    /// Bug fix: when the user answers and the agent resumes generating, the
    /// FSM must emit DismissAttention so the stale notification is cleared.
    #[test]
    fn attention_to_generating_dismisses_notification() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Attention));
        let effects = fsm.handle(hook(MappedStatus::Generating));
        assert_eq!(fsm.state(), AgentState::Generating);
        assert_eq!(
            effects,
            vec![
                AgentEffect::DismissAttention { key: AttentionKey::Pane("p-1".into()) },
                AgentEffect::StateChanged {
                    from: AgentState::Attention,
                    to: AgentState::Generating
                }
            ]
        );
    }

    /// Bug fix: when the agent goes idle (e.g., Stop hook fires after the
    /// answer), the FSM must also dismiss the attention notification.
    #[test]
    fn attention_to_idle_dismisses_notification() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Attention));
        let effects = fsm.handle(hook(MappedStatus::Idle));
        assert_eq!(fsm.state(), AgentState::Idle);
        assert_eq!(
            effects,
            vec![
                AgentEffect::DismissAttention { key: AttentionKey::Pane("p-1".into()) },
                AgentEffect::StateChanged { from: AgentState::Attention, to: AgentState::Idle }
            ]
        );
    }

    /// Abandonment: hook status file deleted while agent was waiting. The
    /// attention notification should still dismiss even though there's no
    /// new hook payload to trigger it.
    #[test]
    fn attention_then_status_file_removed_dismisses_notification() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Attention));
        let effects = fsm.handle(AgentEvent::StatusFileRemoved);
        assert_eq!(fsm.state(), AgentState::Disconnected);
        assert_eq!(
            effects,
            vec![
                AgentEffect::DismissAttention { key: AttentionKey::Pane("p-1".into()) },
                AgentEffect::StateChanged {
                    from: AgentState::Attention,
                    to: AgentState::Disconnected
                }
            ]
        );
    }

    /// Abandonment: PTY died / session destroyed while agent was waiting
    /// (e.g. Ctrl-C). Notification still dismisses.
    #[test]
    fn attention_then_session_ended_dismisses_notification() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Attention));
        let effects = fsm.handle(AgentEvent::SessionEnded);
        assert_eq!(fsm.state(), AgentState::Disconnected);
        assert_eq!(
            effects,
            vec![
                AgentEffect::DismissAttention { key: AttentionKey::Pane("p-1".into()) },
                AgentEffect::StateChanged {
                    from: AgentState::Attention,
                    to: AgentState::Disconnected
                }
            ]
        );
    }

    /// Non-attention abandonment must not spuriously dismiss — there's no
    /// notification to dismiss. Only StateChanged should fire.
    #[test]
    fn generating_then_status_file_removed_emits_state_changed_only() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Generating));
        let effects = fsm.handle(AgentEvent::StatusFileRemoved);
        assert_eq!(fsm.state(), AgentState::Disconnected);
        assert_eq!(
            effects,
            vec![AgentEffect::StateChanged {
                from: AgentState::Generating,
                to: AgentState::Disconnected
            }]
        );
    }

    #[test]
    fn generating_to_generating_emits_nothing() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Generating));
        let effects = fsm.handle(hook(MappedStatus::Generating));
        assert_eq!(fsm.state(), AgentState::Generating);
        assert!(effects.is_empty(), "expected no churn, got {effects:?}");
    }

    #[test]
    fn idle_to_idle_emits_nothing() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        let effects = fsm.handle(hook(MappedStatus::Idle));
        assert_eq!(fsm.state(), AgentState::Idle);
        assert!(effects.is_empty(), "expected no churn, got {effects:?}");
    }

    /// Duplicate attention events (e.g. the agent re-emits the same hook
    /// every few seconds while waiting) must not re-push the notification
    /// and must not move state. Suppressing avoids notification churn in
    /// the sidebar.
    #[test]
    fn attention_to_attention_suppresses_duplicate_push() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Attention));
        let effects = fsm.handle(hook(MappedStatus::Attention));
        assert_eq!(fsm.state(), AgentState::Attention);
        assert!(
            effects.is_empty(),
            "duplicate attention event must not re-emit PushAttention, got {effects:?}"
        );
    }

    #[test]
    fn identity_to_attention_key_prefers_pane_over_session_and_cwd() {
        let id = AgentIdentity {
            pane_id: Some("p-1".into()),
            session_id: Some("s-1".into()),
            cwd: Some(PathBuf::from("/tmp/x")),
        };
        assert_eq!(id.to_attention_key(), Some(AttentionKey::Pane("p-1".into())));
    }

    #[test]
    fn identity_to_attention_key_falls_back_to_session_over_cwd() {
        let id = AgentIdentity {
            pane_id: None,
            session_id: Some("s-1".into()),
            cwd: Some(PathBuf::from("/tmp/x")),
        };
        assert_eq!(id.to_attention_key(), Some(AttentionKey::Session("s-1".into())));
    }

    #[test]
    fn identity_to_attention_key_uses_cwd_when_nothing_else() {
        let id =
            AgentIdentity { pane_id: None, session_id: None, cwd: Some(PathBuf::from("/tmp/x")) };
        assert_eq!(id.to_attention_key(), Some(AttentionKey::Cwd(PathBuf::from("/tmp/x"))));
    }

    #[test]
    fn identity_with_no_fields_yields_no_attention_key() {
        let id = AgentIdentity { pane_id: None, session_id: None, cwd: None };
        assert_eq!(id.to_attention_key(), None);
    }

    /// Registry routing correctness: two `AgentIdentity` values that share
    /// the same *canonical* key (pane > session > cwd) must hash and
    /// compare equal, so they route to the same FSM instance even when
    /// less-specific fields diverge.
    #[test]
    fn identities_sharing_pane_id_are_equal_and_hash_same() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let a = AgentIdentity {
            pane_id: Some("p-1".into()),
            session_id: Some("s-1".into()),
            cwd: None,
        };
        let b = AgentIdentity {
            pane_id: Some("p-1".into()),
            session_id: None,
            cwd: Some(PathBuf::from("/tmp/different")),
        };
        assert_eq!(a, b, "identities sharing pane_id must compare equal");

        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish(), "hashes must agree for equal identities");
    }

    #[test]
    fn identities_with_different_panes_are_not_equal() {
        let a = AgentIdentity { pane_id: Some("p-1".into()), session_id: None, cwd: None };
        let b = AgentIdentity { pane_id: Some("p-2".into()), session_id: None, cwd: None };
        assert_ne!(a, b);
    }

    /// Re-entry: after a disconnect, a fresh hook brings the agent back.
    #[test]
    fn disconnected_to_generating_on_fresh_hook() {
        let mut fsm = AgentFsm::new(identity_pane("p-1"));
        fsm.handle(hook(MappedStatus::Generating));
        fsm.handle(AgentEvent::StatusFileRemoved);
        assert_eq!(fsm.state(), AgentState::Disconnected);

        let effects = fsm.handle(hook(MappedStatus::Generating));
        assert_eq!(fsm.state(), AgentState::Generating);
        assert_eq!(
            effects,
            vec![AgentEffect::StateChanged {
                from: AgentState::Disconnected,
                to: AgentState::Generating
            }]
        );
    }
}
