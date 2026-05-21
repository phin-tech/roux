//! Per-session pane state persistence.
//!
//! Writes to `~/.config/roux/pane_state/<session_id>.json` as a versioned
//! envelope: `{ "version": 1, "data": <pane-state-payload> }`.
//!
//! Rust validates the persisted payload shape on both save and load so the
//! app never writes or restores obviously-invalid pane state (unknown pane
//! kinds, malformed layout nodes, corrupt profile refs, etc.). The frontend
//! still owns the *meaning* of the layout tree and schema version policy, but
//! the backend now owns the serialization contract instead of treating `data`
//! as opaque JSON.
//!
//! Loader returns `None` on any failure (missing file, IO error, parse error,
//! wrong version) but logs the cause so "my layout vanished" stays debuggable.
//! Saves are atomic via tmp-file + rename in the same directory.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::pane_service::PaneDescriptor;
use roux_core::SpawnProfile;

const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PaneKind {
    Shell,
    Markdown,
    Command,
    Notes,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum SplitDirection {
    H,
    V,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum PersistedSpawnProfileRef {
    Registered { id: String },
    Inline { profile: Box<SpawnProfile> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum PersistedLayoutNode {
    Leaf {
        #[serde(rename = "paneId")]
        pane_id: String,
    },
    Split {
        direction: SplitDirection,
        children: Vec<PersistedLayoutNode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sizes: Option<Vec<f64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stacked: Option<bool>,
        #[serde(rename = "activeIndex", skip_serializing_if = "Option::is_none")]
        active_index: Option<usize>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PersistedPaneDescriptor {
    id: String,
    #[serde(rename = "type")]
    pane_type: PaneKind,
    pty_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    working_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spawn_profile_ref: Option<PersistedSpawnProfileRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nono_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nono_allow_dirs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes_view_mode: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PersistedPaneStatePayload {
    schema_version: u32,
    layout: PersistedLayoutNode,
    descriptors: Vec<PersistedPaneDescriptor>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    version: u32,
    data: serde_json::Value,
}

#[derive(Debug, Clone)]
struct LatestProviderSession {
    provider: Option<String>,
    provider_session_id: String,
    timestamp: i64,
}

fn parse_payload(data: serde_json::Value) -> Result<PersistedPaneStatePayload, String> {
    serde_json::from_value(data).map_err(|e| format!("invalid pane-state payload: {e}"))
}

fn parse_payload_ref(data: &serde_json::Value) -> Result<PersistedPaneStatePayload, String> {
    serde_json::from_value(data.clone()).map_err(|e| format!("invalid pane-state payload: {e}"))
}

fn is_safe_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn pane_state_dir(base: &Path) -> PathBuf {
    base.join("pane_state")
}

fn pane_state_file(base: &Path, session_id: &str) -> PathBuf {
    pane_state_dir(base).join(format!("{session_id}.json"))
}

fn status_dir(base: &Path) -> PathBuf {
    base.join("status")
}

fn latest_provider_sessions_by_pane(
    base: &Path,
    session_id: &str,
) -> HashMap<String, LatestProviderSession> {
    let dir = status_dir(base);
    let Ok(entries) = fs::read_dir(&dir) else {
        return HashMap::new();
    };

    let mut latest = HashMap::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if parsed
            .get("roux_session_id")
            .and_then(|v| v.as_str())
            != Some(session_id)
        {
            continue;
        }
        let Some(pane_id) = parsed
            .get("roux_pane_id")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        // Generic field is the new contract; `claude_session_id` is the
        // legacy name kept for old hook payloads. Track which field
        // supplied the value so we only default `provider = "claude"`
        // for the legacy field — the new generic field is provider-
        // agnostic and must not be auto-tagged.
        let generic_session_id = parsed
            .get("provider_session_id")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let legacy_claude_session_id = parsed
            .get("claude_session_id")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let Some(provider_session_id) = generic_session_id
            .clone()
            .or_else(|| legacy_claude_session_id.clone())
        else {
            continue;
        };
        let provider = parsed
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| legacy_claude_session_id.as_ref().map(|_| "claude".to_string()));
        let timestamp = parsed.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
        let candidate = LatestProviderSession { provider, provider_session_id, timestamp };
        let should_replace = latest
            .get(pane_id)
            .map(|existing: &LatestProviderSession| candidate.timestamp >= existing.timestamp)
            .unwrap_or(true);
        if should_replace {
            latest.insert(pane_id.to_string(), candidate);
        }
    }
    latest
}

/// Fill descriptor `provider`/`provider_session_id` from the latest
/// status-file entry per pane. Called from BOTH save and load:
///
/// - On load: the frontend reads the descriptor and uses it to build the
///   resume command without having to wait for a status update first.
/// - On save: the latest provider session id gets baked into the on-disk
///   layout, so a hard quit (no debounce flush) can still restore it next
///   launch even if the status file is later rotated/cleaned.
///
/// Existing values are never overwritten — a descriptor that already names
/// a session id wins over anything in the status dir.
fn enrich_payload_with_provider_sessions(
    base: &Path,
    session_id: &str,
    payload: &mut PersistedPaneStatePayload,
) {
    let latest = latest_provider_sessions_by_pane(base, session_id);
    if latest.is_empty() {
        return;
    }
    for descriptor in &mut payload.descriptors {
        let Some(provider_session) = latest.get(&descriptor.id) else {
            continue;
        };
        if descriptor.provider_session_id.is_none() {
            descriptor.provider_session_id = Some(provider_session.provider_session_id.clone());
        }
        if descriptor.provider.is_none() {
            descriptor.provider = provider_session.provider.clone();
        }
    }
}

/// Test-friendly loader. Returns `None` on any failure, logging the cause.
pub fn load_from(base: &Path, session_id: &str) -> Option<serde_json::Value> {
    if !is_safe_session_id(session_id) {
        rlog!("pane_state::load_from: rejected unsafe session_id {session_id:?}");
        return None;
    }
    let path = pane_state_file(base, session_id);
    let content = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            rlog!("pane_state::load_from: read {path:?} failed: {e}");
            return None;
        }
    };
    let envelope: Envelope = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            rlog!("pane_state::load_from: parse {path:?} failed: {e}");
            return None;
        }
    };
    if envelope.version != CURRENT_VERSION {
        rlog!(
            "pane_state::load_from: version mismatch for {path:?} (expected {}, got {})",
            CURRENT_VERSION,
            envelope.version
        );
        return None;
    }
    let mut payload = match parse_payload_ref(&envelope.data) {
        Ok(payload) => payload,
        Err(e) => {
            rlog!("pane_state::load_from: {e}");
            return None;
        }
    };
    enrich_payload_with_provider_sessions(base, session_id, &mut payload);
    serde_json::to_value(payload).ok()
}

/// Test-friendly saver. Writes atomically via tmp-file + rename.
pub fn save_to(base: &Path, session_id: &str, data: serde_json::Value) -> Result<(), String> {
    if !is_safe_session_id(session_id) {
        return Err(format!("unsafe session_id: {session_id:?}"));
    }
    let dir = pane_state_dir(base);
    fs::create_dir_all(&dir).map_err(|e| format!("create_dir_all {dir:?}: {e}"))?;

    let mut payload = parse_payload(data)?;
    enrich_payload_with_provider_sessions(base, session_id, &mut payload);
    let data = serde_json::to_value(payload).map_err(|e| format!("serialize payload: {e}"))?;

    let envelope = Envelope { version: CURRENT_VERSION, data };
    let serialized = serde_json::to_vec_pretty(&envelope).map_err(|e| format!("serialize: {e}"))?;

    let target = pane_state_file(base, session_id);
    // Tmp file must live in the same directory as the target so rename stays
    // atomic (same filesystem).
    let tmp = dir.join(format!("{session_id}.json.tmp.{}", std::process::id()));

    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("create tmp {tmp:?}: {e}"))?;
        f.write_all(&serialized).map_err(|e| format!("write tmp {tmp:?}: {e}"))?;
        f.sync_all().map_err(|e| format!("sync tmp {tmp:?}: {e}"))?;
    }

    fs::rename(&tmp, &target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("rename {tmp:?} -> {target:?}: {e}")
    })?;
    Ok(())
}

