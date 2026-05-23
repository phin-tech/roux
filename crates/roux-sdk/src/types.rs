use roux_core::PtyInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub kind: String,
    pub pid: u32,
    pub socket: String,
    #[serde(default)]
    pub log_path: Option<String>,
    pub started_at_ms: u64,
    pub uptime_ms: u64,
    pub session_count: usize,
    pub project_count: usize,
    #[serde(default)]
    pub watch_count: usize,
    #[serde(default)]
    pub process_count: usize,
    #[serde(default)]
    pub pty_count: usize,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PtyKind {
    Shell,
    Task,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyRecord {
    pub id: String,
    pub kind: PtyKind,
    pub command: Option<String>,
    pub working_dir: String,
    pub started_at_ms: u64,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub generation: u64,
    pub retained_output_bytes: usize,
    pub output_truncated: bool,
    pub cols: u16,
    pub rows: u16,
    pub info: PtyInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtySnapshot {
    pub record: PtyRecord,
    pub output: String,
    pub output_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum PtyAttachFrame {
    #[serde(rename = "ready")]
    Ready {
        id: String,
        record: Box<PtyRecord>,
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
