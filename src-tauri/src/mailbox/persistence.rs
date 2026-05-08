//! On-disk persistence for the mailbox event store.
//!
//! Two files under `roux_config_dir()`:
//!
//! - `events.jsonl` — append-only NDJSON. Each row is a JSON object
//!   carrying `schemaVersion` plus the flattened `Event` fields. Rows
//!   with unknown future versions are preserved on disk but skipped at
//!   load time, so a downgrade doesn't lose data.
//! - `read_state.json` — versioned envelope `{ "version": N, "data": [...] }`.
//!   Rewritten in full on mutation (read/ack/clear).
//!
//! Mirrors the alias persistence pattern (`crate::aliases::persistence`).

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use roux_core::{Event, ReadState};
use serde::{Deserialize, Serialize};

const EVENT_SCHEMA_VERSION: u32 = 1;
const READ_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct EventRowV1 {
    #[serde(rename = "schemaVersion", default = "default_schema_version")]
    schema_version: u32,
    #[serde(flatten)]
    event: Event,
}

fn default_schema_version() -> u32 {
    EVENT_SCHEMA_VERSION
}

#[derive(Debug, Serialize, Deserialize)]
struct ReadStateFile {
    version: u32,
    data: Vec<ReadState>,
}

pub fn events_path() -> PathBuf {
    crate::paths::roux_config_dir().join("events.jsonl")
}

pub fn read_state_path() -> PathBuf {
    crate::paths::roux_config_dir().join("read_state.json")
}

pub fn load_events() -> Vec<Event> {
    load_events_from(&events_path())
}

pub fn load_read_state() -> Vec<ReadState> {
    load_read_state_from(&read_state_path())
}

pub fn append_event(event: &Event) -> io::Result<()> {
    append_event_to(&events_path(), event)
}

pub fn save_read_state(states: &[ReadState]) -> io::Result<()> {
    save_read_state_to(&read_state_path(), states)
}

/// Read all events from `path`. Malformed JSON lines are skipped silently
/// (logged via stderr). Rows whose `schemaVersion` is greater than the
/// version this binary understands are skipped at load time but retained
/// on disk by virtue of being append-only.
pub(crate) fn load_events_from(path: &Path) -> Vec<Event> {
    if !path.exists() {
        return Vec::new();
    }
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (lineno, line) in reader.lines().enumerate() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        // First-pass parse to inspect schemaVersion — lets us silently
        // skip future-version rows without a hard parse error.
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[roux] events.jsonl:{}: skipping malformed row: {e}",
                    lineno + 1
                );
                continue;
            }
        };
        let schema = value
            .get("schemaVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(EVENT_SCHEMA_VERSION as u64);
        if schema > EVENT_SCHEMA_VERSION as u64 {
            eprintln!(
                "[roux] events.jsonl:{}: skipping future schemaVersion={schema}",
                lineno + 1
            );
            continue;
        }
        match serde_json::from_value::<EventRowV1>(value) {
            Ok(row) => out.push(row.event),
            Err(e) => {
                eprintln!(
                    "[roux] events.jsonl:{}: row failed to deserialize: {e}",
                    lineno + 1
                );
            }
        }
    }
    out
}

pub(crate) fn append_event_to(path: &Path, event: &Event) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let row = EventRowV1 { schema_version: EVENT_SCHEMA_VERSION, event: event.clone() };
    let json = serde_json::to_string(&row)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

pub(crate) fn load_read_state_from(path: &Path) -> Vec<ReadState> {
    if !path.exists() {
        return Vec::new();
    }
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    if let Ok(file) = serde_json::from_str::<ReadStateFile>(&content) {
        return file.data;
    }
    // Defensive fallback for hand-edited bare arrays.
    serde_json::from_str::<Vec<ReadState>>(&content).unwrap_or_default()
}