/// Test-friendly delete. Best-effort; missing file is not an error.
pub fn delete_from(base: &Path, session_id: &str) -> Result<(), String> {
    if !is_safe_session_id(session_id) {
        return Err(format!("unsafe session_id: {session_id:?}"));
    }
    let path = pane_state_file(base, session_id);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {path:?}: {e}")),
    }
}

fn config_base() -> PathBuf {
    crate::paths::roux_config_dir()
}

/// Public loader — reads from the standard config directory.
pub fn load_pane_state(session_id: &str) -> Option<serde_json::Value> {
    load_from(&config_base(), session_id)
}

/// Public saver — writes to the standard config directory.
pub fn save_pane_state(session_id: &str, data: serde_json::Value) -> Result<(), String> {
    save_to(&config_base(), session_id, data)
}

pub fn save_live_to(
    base: &Path,
    session_id: &str,
    schema_version: u32,
    layout: serde_json::Value,
    descriptors: Vec<PaneDescriptor>,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "schemaVersion": schema_version,
        "layout": layout,
        "descriptors": descriptors,
    });
    save_to(base, session_id, payload)
}

/// Public saver for backend-owned live pane snapshots.
pub fn save_live_pane_state(
    session_id: &str,
    schema_version: u32,
    layout: serde_json::Value,
    descriptors: Vec<PaneDescriptor>,
) -> Result<(), String> {
    save_live_to(&config_base(), session_id, schema_version, layout, descriptors)
}

