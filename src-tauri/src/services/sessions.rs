use anyhow::anyhow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::automation_hooks::{
    worktree_provider_hooks, AutomationHookManager, HookContext, HookEvent,
};
use crate::paths::default_notes_vault_root;
use crate::project_service::ProjectHandle;
use crate::pty::{NotesEnvInputs, PtyManager, SmolvmExec};
use crate::services::notes::{self as notes_svc, NotesService};
use crate::session::Session;
use crate::session_service::SessionHandle;
use crate::settings::RouxSettings;
use roux_core::Project;

/// Build a [`SmolvmExec`] for a session that's bound to a smol machine.
/// Returns `Ok(None)` for unbound sessions — the common case.
///
/// When the session field is set but `smolvm` isn't installed (or the
/// configured override path no longer resolves), this returns an error
/// instead of silently falling back to a host shell. The session was
/// explicitly bound; running it outside the VM would surprise the user.
/// To unbind, invoke the `set_session_smol_machine` Tauri command with
/// `machine_name: None`.
fn build_smolvm_exec_for_session(
    smol_machine_name: Option<&str>,
) -> anyhow::Result<Option<SmolvmExec>> {
    let Some(name) = smol_machine_name else {
        return Ok(None);
    };
    let install = crate::services::smolvm::resolve_smolvm_binary().ok_or_else(|| {
        anyhow!(
            "session is bound to smol machine '{name}', but smolvm is not installed; \
             unbind via the panel or install smolvm to continue"
        )
    })?;
    Ok(Some(SmolvmExec {
        binary: install.path,
        machine_name: name.to_string(),
        // v1 hardcodes /bin/sh — POSIX-guaranteed in any reasonable
        // Linux guest. Stretch tier may add a per-session override.
        guest_shell: "/bin/sh".to_string(),
    }))
}

