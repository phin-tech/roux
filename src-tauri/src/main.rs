#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hooks;
mod osc;
mod pty;
mod session;
mod settings;
mod status_watcher;
mod worktree;

use std::sync::Mutex;
use tauri::{Emitter, Manager};

use crate::pty::PtyManager;
use crate::session::{Session, SessionStore};

struct AppState {
    settings: Mutex<settings::RouxSettings>,
    pty_manager: PtyManager,
    session_store: SessionStore,
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
    state.pty_manager.kill(&id)?;
    state.session_store.remove(&id);
    Ok(())
}

#[tauri::command]
fn create_session(
    repo_path: String,
    name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory
    let (work_dir, actual_branch, is_wt) = if let Some(wt_path) = worktree_path {
        // Use provided worktree path — detect branch from the directory
        let br = branch
            .or_else(|| get_current_branch(&wt_path))
            .unwrap_or_else(|| "main".to_string());
        (wt_path, br, false)
    } else if let Some(br) = branch {
        // Create new worktree
        let base = settings.worktree_base_path.as_deref();
        let wt_path = worktree::create_worktree(&repo_path, &br, base)?;
        (wt_path, br, true)
    } else {
        // Use repo directly
        let br = get_current_branch(&repo_path).unwrap_or_else(|| "main".to_string());
        (repo_path.clone(), br, false)
    };

    // Spawn PTY
    let spawn_result = state.pty_manager.spawn(
        &session_id,
        &work_dir,
        settings.default_model.as_deref(),
        &settings.additional_flags,
        app.clone(),
    );

    // Rollback worktree on spawn failure
    if let Err(e) = spawn_result {
        if is_wt {
            let _ = worktree::remove_worktree(&work_dir);
        }
        return Err(e);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let session = Session {
        id: session_id,
        name,
        repo_root: repo_path,
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: is_wt,
        status: "idle".to_string(),
        model: None,
        cost: None,
        created_at: now,
    };

    state.session_store.add(session.clone());
    Ok(session)
}

fn get_current_branch(repo_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

#[tauri::command]
fn list_sessions(state: tauri::State<AppState>) -> Vec<Session> {
    state.session_store.list()
}

fn main() {
    let initial_settings = settings::load_settings();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            settings: Mutex::new(initial_settings),
            pty_manager: PtyManager::new(),
            session_store: SessionStore::load_persisted(),
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
            create_session,
            list_sessions,
        ])
        .setup(|app| {
            if let Err(e) = hooks::install_hooks() {
                eprintln!("Warning: failed to install hooks: {}", e);
            }
            if let Err(e) = status_watcher::start_watching(app.handle().clone()) {
                eprintln!("Warning: failed to start status watcher: {}", e);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
