use crate::session::Session;
use crate::state::AppState;

// Note: spec says Vec<u8> but xterm.js onData sends UTF-8 strings.
// We accept String and convert to bytes server-side for simplicity.
#[tauri::command]
pub(crate) fn write_to_session(id: String, data: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.write(&id, data.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn resize_session(
    id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.pty_manager.resize(&id, cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn attach_pty_output(
    id: String,
    on_event: tauri::ipc::Channel<tauri::ipc::Response>,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.pty_manager.attach_output_channel(&id, on_event);
    Ok(())
}

#[tauri::command]
pub(crate) fn spawn_shell(
    id: String,
    working_dir: String,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.pty_manager.spawn_shell(&id, &working_dir, None, app.clone()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn spawn_task(
    id: String,
    command: String,
    working_dir: String,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state
        .pty_manager
        .spawn_task(&id, &command, &working_dir, None, app.clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn kill_session(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.pty_manager.kill(&id);
    let handle = state.session_handle.clone();
    handle.remove(&id).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_pty_generation(id: String, state: tauri::State<AppState>) -> Option<u64> {
    state.pty_manager.get_generation(&id)
}

#[tauri::command]
pub(crate) async fn create_session(
    repo_path: String,
    name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    extra_flags: Option<Vec<String>>,
    nono_profile: Option<String>,
    state: tauri::State<'_, AppState>,
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
        let wt_path =
            crate::worktree::create_worktree(&repo_path, &br, base).map_err(|e| e.to_string())?;
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
    rlog!(
        "  branch={}, flags={:?}, claude_binary={:?}",
        actual_branch,
        all_flags,
        settings.claude_binary_path
    );

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
    if let Err(e) = spawn_result {
        let message = e.to_string();
        rlog!("Session spawn failed: {}", message);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&work_dir);
        }
        return Err(message);
    }
    rlog!("Session '{}' spawned successfully", session_id);

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    let session = Session {
        id: session_id,
        name,
        repo_root: repo_path.clone(),
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: is_wt,
        status: "idle".to_string(),
        model: None,
        cost: None,
        created_at: now,
        project_id: None,
        is_git_repo: is_git_repo(&repo_path),
    };

    let handle = state.session_handle.clone();
    if let Err(e) = handle.add(session.clone()).await {
        // Rollback: kill the PTY we just spawned and remove worktree if we created one
        state.pty_manager.kill(&session.id);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&session.worktree_path);
        }
        return Err(e.to_string());
    }
    Ok(session)
}

#[tauri::command]
pub(crate) async fn reconnect_session(
    id: String,
    extra_flags: Option<Vec<String>>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let handle = state.session_handle.clone();
    let session = handle
        .get(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session {} not found", id))?;

    let settings = state.settings.lock().unwrap().clone();

    // Kill existing PTY (ignore errors — it may already be dead)
    state.pty_manager.kill(&id);

    // Merge settings flags with per-call extra flags
    let mut all_flags = settings.additional_flags.clone();
    if let Some(ef) = extra_flags {
        all_flags.extend(ef);
    }

    rlog!("Reconnecting session '{}' (id={}) in '{}'", session.name, id, session.worktree_path);

    // Spawn new Claude PTY under the same session ID
    state
        .pty_manager
        .spawn(
            &id,
            &session.worktree_path,
            settings.default_model.as_deref(),
            &all_flags,
            None,
            settings.claude_binary_path.as_deref(),
            app.clone(),
        )
        .map_err(|e| e.to_string())?;

    // Update status to idle
    handle.update_status(&id, "idle").await.map_err(|e| e.to_string())?;

    rlog!("Session '{}' reconnected successfully", id);

    // Return the session with updated status
    let mut updated = session;
    updated.status = "idle".to_string();
    Ok(updated)
}

#[tauri::command]
pub(crate) async fn list_sessions(state: tauri::State<'_, AppState>) -> Result<Vec<Session>, String> {
    let handle = state.session_handle.clone();
    Ok(handle.list().await.map_err(|e| e.to_string())?)
}

#[tauri::command]
pub(crate) async fn refresh_session_git_status(id: String, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let handle = state.session_handle.clone();
    let session = handle.get(&id).await.map_err(|e| e.to_string())?;
    if let Some(s) = session {
        let is_git = is_git_repo(&s.worktree_path);
        if is_git != s.is_git_repo {
            handle.set_git_repo(&id, is_git).await.map_err(|e| e.to_string())?;
        }
        Ok(is_git)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub(crate) fn check_is_git_repo(path: String) -> bool {
    is_git_repo(&path)
}

pub(crate) fn is_git_repo(path: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub(crate) fn get_current_branch(repo_path: &str) -> Option<String> {
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

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeSession {
    session_id: String,
    summary: String,
    modified_at: u64,
}

#[tauri::command]
pub(crate) fn list_claude_sessions(cwd: String) -> Result<Vec<ClaudeSession>, String> {
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
