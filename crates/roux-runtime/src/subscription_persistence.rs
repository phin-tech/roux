use std::fs;
use std::path::{Path, PathBuf};

use roux_core::BusSubscription;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

/// Versioned envelope around the subscription array. Mirrors
/// `aliases/persistence.rs` so future shape migrations follow a uniform
/// pattern (the test for unknown future versions still loading the
/// `data` array applies here too).
#[derive(Debug, Serialize, Deserialize)]
struct SubscriptionsFile {
    version: u32,
    data: Vec<BusSubscription>,
}

pub fn persistence_path() -> PathBuf {
    roux_core::paths::roux_config_dir().join("subscriptions.json")
}

pub fn load_subscriptions() -> Vec<BusSubscription> {
    load_from_path(&persistence_path())
}

pub fn save_subscriptions(entries: &[BusSubscription]) -> std::io::Result<()> {
    save_to_path(entries, &persistence_path())
}

pub fn load_from_path(path: &Path) -> Vec<BusSubscription> {
    if !path.exists() {
        return Vec::new();
    }
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    if let Ok(file) = serde_json::from_str::<SubscriptionsFile>(&content) {
        return file.data;
    }

    // Defensive fallback: bare arrays (hand-edited or pre-versioning).
    serde_json::from_str::<Vec<BusSubscription>>(&content).unwrap_or_default()
}

pub fn save_to_path(entries: &[BusSubscription], path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let envelope = SubscriptionsFile { version: SCHEMA_VERSION, data: entries.to_vec() };
    let json = serde_json::to_string_pretty(&envelope)?;
    fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture(id: &str) -> BusSubscription {
        BusSubscription {
            id: id.into(),
            alias: "auditor".into(),
            pattern: "*.completed".into(),
            project_id: None,
            created_at: 1,
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let entries = vec![fixture("a"), fixture("b")];
        save_to_path(&entries, &path).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "a");
    }

    #[test]
    fn load_returns_empty_when_file_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        assert!(load_from_path(&path).is_empty());
    }

    #[test]
    fn load_returns_empty_when_file_is_garbage() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        fs::write(&path, "not json").unwrap();
        assert!(load_from_path(&path).is_empty());
    }

    #[test]
    fn load_accepts_bare_array_for_backcompat() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let bare = serde_json::to_string(&[fixture("a")]).unwrap();
        fs::write(&path, bare).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn load_envelope_with_unknown_version_still_parses_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        let envelope = serde_json::json!({ "version": 999, "data": [fixture("a")] });
        fs::write(&path, envelope.to_string()).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn save_writes_versioned_envelope() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subscriptions.json");
        save_to_path(&[fixture("a")], &path).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["version"], SCHEMA_VERSION);
        assert!(parsed["data"].is_array());
    }
}
