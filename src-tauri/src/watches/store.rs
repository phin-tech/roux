use std::path::PathBuf;

use roux_core::Watch;

pub use roux_runtime::watch_service::WatchStoreHandle;

pub fn persistence_path() -> PathBuf {
    roux_lib::paths::roux_config_dir().join("watches.json")
}

pub fn load_persisted() -> Vec<Watch> {
    roux_runtime::watch_service::load_persisted_from(persistence_path())
}
