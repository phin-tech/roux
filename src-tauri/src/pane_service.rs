pub use roux_runtime::pane_service::{
    PaneDescriptor, PaneHandle, PaneRecord,
};

pub fn spawn() -> (PaneHandle, tauri::async_runtime::JoinHandle<()>) {
    let (handle, future) = roux_runtime::pane_service::service();
    let join = tauri::async_runtime::spawn(future);
    (handle, join)
}
