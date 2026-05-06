//! Tauri commands for the smol-machines sidebar.
//!
//! Pattern-mirrors `commands::worktrees`: each command that shells out to
//! `smolvm` is `async` and runs the subprocess via
//! `tauri::async_runtime::spawn_blocking` so the webview thread never
//! blocks on the CLI.

use serde::Serialize;

use crate::services::smolvm as svc;

/// Return-shape for the activity-rail detection probe. Mirrors
/// `IntegrationDetection` in `commands::setup` but lives here so the
/// smol-machines bindings stay self-contained.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SmolvmDetection {
    pub binary_path: Option<String>,
    pub version: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_detect_smolvm() -> SmolvmDetection {
    tauri::async_runtime::spawn_blocking(|| match svc::resolve_smolvm_binary() {
        Some(install) => SmolvmDetection {
            binary_path: Some(install.path.to_string_lossy().into_owned()),
            version: Some(install.version),
        },
        None => SmolvmDetection { binary_path: None, version: None },
    })
    .await
    .unwrap_or(SmolvmDetection { binary_path: None, version: None })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_list_smol_machines() -> Result<Vec<roux_core::SmolMachine>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::list_machines(&install.path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("list_smol_machines task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_start_smol_machine(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::start_machine(&install.path, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("start_smol_machine task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_stop_smol_machine(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::stop_machine(&install.path, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("stop_smol_machine task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_delete_smol_machine(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::delete_machine(&install.path, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("delete_smol_machine task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_create_smol_machine(
    request: roux_core::SmolMachineCreateRequest,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::create_machine(&install.path, &request).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("create_smol_machine task panicked: {e}"))?
}
