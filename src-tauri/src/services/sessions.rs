use anyhow::anyhow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::paths::default_notes_vault_root;
use crate::pty::{NotesEnvInputs, PtyManager};
use crate::services::notes::{self as notes_svc, NotesService};
use crate::session::Session;
use crate::session_service::SessionHandle;
use crate::settings::RouxSettings;

/// Build the `NotesEnvInputs` for a brand-new session that hasn't been
/// assigned a project yet. Project slug stays `None` until the user
/// assigns one (at which point a reconnect refreshes the env).
fn build_notes_env_for_new_session(
    settings: &RouxSettings,
    session_id: &str,
    branch: &str,
    repo_path: &str,
) -> NotesEnvInputs {
    let vault_root_path = settings
        .notes_vault_root
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_vault_root);
    let mut svc = NotesService::new(vault_root_path.clone());
    let remote = git_origin_url(repo_path);
    let repo_slug = svc.freeze_repo_slug(repo_path, remote.as_deref());
    NotesEnvInputs {
        vault_root: vault_root_path.to_string_lossy().into_owned(),
        session_slug: notes_svc::session_slug(branch, session_id),
        repo_slug,
        project_slug: None,
    }
}

/// Build `NotesEnvInputs` for an existing session (used on reconnect).
/// If the session has a `project_id`, the project name is looked up
/// and the project slug frozen in the vault index.
async fn build_notes_env_for_existing_session(
    settings: &RouxSettings,
    project_handle: &crate::project_service::ProjectHandle,
    session: &Session,
) -> NotesEnvInputs {
    let vault_root_path = settings
        .notes_vault_root
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_vault_root);
    let mut svc = NotesService::new(vault_root_path.clone());
    let remote = git_origin_url(&session.repo_root);
    let repo_slug = svc.freeze_repo_slug(&session.repo_root, remote.as_deref());

    let project_slug = match session.project_id.as_deref() {
        Some(pid) => match project_handle.list().await.ok() {
            Some(projects) => projects
                .into_iter()
                .find(|p| p.id == pid)
                .map(|p| svc.freeze_project_slug(pid, &p.name)),
            None => None,
        },
        None => None,
    };

    NotesEnvInputs {
        vault_root: vault_root_path.to_string_lossy().into_owned(),
        session_slug: notes_svc::session_slug(&session.branch, &session.id),
        repo_slug,
        project_slug,
    }
}

