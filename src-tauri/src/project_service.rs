use std::path::PathBuf;

pub use roux_runtime::project_service::{
    load_persisted_from, ProjectHandle, ServiceError,
};

pub fn spawn(
    initial_projects: Vec<roux_core::Project>,
) -> (ProjectHandle, tauri::async_runtime::JoinHandle<()>) {
    spawn_with_path(initial_projects, persistence_path())
}

pub fn spawn_with_path(
    initial_projects: Vec<roux_core::Project>,
    persist_path: PathBuf,
) -> (ProjectHandle, tauri::async_runtime::JoinHandle<()>) {
    let (handle, future) =
        roux_runtime::project_service::service_with_path(initial_projects, persist_path);
    let join = tauri::async_runtime::spawn(future);
    (handle, join)
}

pub fn load_persisted() -> Vec<roux_core::Project> {
    load_persisted_from(&persistence_path())
}

fn persistence_path() -> PathBuf {
    crate::paths::roux_config_dir().join("projects.json")
}
