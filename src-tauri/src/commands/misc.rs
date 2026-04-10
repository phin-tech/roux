use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub(crate) fn get_log_path() -> String {
    crate::logging::log_path().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn frontend_log(message: String) {
    crate::logging::log(&format!("[frontend] {}", message));
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_open_in_editor(path: String) -> Result<(), String> {
    std::process::Command::new("code")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to open VS Code: {}", e))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn quit_app(state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    state.pty_manager.shutdown_all();
    state.session_handle.shutdown().await;
    state.project_handle.shutdown().await;
    crate::socket::cleanup_socket();
    app.exit(0);
    Ok(())
}
