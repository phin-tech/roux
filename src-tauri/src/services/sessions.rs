use anyhow::anyhow;

use crate::pty::PtyManager;
use crate::session::Session;
use crate::session_service::SessionHandle;
use crate::settings::RouxSettings;

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

/// Describes how to resolve the working directory for a new session.
pub(crate) enum SessionTarget<'a> {
    /// Use the repo directory directly (no worktree).
    Repo,
    /// Use an existing worktree path. The session does NOT own this worktree.
    ExistingWorktree { path: &'a str },
    /// Create a new worktree from a branch. The session owns this worktree.
    NewWorktree { branch: &'a str },
}

pub(crate) async fn create_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    repo_path: &str,
    name: &str,
    target: SessionTarget<'_>,
    extra_flags: &[String],
    nono_profile: Option<&str>,
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory based on session target
    let (work_dir, actual_branch, is_wt) = match target {
        SessionTarget::ExistingWorktree { path } => {
            let br = get_current_branch(path).unwrap_or_else(|| "main".to_string());
            (path.to_string(), br, false)
        }
        SessionTarget::NewWorktree { branch } => {
            let base = settings.worktree_base_path.as_deref();
            let wt_path = crate::worktree::create_worktree(repo_path, branch, base)?;
            (wt_path, branch.to_string(), true)
        }
        SessionTarget::Repo => {
            let br = get_current_branch(repo_path).unwrap_or_else(|| "main".to_string());
            (repo_path.to_string(), br, false)
        }
    };

    // Merge settings flags with per-session extra flags
    let mut all_flags = settings.additional_flags.clone();
    all_flags.extend_from_slice(extra_flags);

    rlog!("Creating session '{}' (id={}) in '{}'", name, session_id, work_dir);
    rlog!(
        "  branch={}, flags={:?}, claude_binary={:?}",
        actual_branch,
        all_flags,
        settings.claude_binary_path
    );

    // Spawn PTY
    let spawn_result = pty_manager.spawn(
        &session_id,
        &work_dir,
        settings.default_model.as_deref(),
        &all_flags,
        nono_profile,
        settings.claude_binary_path.as_deref(),
        app.clone(),
    );

    if let Err(e) = spawn_result {
        rlog!("Session spawn failed: {}", e);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&work_dir);
        }
        return Err(anyhow!("{}", e));
    }
    rlog!("Session '{}' spawned successfully", session_id);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let session = Session {
        id: session_id,
        name: name.to_string(),
        repo_root: repo_path.to_string(),
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: is_wt,
        status: roux_core::SessionStatus::Idle,
        model: None,
        cost: None,
        created_at: now,
        project_id: None,
        is_git_repo: is_git_repo(repo_path),
    };

    if let Err(e) = session_handle.add(session.clone()).await {
        pty_manager.kill(&session.id);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&session.worktree_path);
        }
        return Err(e.into());
    }
    Ok(session)
}

/// Create a session that hosts a **plain shell** in its primary PTY,
/// instead of the claude binary. The frontend attaches a spawn profile and
/// writes setup / startup commands into the shell after it comes up. Used
/// for every non-Claude profile in the new-session picker.
///
/// Parallel shape to [`create_session`] minus the Claude-specific inputs
/// (default model, additional flags, nono profile, claude binary path),
/// but settings-aware for anything non-Claude-specific — notably
/// `worktree_base_path` so new worktrees land in the user's configured
/// base directory regardless of which profile spawned them.
/// Emits the same Session record so the rest of the app doesn't need to
/// know which creation path was used.
pub(crate) async fn create_session_shell(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    repo_path: &str,
    name: &str,
    target: SessionTarget<'_>,
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory based on session target. Same logic as
    // create_session; cheaper than extracting a helper because the shape
    // is small and the two callers diverge quickly after this step.
    let (work_dir, actual_branch, is_wt) = match target {
        SessionTarget::ExistingWorktree { path } => {
            let br = get_current_branch(path).unwrap_or_else(|| "main".to_string());
            (path.to_string(), br, false)
        }
        SessionTarget::NewWorktree { branch } => {
            let base = settings.worktree_base_path.as_deref();
            let wt_path = crate::worktree::create_worktree(repo_path, branch, base)?;
            (wt_path, branch.to_string(), true)
        }
        SessionTarget::Repo => {
            let br = get_current_branch(repo_path).unwrap_or_else(|| "main".to_string());
            (repo_path.to_string(), br, false)
        }
    };

    rlog!(
        "Creating shell session '{}' (id={}) in '{}' (branch={})",
        name,
        session_id,
        work_dir,
        actual_branch,
    );

    // The session's primary pane id matches the frontend's formula (see
    // actions.ts::initSession). Passing both ids into the PTY env keeps
    // tier-1 hook routing happy the moment the user starts an agent.
    let pane_id = format!("{}-main", session_id);
    let spawn_result = pty_manager.spawn_shell(
        &session_id,
        &work_dir,
        Some(&session_id),
        Some(&pane_id),
        app.clone(),
    );

    if let Err(e) = spawn_result {
        rlog!("Shell session spawn failed: {}", e);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&work_dir);
        }
        return Err(anyhow!("{}", e));
    }
    rlog!("Shell session '{}' spawned", session_id);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let session = Session {
        id: session_id,
        name: name.to_string(),
        repo_root: repo_path.to_string(),
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: is_wt,
        status: roux_core::SessionStatus::Idle,
        model: None,
        cost: None,
        created_at: now,
        project_id: None,
        is_git_repo: is_git_repo(repo_path),
    };

    if let Err(e) = session_handle.add(session.clone()).await {
        pty_manager.kill(&session.id);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&session.worktree_path);
        }
        return Err(e.into());
    }
    Ok(session)
}

