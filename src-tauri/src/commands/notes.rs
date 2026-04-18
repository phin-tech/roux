//! Tauri commands for the multi-scoped notes vault.
//!
//! **Experimental.** Command names, argument shapes, and return types are
//! subject to change. See the Stability section of
//! `docs/superpowers/specs/2026-04-18-notes-expansion-design.md`.
//!
//! These handlers are deliberately thin. All real behavior lives in
//! `services::notes`, which is extensively unit-tested. Each command here
//! is: (1) load the requested session record (if any), (2) call
//! `NotesService::resolve_target` to produce a `(Scope, topic, session_slug)`
//! tuple, (3) delegate to the matching `NotesService` method. That gives
//! us a single source of truth for scope resolution and slug freezing, and
//! keeps the Tauri adapter layer from accumulating its own behavior.

use crate::paths::default_notes_vault_root;
use crate::services::notes::{AppendOpts, NotesService, Scope};
use crate::state::AppState;
use roux_core::Session;
use serde::{Deserialize, Serialize};

/// Scope + addressing info for a single note file. Sent verbatim from the
/// frontend or the CLI; the backend resolves the pieces it needs.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotesTarget {
    /// One of `"global" | "project" | "repo" | "session"`.
    pub(crate) scope: String,
    /// Session id used to resolve repo/project context. Falls through to
    /// the focused session on the frontend side when unset.
    pub(crate) session_id: Option<String>,
    /// Optional topic filename (without `.md`). `None` targets the scope's
    /// `notes.md` anchor.
    pub(crate) topic: Option<String>,
    /// Override the repo/project slug directly (CLI `--repo` / `--project`).
    /// Ignored for the session scope.
    pub(crate) override_slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotesRead {
    pub(crate) path: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotesSearchQuery {
    pub(crate) tags: Vec<String>,
    /// Optional scope name (`"global" | "project" | "repo" | "session"`)
    /// to restrict the walk. `None` walks the whole vault.
    pub(crate) scope: Option<String>,
    pub(crate) exact: bool,
}

fn vault_root(state: &AppState) -> std::path::PathBuf {
    let override_path = state
        .settings
        .lock()
        .ok()
        .and_then(|s| s.notes_vault_root.clone())
        .filter(|p| !p.is_empty());
    override_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_notes_vault_root)
}

