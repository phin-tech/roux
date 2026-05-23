use std::path::{Path, PathBuf};

use roux_core::AgentAlias;

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
    roux_runtime::alias_persistence::load_from_path(path)
}

pub(crate) fn save_to_path(entries: &[AgentAlias], path: &Path) -> std::io::Result<()> {
    roux_runtime::alias_persistence::save_to_path(entries, path)
}
