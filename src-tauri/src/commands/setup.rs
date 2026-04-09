use crate::services::setup as svc;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupStatus {
    cli_installed: bool,
    gh_available: bool,
}

#[tauri::command]
pub(crate) fn check_setup_status() -> SetupStatus {
    SetupStatus {
        cli_installed: svc::is_cli_installed(),
        gh_available: svc::is_command_available("gh"),
    }
}

#[tauri::command]
pub(crate) fn check_setup_needed() -> bool {
    !svc::is_cli_installed()
}

#[tauri::command]
pub(crate) fn run_setup() -> Result<(), String> {
    svc::install_hooks().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn check_nono_installed() -> bool {
    svc::is_command_available("nono")
}

#[tauri::command]
pub(crate) fn list_nono_profiles() -> Vec<String> {
    svc::list_nono_profiles()
}