/// Return the best-effort git origin URL for `repo_root`. `None` if git
/// isn't available, the directory isn't a git repo, or no `origin` remote
/// is configured.
fn git_origin_url(repo_root: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
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

/// Pull the session record the command should operate against, plus the
/// project name (when the session has one) and the remote URL.
///
/// Returns `(None, None, None)` when no session is requested and the scope
/// doesn't require one (e.g. `Scope::Global`).
async fn load_session_context(
    state: &AppState,
    session_id: Option<&str>,
) -> Result<(Option<Session>, Option<String>, Option<String>), String> {
    let Some(sid) = session_id else {
        return Ok((None, None, None));
    };
    let sessions = state.session_handle.list().await.map_err(|e| e.to_string())?;
    let Some(session) = sessions.into_iter().find(|s| s.id == sid) else {
        return Ok((None, None, None));
    };
    let project_name = match session.project_id.as_deref() {
        Some(pid) => {
            let projects = state.project_handle.list().await.map_err(|e| e.to_string())?;
            projects.into_iter().find(|p| p.id == pid).map(|p| p.name)
        }
        None => None,
    };
    let remote = git_origin_url(&session.repo_root);
    Ok((Some(session), project_name, remote))
}

fn build_service(state: &AppState) -> NotesService {
    NotesService::new(vault_root(state))
}

async fn resolve(
    svc: &mut NotesService,
    state: &AppState,
    target: &NotesTarget,
) -> Result<(Scope, Option<String>, String), String> {
    let (session, project_name, remote) =
        load_session_context(state, target.session_id.as_deref()).await?;
    svc.resolve_target(
        &target.scope,
        session.as_ref(),
        project_name.as_deref(),
        remote.as_deref(),
        target.topic.clone(),
    )
}

/// Core logic for `notes_read`; also called from the socket CLI bridge.
pub(crate) async fn do_notes_read(
    target: NotesTarget,
    state: &AppState,
) -> Result<NotesRead, String> {
    let mut svc = build_service(state);
    let (scope, topic, session_slug) = resolve(&mut svc, state, &target).await?;
    let content = svc
        .read_file(&scope, topic.as_deref(), &session_slug)
        .map_err(|e| e.to_string())?;
    let path = svc
        .file_path(&scope, topic.as_deref(), &session_slug)
        .to_string_lossy()
        .into_owned();
    Ok(NotesRead { path, content })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn notes_read(
    target: NotesTarget,
    state: tauri::State<'_, AppState>,
) -> Result<NotesRead, String> {
    do_notes_read(target, &state).await
}

pub(crate) async fn do_notes_write(
    target: NotesTarget,
    content: String,
    tags: Vec<String>,
    state: &AppState,
) -> Result<(), String> {
    let mut svc = build_service(state);
    let (scope, topic, session_slug) = resolve(&mut svc, state, &target).await?;
    let now = now_iso8601();
    svc.write_file(
        &scope,
        topic.as_deref(),
        &session_slug,
        &content,
        &now,
        &tags,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn notes_write(
    target: NotesTarget,
    content: String,
    tags: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    do_notes_write(target, content, tags, &state).await
}

pub(crate) async fn do_notes_append(
    target: NotesTarget,
    content: String,
    timestamped: bool,
    tags: Vec<String>,
    state: &AppState,
) -> Result<(), String> {
    let mut svc = build_service(state);
    let (scope, topic, session_slug) = resolve(&mut svc, state, &target).await?;
    let now = now_iso8601();

    let include_web_anchor = state
        .settings
        .lock()
        .map_err(|e| e.to_string())?
        .notes_include_web_anchors;

    // Hoist the owned strings above the match so AppendOpts can borrow them.
    let ts = now_short();
    let id = short_entry_id();
    let opts = if timestamped {
        AppendOpts::Timestamped {
            timestamp: &ts,
            id: &id,
            include_web_anchor,
        }
    } else {
        AppendOpts::Plain
    };

    svc.append_file(
        &scope,
        topic.as_deref(),
        &session_slug,
        &content,
        opts,
        &now,
        &tags,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn notes_append(
    target: NotesTarget,
    content: String,
    timestamped: bool,
    tags: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    do_notes_append(target, content, timestamped, tags, &state).await
}

pub(crate) async fn do_notes_path(
    target: NotesTarget,
    dir: bool,
    state: &AppState,
) -> Result<String, String> {
    let mut svc = build_service(state);
    let (scope, topic, session_slug) = resolve(&mut svc, state, &target).await?;
    let p = if dir {
        svc.dir_path(&scope, &session_slug)
    } else {
        svc.file_path(&scope, topic.as_deref(), &session_slug)
    };
    Ok(p.to_string_lossy().into_owned())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn notes_path(
    target: NotesTarget,
    dir: bool,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    do_notes_path(target, dir, &state).await
}

pub(crate) fn do_notes_search(
    query: NotesSearchQuery,
    state: &AppState,
) -> Result<Vec<String>, String> {
    if query.tags.is_empty() {
        return Err("at least one --tag is required".to_string());
    }
    let svc = build_service(state);
    let hits = svc.search(query.scope.as_deref(), &query.tags, query.exact);
    Ok(hits.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn notes_search(
    query: NotesSearchQuery,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    do_notes_search(query, &state)
}

pub(crate) fn do_notes_vault_root(state: &AppState) -> String {
    vault_root(state).to_string_lossy().into_owned()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn notes_vault_root(state: tauri::State<'_, AppState>) -> Result<String, String> {
    Ok(do_notes_vault_root(&state))
}

fn now_iso8601() -> String {
    // Best-effort local-ish ISO-8601 via std::time. For v1, seconds resolution
    // is fine and we avoid pulling in chrono.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_epoch_seconds_iso8601(secs)
}

fn now_short() -> String {
    // `YYYY-MM-DD HH:MM` local time — but we don't have a timezone db in
    // std, so this is a best-effort UTC rendering until we pull in chrono
    // or time. v1 acceptable per the spec's timestamp format notes.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_epoch_seconds_short(secs)
}

fn short_entry_id() -> String {
    // First 8 hex chars of a fresh UUID v4.
    let u = uuid::Uuid::new_v4();
    let s = u.simple().to_string();
    s.chars().take(8).collect()
}

fn format_epoch_seconds_iso8601(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn format_epoch_seconds_short(secs: u64) -> String {
    let (y, mo, d, h, mi, _s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}

/// Civil date/time from Unix epoch seconds, UTC. Enough for writing
/// timestamps in note frontmatter and entry headings. Not meant to be a
/// general-purpose calendar library — we'll pull in `chrono`/`time` if we
/// need timezones, leap seconds, or anything fancier.
fn epoch_to_ymdhms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;
    let (y, mo, d) = civil_from_days(days + 719468);
    (y, mo, d, h, mi, s)
}

/// http://howardhinnant.github.io/date_algorithms.html#civil_from_days
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}