/// Public delete — removes from the standard config directory.
pub fn delete_pane_state(session_id: &str) -> Result<(), String> {
    delete_from(&config_base(), session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn minimal_payload() -> serde_json::Value {
        json!({
            "schemaVersion": 4,
            "layout": { "kind": "leaf", "paneId": "sess1-main" },
            "descriptors": [
                { "id": "sess1-main", "type": "shell", "ptyId": "sess1" }
            ]
        })
    }

    fn write_status(
        base: &Path,
        filename: &str,
        session_id: &str,
        pane_id: &str,
        provider: &str,
        provider_session_id: &str,
        timestamp: i64,
    ) {
        let dir = status_dir(base);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(filename),
            serde_json::to_string(&json!({
                "status": "idle",
                "roux_session_id": session_id,
                "roux_pane_id": pane_id,
                "provider": provider,
                "provider_session_id": provider_session_id,
                "timestamp": timestamp
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn roundtrip_save_then_load_returns_identical_payload() {
        let dir = tempfile::tempdir().unwrap();
        let payload = minimal_payload();
        save_to(dir.path(), "sess1", payload.clone()).unwrap();
        let loaded = load_from(dir.path(), "sess1").unwrap();
        assert_eq!(loaded, payload);
    }

    fn write_status_raw(base: &Path, filename: &str, body: serde_json::Value) {
        let dir = status_dir(base);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(filename), serde_json::to_string(&body).unwrap()).unwrap();
    }

    #[test]
    fn load_does_not_default_provider_to_claude_for_generic_provider_session_id() {
        let dir = tempfile::tempdir().unwrap();
        save_to(dir.path(), "sess1", minimal_payload()).unwrap();
        // Status file uses the new generic field with no `provider` key —
        // we must NOT auto-tag this as Claude.
        write_status_raw(
            dir.path(),
            "generic.json",
            json!({
                "status": "idle",
                "roux_session_id": "sess1",
                "roux_pane_id": "sess1-main",
                "provider_session_id": "agent-xyz",
                "timestamp": 50,
            }),
        );

        let loaded = load_from(dir.path(), "sess1").unwrap();

        assert_eq!(
            loaded["descriptors"][0]["providerSessionId"],
            json!("agent-xyz"),
        );
        // Provider must remain absent — not silently coerced to "claude".
        assert!(loaded["descriptors"][0].get("provider").is_none());
    }

    #[test]
    fn load_defaults_provider_to_claude_only_for_legacy_claude_session_id() {
        let dir = tempfile::tempdir().unwrap();
        save_to(dir.path(), "sess1", minimal_payload()).unwrap();
        // Legacy field with no explicit provider — should still default
        // to "claude" since `claude_session_id` is by definition claude.
        write_status_raw(
            dir.path(),
            "legacy.json",
            json!({
                "status": "idle",
                "roux_session_id": "sess1",
                "roux_pane_id": "sess1-main",
                "claude_session_id": "claude-legacy-1",
                "timestamp": 50,
            }),
        );

        let loaded = load_from(dir.path(), "sess1").unwrap();

        assert_eq!(
            loaded["descriptors"][0]["providerSessionId"],
            json!("claude-legacy-1"),
        );
        assert_eq!(loaded["descriptors"][0]["provider"], json!("claude"));
    }

    #[test]
    fn load_enriches_descriptors_with_latest_provider_session_status() {
        let dir = tempfile::tempdir().unwrap();
        save_to(dir.path(), "sess1", minimal_payload()).unwrap();
        write_status(
            dir.path(),
            "old.json",
            "sess1",
            "sess1-main",
            "claude",
            "claude-old",
            10,
        );
        write_status(
            dir.path(),
            "new.json",
            "sess1",
            "sess1-main",
            "claude",
            "claude-new",
            20,
        );

        let loaded = load_from(dir.path(), "sess1").unwrap();

        assert_eq!(
            loaded["descriptors"][0]["providerSessionId"],
            json!("claude-new"),
        );
        assert_eq!(loaded["descriptors"][0]["provider"], json!("claude"));
    }

    #[test]
    fn load_does_not_overwrite_explicit_provider_session_id_with_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let payload = json!({
            "schemaVersion": 4,
            "layout": { "kind": "leaf", "paneId": "sess1-main" },
            "descriptors": [
                {
                    "id": "sess1-main",
                    "type": "shell",
                    "ptyId": "sess1",
                    "provider": "claude",
                    "providerSessionId": "explicit-current"
                }
            ]
        });
        save_to(dir.path(), "sess1", payload).unwrap();
        write_status(
            dir.path(),
            "stale.json",
            "sess1",
            "sess1-main",
            "claude",
            "stale-from-status",
            999,
        );

        let loaded = load_from(dir.path(), "sess1").unwrap();

        assert_eq!(
            loaded["descriptors"][0]["providerSessionId"],
            json!("explicit-current"),
        );
    }

    #[test]
    fn load_ignores_status_for_other_sessions_or_removed_panes() {
        let dir = tempfile::tempdir().unwrap();
        save_to(dir.path(), "sess1", minimal_payload()).unwrap();
        write_status(
            dir.path(),
            "other-session.json",
            "sess2",
            "sess1-main",
            "claude",
            "other-session-id",
            999,
        );
        write_status(
            dir.path(),
            "removed-pane.json",
            "sess1",
            "removed-pane",
            "claude",
            "removed-pane-id",
            999,
        );

        let loaded = load_from(dir.path(), "sess1").unwrap();

        assert!(loaded["descriptors"][0].get("providerSessionId").is_none());
    }

    #[test]
    fn load_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_from(dir.path(), "does-not-exist").is_none());
    }

    #[test]
    fn load_wrong_version_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = pane_state_dir(dir.path());
        fs::create_dir_all(&state_dir).unwrap();
        let path = pane_state_file(dir.path(), "sess1");
        fs::write(
            &path,
            serde_json::to_string(
                &json!({ "version": 999, "data": { "schemaVersion": 4, "layout": { "kind": "leaf", "paneId": "sess1-main" }, "descriptors": [] } }),
            )
            .unwrap(),
        )
            .unwrap();

        assert!(load_from(dir.path(), "sess1").is_none());
    }

    #[test]
    fn load_corrupt_json_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = pane_state_dir(dir.path());
        fs::create_dir_all(&state_dir).unwrap();
        let path = pane_state_file(dir.path(), "sess1");
        fs::write(&path, "{ not valid json").unwrap();

        assert!(load_from(dir.path(), "sess1").is_none());
    }

    #[test]
    fn delete_missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        delete_from(dir.path(), "never-existed").unwrap();
    }

    #[test]
    fn delete_removes_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        save_to(dir.path(), "sess1", minimal_payload()).unwrap();
        assert!(pane_state_file(dir.path(), "sess1").exists());

        delete_from(dir.path(), "sess1").unwrap();
        assert!(!pane_state_file(dir.path(), "sess1").exists());
    }

    #[test]
    fn save_creates_pane_state_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!pane_state_dir(dir.path()).exists());

        save_to(dir.path(), "sess1", minimal_payload()).unwrap();
        assert!(pane_state_dir(dir.path()).is_dir());
    }

    #[test]
    fn save_is_atomic_no_tmp_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        save_to(dir.path(), "sess1", minimal_payload()).unwrap();

        let state_dir = pane_state_dir(dir.path());
        let entries: Vec<_> = fs::read_dir(&state_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert_eq!(entries, vec!["sess1.json"]);
    }

    #[test]
    fn unsafe_session_ids_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let payload = minimal_payload();
        assert!(save_to(dir.path(), "../etc/passwd", payload.clone()).is_err());
        assert!(save_to(dir.path(), "a/b", payload.clone()).is_err());
        assert!(save_to(dir.path(), "", payload).is_err());
        assert!(load_from(dir.path(), "../etc/passwd").is_none());
        assert!(delete_from(dir.path(), "../etc/passwd").is_err());
    }

    #[test]
    fn save_rejects_invalid_pane_descriptor_type() {
        let dir = tempfile::tempdir().unwrap();
        let payload = json!({
            "schemaVersion": 4,
            "layout": { "kind": "leaf", "paneId": "sess1-main" },
            "descriptors": [
                { "id": "sess1-main", "type": "claude", "ptyId": "sess1" }
            ]
        });
        assert!(save_to(dir.path(), "sess1", payload).is_err());
    }

    #[test]
    fn load_rejects_invalid_pane_descriptor_type() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = pane_state_dir(dir.path());
        fs::create_dir_all(&state_dir).unwrap();
        let path = pane_state_file(dir.path(), "sess1");
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "version": 1,
                "data": {
                    "schemaVersion": 4,
                    "layout": { "kind": "leaf", "paneId": "sess1-main" },
                    "descriptors": [
                        { "id": "sess1-main", "type": "claude", "ptyId": "sess1" }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(load_from(dir.path(), "sess1").is_none());
    }

    #[test]
    fn is_safe_session_id_accepts_uuids_and_simple_names() {
        assert!(is_safe_session_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_safe_session_id("abc_123"));
        assert!(is_safe_session_id("abc-123"));
        assert!(!is_safe_session_id(""));
        assert!(!is_safe_session_id("a/b"));
        assert!(!is_safe_session_id("a b"));
        assert!(!is_safe_session_id("a."));
    }

    #[test]
    fn load_returns_latest_after_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let first = json!({
            "schemaVersion": 4,
            "layout": { "kind": "leaf", "paneId": "sess1-main" },
            "descriptors": [
                { "id": "sess1-main", "type": "shell", "ptyId": "sess1", "name": "first" }
            ]
        });
        let second = json!({
            "schemaVersion": 4,
            "layout": { "kind": "leaf", "paneId": "sess1-main" },
            "descriptors": [
                { "id": "sess1-main", "type": "shell", "ptyId": "sess1", "name": "second" }
            ]
        });
        save_to(dir.path(), "sess1", first).unwrap();
        save_to(dir.path(), "sess1", second.clone()).unwrap();
        let loaded = load_from(dir.path(), "sess1").unwrap();
        assert_eq!(loaded, second);
    }

    #[test]
    fn save_live_to_serializes_backend_descriptors() {
        let dir = tempfile::tempdir().unwrap();
        let descriptors = vec![PaneDescriptor {
            id: "sess1-main".into(),
            pane_type: "shell".into(),
            pty_id: "sess1".into(),
            name: Some("Main".into()),
            working_dir: Some("/tmp/live".into()),
            command: None,
            doc_path: None,
            spawn_profile_ref: None,
            provider: None,
            provider_session_id: None,
            nono_profile: None,
            nono_allow_dirs: None,
            notes_scope: None,
            notes_view_mode: None,
            session_id: None,
        }];

        save_live_to(
            dir.path(),
            "sess1",
            4,
            json!({ "kind": "leaf", "paneId": "sess1-main" }),
            descriptors,
        )
        .unwrap();

        let loaded = load_from(dir.path(), "sess1").unwrap();
        assert_eq!(
            loaded,
            json!({
                "schemaVersion": 4,
                "layout": { "kind": "leaf", "paneId": "sess1-main" },
                "descriptors": [
                    {
                        "id": "sess1-main",
                        "type": "shell",
                        "ptyId": "sess1",
                        "name": "Main",
                        "workingDir": "/tmp/live"
                    }
                ]
            })
        );
    }

    #[test]
    fn save_live_to_enriches_descriptors_with_provider_session_status() {
        let dir = tempfile::tempdir().unwrap();
        write_status(
            dir.path(),
            "claude.json",
            "sess1",
            "sess1-main",
            "claude",
            "claude-session-123",
            10,
        );
        let descriptors = vec![PaneDescriptor {
            id: "sess1-main".into(),
            pane_type: "shell".into(),
            pty_id: "sess1".into(),
            name: None,
            working_dir: None,
            command: None,
            doc_path: None,
            spawn_profile_ref: None,
            provider: None,
            provider_session_id: None,
            nono_profile: None,
            nono_allow_dirs: None,
            notes_scope: None,
            notes_view_mode: None,
            session_id: None,
        }];

        save_live_to(
            dir.path(),
            "sess1",
            4,
            json!({ "kind": "leaf", "paneId": "sess1-main" }),
            descriptors,
        )
        .unwrap();

        let saved = fs::read_to_string(pane_state_file(dir.path(), "sess1")).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            envelope["data"]["descriptors"][0]["providerSessionId"],
            json!("claude-session-123"),
        );
        assert_eq!(envelope["data"]["descriptors"][0]["provider"], json!("claude"));
    }

    #[test]
    fn roundtrip_preserves_stacked_split_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let payload = json!({
            "schemaVersion": 4,
            "layout": {
                "kind": "split",
                "direction": "h",
                "stacked": true,
                "activeIndex": 1,
                "sizes": [0.4, 0.6],
                "children": [
                    { "kind": "leaf", "paneId": "sess1-main" },
                    { "kind": "leaf", "paneId": "notes-pane" }
                ]
            },
            "descriptors": [
                { "id": "sess1-main", "type": "shell", "ptyId": "sess1" },
                { "id": "notes-pane", "type": "notes", "ptyId": "", "notesScope": "session" }
            ]
        });

        save_to(dir.path(), "sess1", payload.clone()).unwrap();

        let loaded = load_from(dir.path(), "sess1").unwrap();
        assert_eq!(loaded, payload);
    }
}
