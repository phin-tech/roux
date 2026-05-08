use std::fs;
use std::path::{Path, PathBuf};

use roux_core::AgentAlias;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

/// Versioned envelope wrapping the alias array. Versioning is baked in
/// from day one so future shape changes can migrate or coexist without
/// silently corrupting on-disk state.
#[derive(Debug, Serialize, Deserialize)]
struct AliasesFile {
    version: u32,
    data: Vec<AgentAlias>,
}

pub fn persistence_path() -> PathBuf {
    crate::paths::roux_config_dir().join("aliases.json")
}

pub fn load_aliases() -> Vec<AgentAlias> {
    load_from_path(&persistence_path())
}

pub fn save_aliases(entries: &[AgentAlias]) -> std::io::Result<()> {
    save_to_path(entries, &persistence_path())
}

pub(crate) fn load_from_path(path: &Path) -> Vec<AgentAlias> {
    if !path.exists() {
        return Vec::new();
    }
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    if let Ok(file) = serde_json::from_str::<AliasesFile>(&content) {
        return file.data;
    }

    // Defensive fallback: pre-versioning files (or hand-edited bare
    // arrays) parse as a plain Vec.
    serde_json::from_str::<Vec<AgentAlias>>(&content).unwrap_or_default()
}

pub(crate) fn save_to_path(entries: &[AgentAlias], path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let envelope = AliasesFile { version: SCHEMA_VERSION, data: entries.to_vec() };
    let json = serde_json::to_string_pretty(&envelope)?;
    fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let mut a = AgentAlias::new("reviewer", None);
        a.session_id = Some("sess-1".into());
        let mut b = AgentAlias::new("frontend", Some("proj-x".into()));
        b.session_id = Some("sess-2".into());
        let entries = vec![a.clone(), b.clone()];

        save_to_path(&entries, &path).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], a);
        assert_eq!(loaded[1], b);
    }

    #[test]
    fn load_returns_empty_when_file_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        assert!(load_from_path(&path).is_empty());
    }

    #[test]
    fn load_returns_empty_when_file_is_malformed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        fs::write(&path, "not json at all").unwrap();
        assert!(load_from_path(&path).is_empty());
    }

    #[test]
    fn load_accepts_bare_array_for_back_compat() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let entries = vec![AgentAlias::new("reviewer", None)];
        let bare_array = serde_json::to_string(&entries).unwrap();
        fs::write(&path, bare_array).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].alias, "reviewer");
    }

    #[test]
    fn load_envelope_with_unknown_version_still_parses_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let entries = vec![AgentAlias::new("reviewer", None)];
        let envelope = serde_json::json!({
            "version": 999,
            "data": entries,
        });
        fs::write(&path, envelope.to_string()).unwrap();
        let loaded = load_from_path(&path);
        assert_eq!(loaded.len(), 1, "unknown future version should still load data");
    }

    #[test]
    fn save_writes_versioned_envelope() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        save_to_path(&[AgentAlias::new("reviewer", None)], &path).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["version"], SCHEMA_VERSION);
        assert!(parsed["data"].is_array());
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("aliases.json");
        save_to_path(&[], &path).unwrap();
        assert!(path.exists());
    }
}
