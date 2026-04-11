//! Per-session pane state persistence.
//!
//! Writes to `~/.config/roux/pane_state/<session_id>.json` as a versioned
//! envelope: `{ "version": 1, "data": <opaque-frontend-json> }`. Rust does
//! not inspect `data` — the frontend owns that schema.
//!
//! Loader returns `None` on any failure (missing file, IO error, parse error,
//! wrong version) but logs the cause so "my layout vanished" stays debuggable.
//! Saves are atomic via tmp-file + rename in the same directory.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const CURRENT_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    version: u32,
    data: serde_json::Value,
}

fn is_safe_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn pane_state_dir(base: &Path) -> PathBuf {
    base.join("pane_state")
}

fn pane_state_file(base: &Path, session_id: &str) -> PathBuf {
    pane_state_dir(base).join(format!("{session_id}.json"))
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
    Some(envelope.data)
}

/// Test-friendly saver. Writes atomically via tmp-file + rename.
pub fn save_to(base: &Path, session_id: &str, data: serde_json::Value) -> Result<(), String> {
    if !is_safe_session_id(session_id) {
        return Err(format!("unsafe session_id: {session_id:?}"));
    }
    let dir = pane_state_dir(base);
    fs::create_dir_all(&dir).map_err(|e| format!("create_dir_all {dir:?}: {e}"))?;

    let envelope = Envelope {
        version: CURRENT_VERSION,
        data,
    };
    let serialized =
        serde_json::to_vec_pretty(&envelope).map_err(|e| format!("serialize: {e}"))?;

    let target = pane_state_file(base, session_id);
    // Tmp file must live in the same directory as the target so rename stays
    // atomic (same filesystem).
    let tmp = dir.join(format!("{session_id}.json.tmp.{}", std::process::id()));

    {
        let mut f =
            fs::File::create(&tmp).map_err(|e| format!("create tmp {tmp:?}: {e}"))?;
        f.write_all(&serialized)
            .map_err(|e| format!("write tmp {tmp:?}: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("sync tmp {tmp:?}: {e}"))?;
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

/// Public delete — removes from the standard config directory.
pub fn delete_pane_state(session_id: &str) -> Result<(), String> {
    delete_from(&config_base(), session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn roundtrip_save_then_load_returns_identical_payload() {
        let dir = tempfile::tempdir().unwrap();
        let payload = json!({
            "layout": { "kind": "leaf", "paneId": "sess1-main" },
            "descriptors": [
                { "id": "sess1-main", "type": "claude", "ptyId": "sess1" }
            ]
        });
        save_to(dir.path(), "sess1", payload.clone()).unwrap();
        let loaded = load_from(dir.path(), "sess1").unwrap();
        assert_eq!(loaded, payload);
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
            serde_json::to_string(&json!({ "version": 999, "data": {} })).unwrap(),
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
        save_to(dir.path(), "sess1", json!({})).unwrap();
        assert!(pane_state_file(dir.path(), "sess1").exists());

        delete_from(dir.path(), "sess1").unwrap();
        assert!(!pane_state_file(dir.path(), "sess1").exists());
    }

    #[test]
    fn save_creates_pane_state_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!pane_state_dir(dir.path()).exists());

        save_to(dir.path(), "sess1", json!({"hello": "world"})).unwrap();
        assert!(pane_state_dir(dir.path()).is_dir());
    }

    #[test]
    fn save_is_atomic_no_tmp_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        save_to(dir.path(), "sess1", json!({"a": 1})).unwrap();

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
        assert!(save_to(dir.path(), "../etc/passwd", json!({})).is_err());
        assert!(save_to(dir.path(), "a/b", json!({})).is_err());
        assert!(save_to(dir.path(), "", json!({})).is_err());
        assert!(load_from(dir.path(), "../etc/passwd").is_none());
        assert!(delete_from(dir.path(), "../etc/passwd").is_err());
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
        save_to(dir.path(), "sess1", json!({"n": 1})).unwrap();
        save_to(dir.path(), "sess1", json!({"n": 2})).unwrap();
        let loaded = load_from(dir.path(), "sess1").unwrap();
        assert_eq!(loaded, json!({"n": 2}));
    }
}
