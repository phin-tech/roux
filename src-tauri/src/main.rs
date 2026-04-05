#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;

use std::sync::Mutex;
use tauri::{Emitter, Manager};

struct AppState {
    settings: Mutex<settings::RouxSettings>,
}

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> settings::RouxSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn update_settings(
    settings: settings::RouxSettings,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    settings::save_settings(&settings)?;
    *state.settings.lock().unwrap() = settings.clone();
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())
}

fn main() {
    let initial_settings = settings::load_settings();

    tauri::Builder::default()
        .manage(AppState {
            settings: Mutex::new(initial_settings),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
