#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hooks;
#[macro_use]
mod logging;
mod pty;
mod session;
mod settings;
mod status_watcher;
mod tasks;
mod worktree;

use std::sync::Mutex;
use tauri::{Emitter, Manager};

use crate::pty::PtyManager;
use crate::session::{Session, SessionStore};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DocFile {
    path: String,
    name: String,
    relative_path: String,
    modified: u64,
}

struct AppState {
    settings: Mutex<settings::RouxSettings>,
    pty_manager: PtyManager,
    session_store: SessionStore,
}

#[tauri::command]
fn get_log_path() -> String {
    logging::log_path().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}

#[tauri::command]
fn frontend_log(message: String) {
    logging::log(&format!("[frontend] {}", message));
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
    let settings = settings.normalized();
    logging::set_enabled(settings.enable_logging);
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

#[tauri::command]
fn cmd_list_branches(repo_path: String) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to list branches: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(branches)
}

#[tauri::command]
fn cmd_open_in_editor(path: String) -> Result<(), String> {
    std::process::Command::new("code")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to open VS Code: {}", e))?;
    Ok(())
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
fn spawn_shell(
    id: String,
    working_dir: String,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.pty_manager.spawn_shell(&id, &working_dir, app.clone())
}

#[tauri::command]
fn spawn_task(
    id: String,
    command: String,
    working_dir: String,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.pty_manager.spawn_task(&id, &command, &working_dir, app.clone())
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
    extra_flags: Option<Vec<String>>,
    nono_profile: Option<String>,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory
    let (work_dir, actual_branch, is_wt) = if let Some(wt_path) = worktree_path {
        // Use provided worktree path — detect branch from the directory
        let br =
            branch.or_else(|| get_current_branch(&wt_path)).unwrap_or_else(|| "main".to_string());
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

    // Merge settings flags with per-session extra flags
    let mut all_flags = settings.additional_flags.clone();
    if let Some(ef) = extra_flags {
        all_flags.extend(ef);
    }

    rlog!("Creating session '{}' (id={}) in '{}'", name, session_id, work_dir);
    rlog!("  branch={}, flags={:?}, claude_binary={:?}", actual_branch, all_flags, settings.claude_binary_path);

    // Spawn PTY
    let spawn_result = state.pty_manager.spawn(
        &session_id,
        &work_dir,
        settings.default_model.as_deref(),
        &all_flags,
        nono_profile.as_deref(),
        settings.claude_binary_path.as_deref(),
        app.clone(),
    );

    // Rollback worktree on spawn failure
    if let Err(ref e) = spawn_result {
        rlog!("Session spawn failed: {}", e);
        if is_wt {
            let _ = worktree::remove_worktree(&work_dir);
        }
        return Err(e.clone());
    }
    rlog!("Session '{}' spawned successfully", session_id);

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

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

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ClaudeSession {
    session_id: String,
    summary: String,
    modified_at: u64,
}

#[tauri::command]
fn list_claude_sessions(cwd: String) -> Result<Vec<ClaudeSession>, String> {
    use std::io::BufRead;

    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let projects_dir = home.join(".claude").join("projects");

    // Claude encodes the path by replacing / and . with -
    let encoded = cwd.replace('/', "-").replace('.', "-");
    let project_dir = projects_dir.join(&encoded);

    if !project_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in std::fs::read_dir(&project_dir).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("jsonl") {
            continue;
        }
        let session_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        let modified_at = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Read first user message as summary
        let summary = (|| -> Option<String> {
            let file = std::fs::File::open(&path).ok()?;
            let reader = std::io::BufReader::new(file);
            for line in reader.lines() {
                let line = line.ok()?;
                if !line.contains("\"type\":\"user\"") {
                    continue;
                }
                let val: serde_json::Value = serde_json::from_str(&line).ok()?;
                let content = val.get("message")?.get("content")?;
                if let Some(s) = content.as_str() {
                    return Some(s.chars().take(120).collect());
                }
                if let Some(arr) = content.as_array() {
                    for item in arr {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                return Some(text.chars().take(120).collect());
                            }
                        }
                    }
                }
                return None;
            }
            None
        })()
        .unwrap_or_default();

        sessions.push(ClaudeSession { session_id, summary, modified_at });
    }

    sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(sessions)
}

