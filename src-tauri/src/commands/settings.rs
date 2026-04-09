use crate::state::AppState;
use tauri::Emitter;

#[tauri::command]
pub(crate) fn get_settings(state: tauri::State<AppState>) -> crate::settings::RouxSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub(crate) fn update_settings(
    settings: crate::settings::RouxSettings,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let settings = settings.normalized();
    crate::logging::set_enabled(settings.enable_logging);
    crate::settings::save_settings(&settings).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings.clone();
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())
}
