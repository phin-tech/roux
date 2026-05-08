//! Tauri commands for the mailbox UI.
//!
//! These commands are intentionally NOT typed via `#[specta::specta]` —
//! `Event::structured` is `serde_json::Value`, which specta can't render
//! as valid TypeScript. The frontend hand-writes its types in
//! `src/lib/types/mailbox.ts` and calls `invoke()` directly, mirroring
//! how `pane_state` commands work.

use roux_core::{Event, EventBuilder, EventKind, ReadState};
use roux_lib::aliases::ProjectFilter;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxPostInput {
    pub to: Option<String>,
    pub topic: Option<String>,
    pub body: String,
    pub subject: Option<String>,
    pub kind: Option<String>,
    pub project_id: Option<String>,
    pub correlation_id: Option<String>,
    pub structured: Option<Value>,
    pub from: Option<String>,
}

fn parse_kind(s: &str) -> Result<EventKind, String> {
    match s {
        "task" => Ok(EventKind::Task),
        "result" => Ok(EventKind::Result),
        "question" => Ok(EventKind::Question),
        "fyi" => Ok(EventKind::Fyi),
        "signal" => Ok(EventKind::Signal),
        other => Err(format!("invalid kind: {other}")),
    }
}

fn project_filter<'a>(project_id: Option<&'a str>, global: bool) -> ProjectFilter<'a> {
    match (project_id, global) {
        (Some(p), _) => ProjectFilter::Exact(Some(p)),
        (None, true) => ProjectFilter::Exact(None),
        (None, false) => ProjectFilter::Any,
    }
}

#[tauri::command]
pub async fn mailbox_list_for_recipient(
    alias: String,
    unread_only: Option<bool>,
    project_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Event>, String> {
    Ok(state.mailbox_manager.list_for_recipient(
        &alias,
        unread_only.unwrap_or(false),
        project_filter(project_id.as_deref(), global.unwrap_or(false)),
    ))
}

#[tauri::command]
pub async fn mailbox_list_for_topic(
    topic: String,
    project_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Event>, String> {
    Ok(state.mailbox_manager.list_for_topic(
        &topic,
        project_filter(project_id.as_deref(), global.unwrap_or(false)),
    ))
}

#[tauri::command]
pub async fn mailbox_list_all(
    project_id: Option<String>,
    global: Option<bool>,
    limit: Option<u32>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Event>, String> {
    Ok(state.mailbox_manager.list_all(
        project_filter(project_id.as_deref(), global.unwrap_or(false)),
        limit.map(|n| n as usize),
    ))
}

#[tauri::command]
pub async fn mailbox_unread_count(
    alias: String,
    project_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<u32, String> {
    Ok(state.mailbox_manager.unread_count(
        &alias,
        project_filter(project_id.as_deref(), global.unwrap_or(false)),
    ) as u32)
}

#[tauri::command]
pub async fn mailbox_get_event(
    event_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<Event>, String> {
    Ok(state.mailbox_manager.get(&event_id))
}

#[tauri::command]
pub async fn mailbox_read_state(
    event_id: String,
    recipient: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<ReadState>, String> {
    Ok(state.mailbox_manager.read_state(&event_id, &recipient))
}

#[tauri::command]
pub async fn mailbox_post(
    input: MailboxPostInput,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Event, String> {
    if input.to.is_none() && input.topic.is_none() {
        return Err("at least one of `to` or `topic` required".into());
    }

    let canonical_to = match input.to.as_deref() {
        Some(s) => Some(roux_core::validate_alias_name(s).map_err(|e| e.to_string())?),
        None => None,
    };
    if let Some(c) = &canonical_to {
        state.alias_manager.ensure(c, input.project_id.clone(), Some(&app));
    }

    let kind = match input.kind.as_deref() {
        Some(s) => parse_kind(s)?,
        None => EventKind::Task,
    };

    let mut builder = EventBuilder::new(input.body).kind(kind);
    if let Some(c) = canonical_to {
        builder = builder.to(c);
    }
    if let Some(t) = input.topic {
        builder = builder.topic(t);
    }
    if let Some(f) = input.from {
        builder = builder.from(f);
    }
    if let Some(p) = input.project_id {
        builder = builder.project_id(p);
    }
    if let Some(s) = input.subject {
        builder = builder.subject(s);
    }
    if let Some(c) = input.correlation_id {
        builder = builder.correlation_id(c);
    }
    if let Some(v) = input.structured {
        if !v.is_null() {
            builder = builder.structured(v);
        }
    }

    state.mailbox_manager.post(builder, Some(&app)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mailbox_mark_read(
    event_id: String,
    recipient: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    Ok(state.mailbox_manager.mark_read(&event_id, &recipient, Some(&app)))
}

#[tauri::command]
pub async fn mailbox_ack(
    event_id: String,
    recipient: String,
    result: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    Ok(state.mailbox_manager.ack(&event_id, &recipient, result, Some(&app)))
}

#[tauri::command]
pub async fn mailbox_clear_read(
    recipient: String,
    project_id: Option<String>,
    global: Option<bool>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<u32, String> {
    Ok(state.mailbox_manager.clear_read(
        &recipient,
        project_filter(project_id.as_deref(), global.unwrap_or(false)),
        Some(&app),
    ) as u32)
}

/// Deliver a mailbox event to the recipient's pane by writing the body
/// (plus a trailing CR) into its PTY. Acks the event with a "delivered
/// via UI" marker so the sender's `mailbox sent` view shows it landed.
///
/// Requires the recipient alias to be currently bound to a pane —
/// "deliver" is a last-mile PTY-typing operation, distinct from posting
/// (which queues durably regardless of binding).
#[tauri::command]
pub async fn mailbox_deliver_to_pane(
    event_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let event = state
        .mailbox_manager
        .get(&event_id)
        .ok_or_else(|| format!("event '{event_id}' not found"))?;

    let recipient = event
        .to
        .as_deref()
        .ok_or_else(|| "event has no `to` recipient — nothing to deliver".to_string())?;

    let alias = state
        .alias_manager
        .get(recipient, event.project_id.as_deref())
        .ok_or_else(|| format!("alias '{recipient}' not found"))?;

    let pane_id = alias.pane_id.clone().ok_or_else(|| {
        format!(
            "alias '{recipient}' has no pane bound; claim from inside a pane or use `roux send` directly"
        )
    })?;

    // Resolve pty_id via the pane handle.
    let pane_records =
        state.pane_handle.list_by_ids(vec![pane_id.clone()]).await.map_err(|e| e.to_string())?;
    let pane = pane_records
        .first()
        .ok_or_else(|| format!("pane '{pane_id}' not found"))?;
    let pty_id = pane.pty_id.clone();

    // Type the body into the pane. Append CR so Claude/Codex see it as
    // submitted input rather than a half-typed line.
    let mut bytes = event.body.clone().into_bytes();
    bytes.push(b'\r');
    state
        .pty_manager
        .write(&pty_id, &bytes)
        .map_err(|e| format!("failed to write to pane: {e}"))?;

    // Ack so the sender's `sent` view shows the delivery.
    state.mailbox_manager.ack(
        &event_id,
        recipient,
        Some(format!("delivered to pane {pane_id}")),
        Some(&app),
    );
    Ok(())
}
