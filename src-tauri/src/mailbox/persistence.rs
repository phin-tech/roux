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

/// Append-only retract marker. Written when a sender unsends an
/// event; the loader detects rows with `rowType: "retract"` and
/// applies `retracted_at` to the matching event so the audit log
/// stays append-only (we don't rewrite previously-written rows).
#[derive(Debug, Serialize, Deserialize)]
struct RetractRowV1 {
    #[serde(rename = "schemaVersion", default = "default_schema_version")]
    schema_version: u32,
    /// Tag used by the loader to disambiguate from event rows.
    #[serde(rename = "rowType")]
    row_type: String,
    #[serde(rename = "eventId")]
    event_id: String,
    #[serde(rename = "retractedAt")]
    retracted_at: u64,
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

pub fn append_retract(event_id: &str, retracted_at: u64) -> io::Result<()> {
    append_retract_to(&events_path(), event_id, retracted_at)
}

pub fn save_read_state(states: &[ReadState]) -> io::Result<()> {
    save_read_state_to(&read_state_path(), states)
}

/// Read all events from `path`. Malformed JSON lines are skipped silently
/// (logged via stderr). Rows whose `schemaVersion` is greater than the
/// version this binary understands are skipped at load time but retained
/// on disk by virtue of being append-only.
///
/// Retract marker rows (`rowType: "retract"`) are detected here and
/// applied to the matching event's `retracted_at` field — the audit log
/// stays append-only and the in-memory store reflects the retraction.
pub(crate) fn load_events_from(path: &Path) -> Vec<Event> {
    if !path.exists() {
        return Vec::new();
    }
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);
    let mut events: Vec<Event> = Vec::new();
    let mut retracts: Vec<(String, u64)> = Vec::new();
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
        // Retract marker — collect; apply after all events are loaded.
        if value.get("rowType").and_then(|v| v.as_str()) == Some("retract") {
            match serde_json::from_value::<RetractRowV1>(value) {
                Ok(row) => retracts.push((row.event_id, row.retracted_at)),
                Err(e) => {
                    eprintln!(
                        "[roux] events.jsonl:{}: malformed retract row: {e}",
                        lineno + 1
                    );
                }
            }
            continue;
        }
        match serde_json::from_value::<EventRowV1>(value) {
            Ok(row) => events.push(row.event),
            Err(e) => {
                eprintln!(
                    "[roux] events.jsonl:{}: row failed to deserialize: {e}",
                    lineno + 1
                );
            }
        }
    }
    // Apply retract markers. Earliest retract wins on duplicates —
    // the first retract is authoritative; later markers (which only
    // appear on corruption or hand-edited files in normal use) are
    // ignored. The matching test is `earliest_retract_wins_when_duplicated`.
    for (id, at) in retracts {
        if let Some(e) = events.iter_mut().find(|e| e.id == id) {
            if e.retracted_at.is_none() {
                e.retracted_at = Some(at);
            }
        }
    }
    events
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

pub(crate) fn append_retract_to(
    path: &Path,
    event_id: &str,
    retracted_at: u64,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let row = RetractRowV1 {
        schema_version: EVENT_SCHEMA_VERSION,
        row_type: "retract".to_string(),
        event_id: event_id.to_string(),
        retracted_at,
    };
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

    // ── Retract marker rows ────────────────────────────────────────

    #[test]
    fn append_retract_then_load_applies_retracted_at() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_event_to(&path, &sample_event("e1", "x")).unwrap();
        append_retract_to(&path, "e1", 9999).unwrap();
        let loaded = load_events_from(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].retracted_at, Some(9999));
    }

    #[test]
    fn retract_marker_for_missing_event_is_silent_noop() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_event_to(&path, &sample_event("e1", "x")).unwrap();
        // Marker references an event that never existed (or was
        // evicted). Loader doesn't fail; just doesn't apply.
        append_retract_to(&path, "ghost", 5000).unwrap();
        let loaded = load_events_from(&path);
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].retracted_at.is_none());
    }

    #[test]
    fn earliest_retract_wins_when_duplicated() {
        // Defensive: if two retract markers somehow appear for the
        // same id (e.g. corruption, ill-behaved external editor), the
        // first one is authoritative.
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_event_to(&path, &sample_event("e1", "x")).unwrap();
        append_retract_to(&path, "e1", 1000).unwrap();
        append_retract_to(&path, "e1", 9000).unwrap();
        let loaded = load_events_from(&path);
        assert_eq!(loaded[0].retracted_at, Some(1000));
    }

    #[test]
    fn append_retract_writes_row_type_tag() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_retract_to(&path, "e1", 5).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(parsed["rowType"], "retract");
        assert_eq!(parsed["eventId"], "e1");
        assert_eq!(parsed["retractedAt"], 5);
    }
}