pub(crate) async fn reconnect_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    id: &str,
    extra_flags: &[String],
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session = session_handle
        .get(id)
        .await?
        .ok_or_else(|| anyhow!("Session {} not found", id))?;

    pty_manager.kill(id);

    let mut all_flags = settings.additional_flags.clone();
    all_flags.extend_from_slice(extra_flags);

    rlog!("Reconnecting session '{}' (id={}) in '{}'", session.name, id, session.worktree_path);

    pty_manager
        .spawn(
            id,
            &session.worktree_path,
            settings.default_model.as_deref(),
            &all_flags,
            None,
            settings.claude_binary_path.as_deref(),
            app.clone(),
        )
        .map_err(|e| anyhow!("{}", e))?;

    session_handle.update_status(id, roux_core::SessionStatus::Idle).await?;

    rlog!("Session '{}' reconnected successfully", id);

    let mut updated = session;
    updated.status = roux_core::SessionStatus::Idle;
    Ok(updated)
}

/// Reconnect a session by respawning a **plain shell** in its primary PTY,
/// without running the claude binary directly. Used by any session whose
/// primary pane was originally created via `create_session_shell` — i.e.
/// every non-Claude-builtin profile (Codex, Plain shell, user profiles,
/// inline Custom…). The frontend re-runs the pane's profile commands
/// into the fresh shell after this call, so agents like Codex come back
/// up by typing their startup command rather than being re-execed
/// directly by the backend.
///
/// Parallel to [`reconnect_session`] minus the Claude-specific inputs.
/// Both functions kill the old PTY first so the session id is free for a
/// fresh `spawn`/`spawn_shell`.
pub(crate) async fn reconnect_session_shell(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    id: &str,
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session = session_handle
        .get(id)
        .await?
        .ok_or_else(|| anyhow!("Session {} not found", id))?;

    pty_manager.kill(id);

    rlog!(
        "Reconnecting shell session '{}' (id={}) in '{}'",
        session.name,
        id,
        session.worktree_path,
    );

    // Same env-injection contract as create_session_shell: primary pane
    // id matches the frontend's formula so tier-1 hook routing stays
    // deterministic the moment the user starts an agent in the shell.
    let pane_id = format!("{}-main", id);
    pty_manager
        .spawn_shell(
            id,
            &session.worktree_path,
            Some(id),
            Some(&pane_id),
            app.clone(),
        )
        .map_err(|e| anyhow!("{}", e))?;

    session_handle
        .update_status(id, roux_core::SessionStatus::Idle)
        .await?;

    rlog!("Shell session '{}' reconnected successfully", id);

    let mut updated = session;
    updated.status = roux_core::SessionStatus::Idle;
    Ok(updated)
}

pub(crate) async fn kill_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    id: &str,
) -> anyhow::Result<()> {
    pty_manager.kill(id);
    session_handle.remove(id).await?;
    // Best-effort: remove per-session pane state file. Non-fatal if it fails.
    if let Err(e) = crate::pane_state::delete_pane_state(id) {
        rlog!("kill_session: failed to delete pane state for {id}: {e}");
    }
    Ok(())
}

pub(crate) async fn refresh_git_status(
    session_handle: &SessionHandle,
    id: &str,
) -> anyhow::Result<bool> {
    let session = session_handle.get(id).await?;
    if let Some(s) = session {
        let is_git = is_git_repo(&s.worktree_path);
        if is_git != s.is_git_repo {
            session_handle.set_git_repo(id, is_git).await?;
        }
        Ok(is_git)
    } else {
        Ok(false)
    }
}

#[derive(serde::Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeSession {
    pub(crate) session_id: String,
    pub(crate) summary: String,
    pub(crate) modified_at: u64,
}

pub(crate) fn list_claude_sessions(cwd: &str) -> anyhow::Result<Vec<ClaudeSession>> {
    use std::io::BufRead;

    let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot find home directory"))?;
    let projects_dir = home.join(".claude").join("projects");

    let encoded = cwd.replace('/', "-").replace('.', "-");
    let project_dir = projects_dir.join(&encoded);

    if !project_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in std::fs::read_dir(&project_dir)? {
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
