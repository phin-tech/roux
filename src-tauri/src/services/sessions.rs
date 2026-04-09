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

pub(crate) async fn create_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    repo_path: &str,
    name: &str,
    worktree_path: Option<&str>,
    branch: Option<&str>,
    extra_flags: &[String],
    nono_profile: Option<&str>,
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory
    let (work_dir, actual_branch, is_wt) = if let Some(wt_path) = worktree_path {
        let br = branch
            .map(|b| b.to_string())
            .or_else(|| get_current_branch(wt_path))
            .unwrap_or_else(|| "main".to_string());
        (wt_path.to_string(), br, false)
    } else if let Some(br) = branch {
        let base = settings.worktree_base_path.as_deref();
        let wt_path = crate::worktree::create_worktree(repo_path, br, base)?;
        (wt_path, br.to_string(), true)
    } else {
        let br = get_current_branch(repo_path).unwrap_or_else(|| "main".to_string());
        (repo_path.to_string(), br, false)
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
        status: "idle".to_string(),
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

    session_handle.update_status(id, "idle").await?;

    rlog!("Session '{}' reconnected successfully", id);

    let mut updated = session;
    updated.status = "idle".to_string();
    Ok(updated)
}

pub(crate) async fn kill_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    id: &str,
) -> anyhow::Result<()> {
    pty_manager.kill(id);
    session_handle.remove(id).await?;
    Ok(())
}

#[derive(serde::Serialize, Clone)]
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
