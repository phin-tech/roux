#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod osc;
mod pty;
mod settings;
mod worktree;

use std::sync::Mutex;
use tauri::{Emitter, Manager};

use crate::pty::PtyManager;

struct AppState {
    settings: Mutex<settings::RouxSettings>,
    pty_manager: PtyManager,
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

// Note: spec says Vec<u8> but xterm.js onData sends UTF-8 strings.
// We accept String and convert to bytes server-side for simplicity.
#[tauri::command]
fn write_to_session(id: String, data: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.write(&id, data.as_bytes())
}

#[tauri::command]
fn resize_session(
    id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.pty_manager.resize(&id, cols, rows)
}

#[tauri::command]
fn kill_session(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.kill(&id)
}

fn main() {
    let initial_settings = settings::load_settings();

    tauri::Builder::default()
        .manage(AppState {
            settings: Mutex::new(initial_settings),
            pty_manager: PtyManager::new(),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            cmd_create_worktree,
            cmd_remove_worktree,
            cmd_list_worktrees,
            write_to_session,
            resize_session,
            kill_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
