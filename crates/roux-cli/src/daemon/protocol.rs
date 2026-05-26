use serde::{Deserialize, Serialize};
use serde_json::Value;

use roux_core::{AliasEvent, BusSubscriptionEvent, MailboxEvent, SessionStatusEvent};

#[derive(Debug, Deserialize)]
pub(super) struct Request {
    pub(super) command: String,
    pub(super) session_id: Option<String>,
    #[allow(dead_code)]
    pub(super) pane_id: Option<String>,
    #[allow(dead_code)]
    pub(super) auth_token: Option<String>,
    #[serde(default)]
    pub(super) args: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct Response {
    pub(super) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum PtyAttachFrame {
    #[serde(rename = "ready")]
    Ready {
        id: String,
        record: Box<roux_runtime::pty_service::PtyRecord>,
        #[serde(rename = "replayOffset")]
        replay_offset: u64,
        #[serde(rename = "replayBytes")]
        replay_bytes: Vec<u8>,
    },
    #[serde(rename = "output")]
    Output { offset: u64, bytes: Vec<u8> },
    #[serde(rename = "exit")]
    Exit { code: Option<i32>, generation: u64 },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum WatchEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "update")]
    Update { event: Box<roux_core::WatchUpdateEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum AliasEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: AliasEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum MailboxEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: Box<MailboxEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum SubscriptionEventFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: BusSubscriptionEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(super) enum SessionEventFrame {
    Ready,
    Event { event: SessionStatusEvent },
    Warning { message: String },
    Error { error: String },
}

impl Response {
    pub(super) fn success(data: Value) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }

    pub(super) fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }
}