#[tauri::command]
fn check_setup_needed() -> bool {
    !hooks::cli_is_installed()
}

#[tauri::command]
fn run_setup() -> Result<(), String> {
    hooks::install_hooks()
}

#[tauri::command]
fn check_nono_installed() -> bool {
    let user_path = pty::get_user_path();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    std::process::Command::new(&shell)
        .args(["-c", "command -v nono"])
        .env("PATH", &user_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tauri::command]
fn list_nono_profiles() -> Vec<String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let profiles_dir = home.join(".config").join("nono").join("profiles");
    if !profiles_dir.is_dir() {
        return Vec::new();
    }
    let mut profiles = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // Use file stem (without extension) as the profile name
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                profiles.push(name.to_string());
            }
        }
    }
    profiles.sort();
    profiles
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

#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))
}

#[tauri::command]
fn write_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, &contents).map_err(|e| format!("Failed to write file: {}", e))
}

#[tauri::command]
fn list_docs(dir: String) -> Result<Vec<DocFile>, String> {
    use std::path::Path;

    let base = Path::new(&dir);
    if !base.is_dir() {
        return Err(format!("Not a directory: {}", dir));
    }

    let skip_dirs: std::collections::HashSet<&str> =
        ["node_modules", ".git", "target", "dist", ".svelte-kit", ".superpowers"]
            .iter()
            .copied()
            .collect();

    let mut docs = Vec::new();
    let mut stack = vec![base.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !skip_dirs.contains(name) {
                        stack.push(path);
                    }
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let modified = path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let relative =
                    path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();

                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

                docs.push(DocFile {
                    path: path.to_string_lossy().to_string(),
                    name,
                    relative_path: relative,
                    modified,
                });
            }
        }
    }

    docs.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(docs)
}

fn main() {
    let initial_settings = settings::load_settings();
    logging::init(initial_settings.enable_logging);
    rlog!("Settings loaded from {:?}", dirs::config_dir().map(|d| d.join("roux/settings.json")));
    if let Some(ref p) = initial_settings.claude_binary_path {
        rlog!("Claude binary path (from settings): {}", p);
    } else {
        rlog!("Claude binary path: (default, resolved via PATH)");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            settings: Mutex::new(initial_settings),
            pty_manager: PtyManager::new(),
            session_store: SessionStore::load_persisted(),
        })
        .invoke_handler(tauri::generate_handler![
            get_log_path,
            frontend_log,
            get_settings,
            update_settings,
            cmd_create_worktree,
            cmd_remove_worktree,
            cmd_list_worktrees,
            write_to_session,
            resize_session,
            spawn_shell,
            spawn_task,
            kill_session,
            create_session,
            list_sessions,
            list_claude_sessions,
            read_file,
            write_file,
            list_docs,
            cmd_open_in_editor,
            cmd_list_branches,
            check_setup_needed,
            run_setup,
            check_nono_installed,
            list_nono_profiles,
            tasks::cmd_discover_tasks,
            tasks::cmd_load_task_overrides,
            tasks::cmd_save_task_overrides,
        ])
        .setup(|app| {
            // Only auto-update hooks if CLI is already installed (not first run).
            // First-run install is handled by the frontend setup prompt.
            if hooks::cli_is_installed() {
                if let Err(e) = hooks::install_hooks() {
                    eprintln!("Warning: failed to install hooks: {}", e);
                }
            }
            if let Err(e) = status_watcher::start_watching(app.handle().clone()) {
                eprintln!("Warning: failed to start status watcher: {}", e);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Emit to frontend — it will decide whether to close a pane or the window
                let app = window.app_handle();
                let _ = app.emit("close-requested", ());
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