pub(crate) fn save_read_state_to(path: &Path, states: &[ReadState]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let envelope = ReadStateFile {
        version: READ_STATE_SCHEMA_VERSION,
        data: states.to_vec(),
    };
    let json = serde_json::to_string_pretty(&envelope)?;
    fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{EventBuilder, EventKind};
    use tempfile::tempdir;

    fn sample_event(id: &str, body: &str) -> Event {
        EventBuilder::new(body)
            .to("reviewer")
            .from("me")
            .kind(EventKind::Task)
            .build_with(id, 1234)
            .unwrap()
    }

    #[test]
    fn append_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_event_to(&path, &sample_event("e1", "first")).unwrap();
        append_event_to(&path, &sample_event("e2", "second")).unwrap();
        let loaded = load_events_from(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "e1");
        assert_eq!(loaded[1].id, "e2");
    }

    #[test]
    fn load_returns_empty_when_file_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        assert!(load_events_from(&path).is_empty());
    }

    #[test]
    fn load_skips_malformed_rows_but_keeps_valid_ones() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_event_to(&path, &sample_event("e1", "good")).unwrap();
        // Inject a garbage line.
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f, "this is not json").unwrap();
        append_event_to(&path, &sample_event("e2", "also good")).unwrap();
        let loaded = load_events_from(&path);
        let ids: Vec<_> = loaded.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["e1", "e2"]);
    }

    #[test]
    fn load_skips_future_schema_versions() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        // Hand-write a future-version row plus a current-version one.
        let future_row = serde_json::json!({
            "schemaVersion": 999,
            "id": "future-1",
            "createdAt": 0,
            "kind": "task",
            "body": "from the future",
        });
        let mut f =
            OpenOptions::new().create(true).append(true).open(&path).unwrap();
        writeln!(f, "{}", future_row).unwrap();
        drop(f);
        append_event_to(&path, &sample_event("now-1", "current")).unwrap();
        let loaded = load_events_from(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "now-1");
    }

    #[test]
    fn load_treats_missing_schema_version_as_v1() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        // A row WITHOUT schemaVersion — covers any pre-versioning leftover.
        let event = sample_event("legacy", "no version");
        let json = serde_json::to_string(&event).unwrap();
        let mut f =
            OpenOptions::new().create(true).append(true).open(&path).unwrap();
        writeln!(f, "{}", json).unwrap();
        let loaded = load_events_from(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "legacy");
    }

    #[test]
    fn append_creates_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("events.jsonl");
        append_event_to(&path, &sample_event("e1", "x")).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn append_writes_schema_version_field() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_event_to(&path, &sample_event("e1", "x")).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(parsed["schemaVersion"], EVENT_SCHEMA_VERSION);
        assert_eq!(parsed["id"], "e1");
    }

    #[test]
    fn read_state_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("read_state.json");
        let mut s1 = ReadState::new("e1", "reviewer");
        s1.read_at = Some(1000);
        let mut s2 = ReadState::new("e2", "builder");
        s2.acked_at = Some(2000);
        s2.ack_result = Some("done".into());
        save_read_state_to(&path, &[s1.clone(), s2.clone()]).unwrap();
        let loaded = load_read_state_from(&path);
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains(&s1));
        assert!(loaded.contains(&s2));
    }

    #[test]
    fn read_state_save_emits_versioned_envelope() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("read_state.json");
        save_read_state_to(&path, &[ReadState::new("e1", "reviewer")]).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["version"], READ_STATE_SCHEMA_VERSION);
        assert!(parsed["data"].is_array());
    }

    #[test]
    fn read_state_load_accepts_bare_array_for_back_compat() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("read_state.json");
        let bare = serde_json::to_string(&vec![ReadState::new("e1", "reviewer")]).unwrap();
        fs::write(&path, bare).unwrap();
        let loaded = load_read_state_from(&path);
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn read_state_save_creates_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("read_state.json");
        save_read_state_to(&path, &[]).unwrap();
        assert!(path.exists());
    }
}