/// Build the `NotesEnvInputs` for a brand-new session. When `project` is
/// supplied (the blueprint-spawn path), the project slug + context paths
/// are baked in on the very first PTY spawn so the child shell sees the
/// env vars immediately. Otherwise project_slug stays `None` until the
/// user assigns one (at which point a reconnect refreshes the env).
pub(crate) fn build_notes_env_for_new_session(
    settings: &RouxSettings,
    session_id: &str,
    branch: &str,
    repo_path: &str,
    project: Option<&Project>,
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
    let (project_slug, context_paths, project_prompt) = match project {
        Some(p) => (
            Some(svc.freeze_project_slug(&p.id, &p.name)),
            p.context_paths.clone(),
            p.project_prompt.clone(),
        ),
        None => (None, Vec::new(), String::new()),
    };
    NotesEnvInputs {
        vault_root: vault_root_path.to_string_lossy().into_owned(),
        session_slug: notes_svc::session_slug(branch, session_id),
        repo_slug,
        project_slug,
        context_paths,
        project_prompt,
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

    let (project_slug, context_paths, project_prompt) = match session.project_id.as_deref() {
        Some(pid) => match project_handle.list().await.ok() {
            Some(projects) => match projects.into_iter().find(|p| p.id == pid) {
                Some(p) => {
                    (Some(svc.freeze_project_slug(pid, &p.name)), p.context_paths, p.project_prompt)
                }
                None => (None, Vec::new(), String::new()),
            },
            None => (None, Vec::new(), String::new()),
        },
        None => (None, Vec::new(), String::new()),
    };

    NotesEnvInputs {
        vault_root: vault_root_path.to_string_lossy().into_owned(),
        session_slug: notes_svc::session_slug(&session.branch, &session.id),
        repo_slug,
        project_slug,
        context_paths,
        project_prompt,
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
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
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
    NewWorktree { branch: &'a str, start_point: Option<&'a str>, fetch_first: bool },
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
    project_handle: &ProjectHandle,
    settings: &RouxSettings,
    repo_path: &str,
    name: &str,
    target: SessionTarget<'_>,
    nono: Option<&crate::pty::NonoConfig>,
    profile: Option<&str>,
    initial_size: Option<(u16, u16)>,
    project_id: Option<&str>,
    blueprint_id: Option<&str>,
    smol_machine_name: Option<&str>,
    hooks: Option<&AutomationHookManager>,
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
            // Route through the provider-aware create so right-click →
            // New Worktree honors the user's WorktreeProvider setting
            // (Auto / Git / Worktrunk). Without this, every entry point
            // except the Tauri `cmd_create_worktree` command silently
            // bypassed worktrunk and the "using wt" badge in the New
            // Session dialog was the only signal the provider was even
            // active.
            let wt = crate::services::setup::resolve_wt_binary();
            if let Some(hooks) = hooks {
                let context = HookContext {
                    repo_path: Some(repo_path.to_string()),
                    branch: Some(branch.to_string()),
                    cwd: Some(repo_path.to_string()),
                    ..HookContext::new(HookEvent::PreWorktreeCreate)
                        .with_provider(settings.worktree_provider, wt.is_some())
                };
                hooks.run_blocking(HookEvent::PreWorktreeCreate, context).await?;
            }
            let wt_path = roux_core::create_worktree_with_provider(
                repo_path,
                branch,
                base,
                start_point,
                settings.worktree_provider,
                wt.as_ref(),
            )?;
            if let Some(hooks) = hooks {
                let mut context = HookContext {
                    repo_path: Some(repo_path.to_string()),
                    worktree_path: Some(wt_path.clone()),
                    branch: Some(branch.to_string()),
                    cwd: Some(wt_path.clone()),
                    ..HookContext::new(HookEvent::PostWorktreeCreate)
                        .with_provider(settings.worktree_provider, wt.is_some())
                };
                context.provider_hooks_ran =
                    worktree_provider_hooks(HookEvent::PostWorktreeCreate, context.worktrunk);
                hooks.spawn_background(HookEvent::PostWorktreeCreate, context);
            }
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
    // Resolve the project up-front so the very first PTY spawn carries the
    // project notes + ROUX_PROJECT_CONTEXT_PATHS env vars when this session
    // was launched from a blueprint or otherwise pre-tagged with a project.
    let project_record = match project_id {
        Some(pid) => project_handle.get(pid).await.ok().flatten(),
        None => None,
    };
    let notes_env = Some(build_notes_env_for_new_session(
        settings,
        &session_id,
        &actual_branch,
        repo_path,
        project_record.as_ref(),
    ));
    // When the dialog (or socket bridge) provided a `smol_machine_name`,
    // build the SmolvmExec up-front so the very first PTY spawn lands
    // inside the VM. If smolvm got uninstalled between the dialog
    // populating its picker and now, surface that as a clean error
    // before we've created any worktree/session state.
    let smolvm = build_smolvm_exec_for_session(smol_machine_name)?;
    let spawn_result = pty_manager.spawn_shell(
        &session_id,
        &work_dir,
        Some(&session_id),
        Some(&pane_id),
        project_id,
        worktree_env,
        notes_env.as_ref(),
        nono,
        smolvm.as_ref(),
        initial_size,
        crate::pty::PtyRole::SessionPrimary,
        profile,
        app.clone(),
    );

    if let Err(e) = spawn_result {
        rlog!("Shell session spawn failed: {}", e);
        if is_wt {
            // Error-recovery rollback: stay on the native git path even
            // when the user's provider is Worktrunk. A failing pre-remove
            // hook or a lock error here would leave the user with a
            // stranded worktree + no session. Cleanup-always-succeeds
            // trumps hooks-always-fire for error paths.
            let _ = crate::worktree::remove_worktree(repo_path, &work_dir);
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
        project_id: project_id.map(|s| s.to_string()),
        is_git_repo: is_git_repo(repo_path),
        name_override: None,
        primary_pty_id: Some(session_id),
        archived: false,
        ended_at: None,
        blueprint_id: blueprint_id.map(|s| s.to_string()),
        pinned_pr_url: None,
        smol_machine_name: smol_machine_name.map(|s| s.to_string()),
    };

    if let Err(e) = session_handle.add(session.clone()).await {
        pty_manager.kill(&session.id);
        if is_wt {
            // Same rollback policy as the spawn-failure path above:
            // emergency cleanup stays native.
            let _ = crate::worktree::remove_worktree(&session.repo_root, &session.worktree_path);
        }
        return Err(e.into());
    }
    if let Some(hooks) = hooks {
        let context = HookContext {
            repo_path: Some(session.repo_root.clone()),
            worktree_path: Some(session.worktree_path.clone()),
            branch: Some(session.branch.clone()),
            session_id: Some(session.id.clone()),
            project_id: session.project_id.clone(),
            scope: Some("session".into()),
            cwd: Some(session.worktree_path.clone()),
            ..HookContext::new(HookEvent::PostSessionCreate)
        };
        hooks.spawn_background(HookEvent::PostSessionCreate, context);
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
    let worktree_env =
        if session.is_worktree { Some(session.worktree_path.as_str()) } else { None };
    let notes_env =
        Some(build_notes_env_for_existing_session(settings, project_handle, &session).await);

    // If this session is bound to a smol machine, build the SmolvmExec
    // here so the primary respawn lands inside the VM. A bound session
    // whose smolvm has been uninstalled fails clean rather than silently
    // running on host — see ground-truth note in `pty.rs`.
    let smolvm = build_smolvm_exec_for_session(session.smol_machine_name.as_deref())?;
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
            smolvm.as_ref(),
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
/// Pane state is kept so Restore can rebuild the archived session's layout;
/// permanent deletion still removes it.
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
    use super::{list_git_repos_in_roots, restore_session};
    use crate::session::Session;
    use std::path::PathBuf;

    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            name: id.to_string(),
            repo_root: "/tmp/repo".to_string(),
            worktree_path: "/tmp/repo".to_string(),
            branch: "main".to_string(),
            is_worktree: false,
            status: roux_core::SessionStatus::Generating,
            model: None,
            cost: None,
            created_at: 0,
            project_id: None,
            is_git_repo: false,
            name_override: None,
            primary_pty_id: None,
            archived: true,
            ended_at: Some(123),
            blueprint_id: None,
            pinned_pr_url: None,
            smol_machine_name: None,
        }
    }

    fn temp_persist_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        (dir, path)
    }

    #[tokio::test]
    async fn restore_session_unarchives_and_marks_disconnected() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) =
            crate::session_service::spawn_with_path(vec![make_session("s1")], path);

        restore_session(&handle, "s1").await.unwrap();

        let session = handle.get("s1").await.unwrap().unwrap();
        assert!(!session.archived);
        assert!(session.ended_at.is_none());
        assert_eq!(session.status, roux_core::SessionStatus::Disconnected);
    }

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
        std::fs::write(root.join("repo-wt/.git"), "gitdir: /tmp/project/.git/worktrees/repo-wt\n")
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
