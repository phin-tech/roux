pub use roux_runtime::session_service::SessionHandle;

#[cfg(test)]
#[allow(dead_code)]
pub fn spawn(
    initial_sessions: Vec<crate::session::Session>,
) -> (SessionHandle, tauri::async_runtime::JoinHandle<()>) {
    spawn_with_path(initial_sessions, crate::session::persistence_path())
}

#[cfg(test)]
#[allow(dead_code)]
pub fn spawn_with_path(
    initial_sessions: Vec<crate::session::Session>,
    persist_path: std::path::PathBuf,
) -> (SessionHandle, tauri::async_runtime::JoinHandle<()>) {
    let (handle, future) =
        roux_runtime::session_service::service_with_path(initial_sessions, persist_path);
    let join = tauri::async_runtime::spawn(future);
    (handle, join)
}
