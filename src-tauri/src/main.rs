#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;
mod worktree;

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

#[tauri::command]
fn cmd_create_worktree(
    repo_path: String,
    branch: String,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap();
    let base_path = settings.worktree_base_path.as_deref();
    worktree::create_worktree(&repo_path, &branch, base_path)
}

#[tauri::command]
fn cmd_remove_worktree(worktree_path: String) -> Result<(), String> {
    worktree::remove_worktree(&worktree_path)
}

#[tauri::command]
fn cmd_list_worktrees(repo_path: String) -> Result<Vec<worktree::Worktree>, String> {
    worktree::list_worktrees(&repo_path)
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
            cmd_create_worktree,
            cmd_remove_worktree,
            cmd_list_worktrees,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