fn git_origin_url(repo_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if url.is_empty() { None } else { Some(url) }
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

/// Describes how to resolve the working directory for a new session.
pub(crate) enum SessionTarget<'a> {
    /// Use the repo directory directly (no worktree).
    Repo,
    /// Use an existing worktree path. The session does NOT own this worktree.
    ExistingWorktree { path: &'a str },
    /// Create a new worktree from a branch. The session owns this worktree.
    /// When `start_point` is `Some(sp)` and the branch is new, the branch is
    /// created from `sp` instead of HEAD. When `fetch_first` is true, we
    /// `git fetch origin` before resolving the start point (used for
    /// `origin/main`-style bases that may be stale).
    NewWorktree {
        branch: &'a str,
        start_point: Option<&'a str>,
        fetch_first: bool,
    },
}

/// Create a session with a plain shell in its primary PTY. The shell is
/// optionally wrapped in `nono run` via [`NonoConfig`]. The frontend
/// attaches a spawn profile and writes setup / startup commands into the
/// shell after it comes up. This is the one and only session creation
/// path.
///
/// Settings-aware for `worktree_base_path` so new worktrees land in the
/// user's configured base directory.
pub(crate) async fn create_session_shell(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    repo_path: &str,
    name: &str,
    target: SessionTarget<'_>,
    nono: Option<&crate::pty::NonoConfig>,
    profile: Option<&str>,
    initial_size: Option<(u16, u16)>,
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory based on session target.
    let (work_dir, actual_branch, is_wt) = match target {
        SessionTarget::ExistingWorktree { path } => {
            let br = get_current_branch(path).unwrap_or_else(|| "main".to_string());
            (path.to_string(), br, false)
        }
        SessionTarget::NewWorktree { branch, start_point, fetch_first } => {
            if fetch_first {
                crate::worktree::fetch_origin(repo_path)?;
            }
            let base = settings.worktree_base_path.as_deref();
            let wt_path =
                crate::worktree::create_worktree(repo_path, branch, base, start_point)?;
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
    let worktree_env = if is_wt { Some(work_dir.as_str()) } else { None };
    let notes_env = Some(build_notes_env_for_new_session(
        settings,
        &session_id,
        &actual_branch,
        repo_path,
    ));
    let spawn_result = pty_manager.spawn_shell(
        &session_id,
        &work_dir,
        Some(&session_id),
        Some(&pane_id),
        None,
        worktree_env,
        notes_env.as_ref(),
        nono,
        initial_size,
        crate::pty::PtyRole::SessionPrimary,
        profile,
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

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    let session = Session {
        id: session_id.clone(),
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
        name_override: None,
        primary_pty_id: Some(session_id),
        archived: false,
        ended_at: None,
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

/// Reconnect a session by respawning its primary shell PTY, optionally
/// nono-wrapped. The frontend re-runs the pane's profile commands into
/// the fresh shell after this call, so agents come back up by typing
/// their startup command. Kills the old PTY first so the session id is
/// free for a fresh `spawn_shell`.
pub(crate) async fn reconnect_session_shell(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    project_handle: &crate::project_service::ProjectHandle,
    settings: &RouxSettings,
    id: &str,
    nono: Option<&crate::pty::NonoConfig>,
    profile: Option<&str>,
    initial_size: Option<(u16, u16)>,
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session =
        session_handle.get(id).await?.ok_or_else(|| anyhow!("Session {} not found", id))?;

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
    let worktree_env = if session.is_worktree { Some(session.worktree_path.as_str()) } else { None };
    let notes_env =
        Some(build_notes_env_for_existing_session(settings, project_handle, &session).await);
    pty_manager
        .spawn_shell(
            id,
            &session.worktree_path,
            Some(id),
            Some(&pane_id),
            session.project_id.as_deref(),
            worktree_env,
            notes_env.as_ref(),
            nono,
            initial_size,
            crate::pty::PtyRole::SessionPrimary,
            profile,
            app.clone(),
        )
        .map_err(|e| anyhow!("{}", e))?;

    session_handle.update_status(id, roux_core::SessionStatus::Idle).await?;

    rlog!("Shell session '{}' reconnected successfully", id);

    let mut updated = session;
    updated.status = roux_core::SessionStatus::Idle;
    Ok(updated)
}

/// Archive a session: kill its PTYs and flip the `archived` flag so the
/// record stays on disk for the history view. The Tauri command name is
/// still `kill_session` for frontend backward-compat — the behavior changed
/// from hard-delete to soft-archive when the sessions-history pane shipped.
///
/// Worktree cleanup is **not** done here — the close dialog archives only
/// (worktree always kept). Users remove the worktree later from the History
/// pane via the Clean worktree action.
pub(crate) async fn kill_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    id: &str,
) -> anyhow::Result<()> {
    pty_manager.kill_session_ptys(id);
    session_handle.archive(id).await?;
    if let Err(e) = crate::pane_state::delete_pane_state(id) {
        rlog!("kill_session: failed to delete pane state for {id}: {e}");
    }
    Ok(())
}

/// Bring an archived session back to the active list. Status is normalized
/// to `Disconnected`; the existing reconnect flow attaches a fresh PTY on
/// first open.
pub(crate) async fn restore_session(
    session_handle: &SessionHandle,
    id: &str,
) -> anyhow::Result<()> {
    session_handle.restore(id).await?;
    session_handle.update_status(id, roux_core::SessionStatus::Disconnected).await?;
    Ok(())
}

/// Permanently delete a session record. Does not touch the worktree —
/// worktree handling is explicit from the History pane.
pub(crate) async fn delete_session_permanently(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    id: &str,
) -> anyhow::Result<()> {
    pty_manager.kill_session_ptys(id);
    session_handle.remove(id).await?;
    if let Err(e) = crate::pane_state::delete_pane_state(id) {
        rlog!("delete_session_permanently: failed to delete pane state for {id}: {e}");
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

    let encoded = cwd.replace(['/', '.'], "-");
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

    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified_at));
    Ok(sessions)
}

pub(crate) fn list_git_repos_in_roots(roots: &[String], exclude_worktrees: bool) -> Vec<String> {
    let mut repos = BTreeSet::new();
    for root in roots {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        let root_path = PathBuf::from(trimmed);
        if !root_path.is_dir() {
            continue;
        }
        collect_git_repos(&root_path, 3, exclude_worktrees, &mut repos);
    }
    repos.into_iter().collect()
}

fn collect_git_repos(
    start: &Path,
    max_depth: usize,
    exclude_worktrees: bool,
    out: &mut BTreeSet<String>,
) {
    let mut stack = vec![(start.to_path_buf(), 0usize)];
    while let Some((path, depth)) = stack.pop() {
        if is_git_dir(&path) {
            if exclude_worktrees && is_git_worktree(&path) {
                continue;
            }
            out.insert(path.to_string_lossy().to_string());
            continue;
        }
        if depth >= max_depth {
            continue;
        }
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let child = entry.path();
            if !child.is_dir() {
                continue;
            }
            if should_skip_dir(&child) {
                continue;
            }
            stack.push((child, depth + 1));
        }
    }
}

fn is_git_dir(path: &Path) -> bool {
    let dot_git = path.join(".git");
    dot_git.is_dir() || dot_git.is_file()
}

fn is_git_worktree(path: &Path) -> bool {
    let dot_git = path.join(".git");
    if !dot_git.is_file() {
        return false;
    }
    let content = match std::fs::read_to_string(dot_git) {
        Ok(content) => content,
        Err(_) => return false,
    };
    let Some(gitdir) = content.strip_prefix("gitdir:") else {
        return false;
    };
    let normalized = gitdir.trim().replace('\\', "/");
    normalized.contains("/worktrees/")
}

fn should_skip_dir(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return true,
    };
    matches!(name, ".git" | "node_modules" | "target" | "dist" | ".svelte-kit" | ".next")
}

#[cfg(test)]
mod tests {
    use super::list_git_repos_in_roots;

    #[test]
    fn list_git_repos_in_roots_finds_nested_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("src");
        std::fs::create_dir_all(root.join("org1/repo-a/.git")).unwrap();
        std::fs::create_dir_all(root.join("org2/repo-b/.git")).unwrap();

        let repos = list_git_repos_in_roots(&[root.to_string_lossy().to_string()], false);

        assert_eq!(repos.len(), 2);
        assert!(repos.iter().any(|p| p.ends_with("org1/repo-a")));
        assert!(repos.iter().any(|p| p.ends_with("org2/repo-b")));
    }

    #[test]
    fn list_git_repos_in_roots_dedupes_and_ignores_invalid_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repos");
        std::fs::create_dir_all(root.join("repo/.git")).unwrap();

        let root_str = root.to_string_lossy().to_string();
        let missing = tmp.path().join("missing").to_string_lossy().to_string();
        let repos =
            list_git_repos_in_roots(&["".to_string(), root_str.clone(), root_str, missing], false);

        assert_eq!(repos.len(), 1);
        assert!(repos[0].ends_with("repos/repo"));
    }

    #[test]
    fn list_git_repos_in_roots_excludes_worktree_repos_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("src");

        std::fs::create_dir_all(root.join("repo-main/.git")).unwrap();
        std::fs::create_dir_all(root.join("repo-wt")).unwrap();
        std::fs::write(
            root.join("repo-wt/.git"),
            "gitdir: /tmp/project/.git/worktrees/repo-wt\n",
        )
        .unwrap();

        let roots = [root.to_string_lossy().to_string()];
        let included = list_git_repos_in_roots(&roots, false);
        let excluded = list_git_repos_in_roots(&roots, true);

        assert!(included.iter().any(|p| p.ends_with("repo-main")));
        assert!(included.iter().any(|p| p.ends_with("repo-wt")));
        assert!(excluded.iter().any(|p| p.ends_with("repo-main")));
        assert!(!excluded.iter().any(|p| p.ends_with("repo-wt")));
    }
}
