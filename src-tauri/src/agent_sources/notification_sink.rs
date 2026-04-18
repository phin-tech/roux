//! Production `NotificationSink` implementation backed by Tauri.
//!
//! - `push_attention`: spawns an async task to look up the Roux session
//!   matching the hook's `cwd`, humanize the tool-input payload, and
//!   push a `NotificationRequest` onto the `NotificationManager`.
//! - `dismiss_attention`: synchronously removes the notification
//!   carrying the matching dedup key and, when the
//!   `autoClearAttentionState` setting is on, emits an
//!   `agent-attention-cleared` Tauri event so the frontend can clear
//!   the pane's stale `permissionInfo`.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use roux_core::agent_fsm::AttentionKey;
use roux_core::{ActionKind, NotificationAction, NotificationLevel, NotificationRequest, NotificationSource};

use crate::agent_registry::{EventContext, NotificationSink};
use crate::agent_sources::humanize::{humanize_attention, session_label};
use crate::state::AppState;

pub const ATTENTION_CLEARED_EVENT: &str = "agent-attention-cleared";

/// Payload for the `agent-attention-cleared` event. Emitted when a
/// pane-scoped agent leaves `Attention` and the
/// `autoClearAttentionState` setting is enabled. The frontend listens
/// on this and calls `clearPermissionInfo(paneId)` on its agent state
/// store so the Claude Allow/Deny affordance disappears alongside the
/// notification.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttentionClearedEvent {
    pub pane_id: String,
}

pub struct NotificationManagerSink {
    app: AppHandle,
}

impl NotificationManagerSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl NotificationSink for NotificationManagerSink {
    fn push_attention(&self, key: AttentionKey, context: &EventContext) {
        let app = self.app.clone();
        let context = context.clone();
        let dedup_key = key.to_dedup_key();

        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();

            // Best-effort session match for the notification subtitle /
            // focus action. Prefer the explicit `roux_session_id` the
            // modern hook ships; fall back to cwd matching so legacy
            // (cwd-only) hooks still produce a meaningful subtitle.
            // cwd matching is ambiguous when two sessions share a
            // worktree, so the id path is strictly better when present.
            let matched = match state.session_handle.list().await {
                Ok(sessions) => {
                    let by_id = context
                        .roux_session_id
                        .as_deref()
                        .and_then(|sid| sessions.iter().find(|s| s.id == sid).cloned());
                    by_id.or_else(|| {
                        sessions.into_iter().find(|s| {
                            s.worktree_path == context.cwd || s.repo_root == context.cwd
                        })
                    })
                }
                Err(_) => None,
            };
            let session_id = matched.as_ref().map(|s| s.id.clone());
            let session_name = matched.as_ref().map(|s| s.name.clone());

            let (title, body) = humanize_attention(
                context.tool_name.as_deref(),
                context.tool_input.as_ref(),
                context.message.as_deref(),
            );
            let subtitle = session_label(session_name.as_deref(), &context.cwd);

            // Prefer pane-level focus when the hook carried a pane id;
            // fall back to session focus for legacy installs.
            let mut actions: Vec<NotificationAction> = Vec::new();
            if let Some(ref pane_id) = context.roux_pane_id {
                actions.push(NotificationAction {
                    id: "focus".into(),
                    label: "Focus pane".into(),
                    kind: ActionKind::FocusPane { pane_id: pane_id.clone() },
                    primary: true,
                });
            } else if let Some(ref sid) = session_id {
                actions.push(NotificationAction {
                    id: "focus".into(),
                    label: "Focus session".into(),
                    kind: ActionKind::FocusSession { session_id: sid.clone() },
                    primary: true,
                });
            }
            actions.push(NotificationAction {
                id: "dismiss".into(),
                label: "Dismiss".into(),
                kind: ActionKind::Dismiss,
                primary: actions.is_empty(),
            });

            state.notification_manager.push(
                NotificationRequest {
                    level: NotificationLevel::Attention,
                    source: NotificationSource::Hook {
                        provider: if context.provider.is_empty() {
                            "claude".to_string()
                        } else {
                            context.provider.clone()
                        },
                    },
                    title,
                    subtitle,
                    body,
                    session_id,
                    actions,
                    dedup_key: Some(dedup_key),
                },
                Some(&app),
            );
        });
    }

    fn dismiss_attention(&self, key: AttentionKey, context: &EventContext) {
        let dedup_key = key.to_dedup_key();
        let state = self.app.state::<AppState>();
        state.notification_manager.remove_by_dedup_key(&dedup_key, Some(&self.app));

        // Also tell the frontend to clear any `permissionInfo` for this
        // pane, gated on the user's rollback setting. Only applicable
        // when we have a pane-scoped identity — legacy cwd/session
        // fallbacks never drove pane-level state, so there's nothing
        // to clear on the frontend side.
        if let AttentionKey::Pane(pane_id) = &key {
            let auto_clear = state
                .settings
                .lock()
                .map(|g| g.auto_clear_attention_state)
                .unwrap_or(true);
            if auto_clear {
                let _ = self.app.emit(
                    ATTENTION_CLEARED_EVENT,
                    AttentionClearedEvent { pane_id: pane_id.clone() },
                );
            }
        }

        // `context` is accepted for future symmetry / source parity but
        // nothing in the dismissal path currently needs cwd / tool info.
        let _ = context;
    }
}
