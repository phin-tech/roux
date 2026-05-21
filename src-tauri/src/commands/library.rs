use crate::paths::default_notes_vault_root;
use crate::services::library as svc;
use crate::state::AppState;
use roux_core::{LibrarySource, LibrarySourceKind};
use std::path::PathBuf;
use tauri::Emitter;

fn join_err(e: tauri::Error) -> String {
    format!("task join: {e}")
}

async fn active_repo_for_session(
    state: &AppState,
    session_id: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(id) = session_id else {
        return Ok(None);
    };
    let session = state
        .runtime.session_handle
        .get(id)
        .await
        .map_err(|e| format!("load session {id}: {e}"))?;
    Ok(session.map(|session| session.repo_root))
}

fn settings_snapshot(
    state: &AppState,
) -> Result<crate::settings::RouxSettings, String> {
    state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())
        .map(|guard| guard.clone())
}

fn global_root_from(settings: &crate::settings::RouxSettings) -> PathBuf {
    settings
        .notes_vault_root
        .as_ref()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_vault_root)
}

fn library_layers_for(
    settings: &crate::settings::RouxSettings,
    active_repo: Option<String>,
) -> Vec<svc::LibraryLayer> {
    svc::layers(
        global_root_from(settings),
        &settings.library_sources,
        &managed_sources_root(),
        active_repo,
    )
}

fn managed_sources_root() -> PathBuf {
    crate::paths::roux_config_dir().join("library-sources")
}

fn source_by_id(state: &AppState, source_id: &str) -> Result<LibrarySource, String> {
    state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .library_sources
        .iter()
        .find(|source| source.id == source_id)
        .cloned()
        .ok_or_else(|| format!("library source not found: {source_id}"))
}

fn save_sources(
    sources: Vec<LibrarySource>,
    state: &tauri::State<'_, AppState>,
    app: &tauri::AppHandle,
) -> Result<Vec<LibrarySource>, String> {
    let mut settings =
        state.settings.lock().map_err(|_| "settings lock poisoned".to_string())?.clone();
    settings.library_sources = sources;
    settings = settings.normalized();
    crate::settings::save_settings(&settings).map_err(|e| e.to_string())?;
    *state.settings.lock().map_err(|_| "settings lock poisoned".to_string())? = settings.clone();
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())?;
    Ok(settings.library_sources)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_library_items(
    session_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<svc::LibraryItem>, String> {
    let active_repo = active_repo_for_session(&state, session_id.as_deref()).await?;
    let settings = settings_snapshot(&state)?;
    let layers = library_layers_for(&settings, active_repo);
    tauri::async_runtime::spawn_blocking(move || svc::list_items(&layers))
        .await
        .map_err(join_err)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn read_library_item(
    item_id: String,
    session_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<svc::LibraryRead, String> {
    let active_repo = active_repo_for_session(&state, session_id.as_deref()).await?;
    let settings = settings_snapshot(&state)?;
    let layers = library_layers_for(&settings, active_repo);
    tauri::async_runtime::spawn_blocking(move || svc::read_item(&layers, &item_id))
        .await
        .map_err(join_err)?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn render_library_prompt(
    request: svc::RenderLibraryPromptRequest,
    state: tauri::State<'_, AppState>,
) -> Result<svc::RenderedLibraryPrompt, String> {
    let active_repo = active_repo_for_session(&state, request.session_id.as_deref()).await?;
    let settings = settings_snapshot(&state)?;
    let layers = library_layers_for(&settings, active_repo);
    tauri::async_runtime::spawn_blocking(move || svc::render_prompt(&layers, request))
        .await
        .map_err(join_err)?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn save_library_item(
    request: svc::SaveLibraryItemRequest,
    session_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<svc::SavedLibraryItem, String> {
    let active_repo = active_repo_for_session(&state, session_id.as_deref()).await?;
    let settings = settings_snapshot(&state)?;
    let global_root = global_root_from(&settings);
    let library_sources = settings.library_sources.clone();
    let managed_root = managed_sources_root();
    tauri::async_runtime::spawn_blocking(move || {
        svc::save_item(global_root, &library_sources, &managed_root, active_repo, request)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
#[specta::specta]
pub(crate) fn get_library_pinned_repos(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    Ok(state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .library_sources
        .iter()
        .filter(|source| source.kind == LibrarySourceKind::LocalRepo)
        .filter_map(|source| source.path.clone())
        .collect())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_library_pinned_repos(
    pinned_repos: Vec<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let existing_sources = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .library_sources
        .clone();
    let sources = merge_pinned_repos(existing_sources, pinned_repos);
    Ok(save_sources(sources, &state, &app)?
        .into_iter()
        .filter(|source| source.kind == LibrarySourceKind::LocalRepo)
        .filter_map(|source| source.path)
        .collect())
}

fn merge_pinned_repos(
    existing_sources: Vec<LibrarySource>,
    pinned_repos: Vec<String>,
) -> Vec<LibrarySource> {
    let pinned = pinned_repos.iter().cloned().collect::<std::collections::HashSet<_>>();
    let mut seen = std::collections::HashSet::new();
    let mut sources = Vec::new();

    for source in existing_sources {
        if source.kind != LibrarySourceKind::LocalRepo {
            sources.push(source);
            continue;
        }

        let Some(path) = source.path.clone() else {
            continue;
        };
        if pinned.contains(&path) && seen.insert(path.clone()) {
            sources.push(LibrarySource {
                id: source.id,
                kind: LibrarySourceKind::LocalRepo,
                name: source.name,
                enabled: source.enabled,
                order: 0,
                path: Some(path),
                url: None,
                branch: None,
                skill_sync: source.skill_sync,
            });
        }
    }

    for path in pinned_repos {
        if seen.insert(path.clone()) {
            sources.push(LibrarySource {
                id: String::new(),
                kind: LibrarySourceKind::LocalRepo,
                name: String::new(),
                enabled: true,
                order: 0,
                path: Some(path),
                url: None,
                branch: None,
                skill_sync: None,
            });
        }
    }

    for (index, source) in sources.iter_mut().enumerate() {
        source.order = index as u32;
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_pinned_repos_preserves_git_sources() {
        let sources = merge_pinned_repos(
            vec![
                LibrarySource {
                    id: "git-1".into(),
                    kind: LibrarySourceKind::GitRepo,
                    name: "Shared".into(),
                    enabled: true,
                    order: 0,
                    path: None,
                    url: Some("https://example.com/lib.git".into()),
                    branch: Some("main".into()),
                    skill_sync: None,
                },
                LibrarySource {
                    id: "local-1".into(),
                    kind: LibrarySourceKind::LocalRepo,
                    name: "Repo".into(),
                    enabled: true,
                    order: 1,
                    path: Some("/repo".into()),
                    url: None,
                    branch: None,
                    skill_sync: None,
                },
                LibrarySource {
                    id: "git-2".into(),
                    kind: LibrarySourceKind::GitRepo,
                    name: "Later Shared".into(),
                    enabled: true,
                    order: 2,
                    path: None,
                    url: Some("https://example.com/later.git".into()),
                    branch: Some("main".into()),
                    skill_sync: None,
                },
            ],
            vec!["/repo".into(), "/other".into()],
        );

        assert_eq!(sources.len(), 4);
        assert_eq!(sources[0].kind, LibrarySourceKind::GitRepo);
        assert_eq!(sources[1].id, "local-1");
        assert_eq!(sources[2].id, "git-2");
        assert_eq!(sources[3].kind, LibrarySourceKind::LocalRepo);
        assert_eq!(sources.iter().map(|source| source.order).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn list_library_sources(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<LibrarySource>, String> {
    Ok(state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .library_sources
        .clone())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_library_sources(
    sources: Vec<LibrarySource>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<LibrarySource>, String> {
    save_sources(sources, &state, &app)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn clone_library_source(
    source_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let source = source_by_id(&state, &source_id)?;
    let managed_root = managed_sources_root();
    tauri::async_runtime::spawn_blocking(move || svc::clone_git_source(&managed_root, &source))
        .await
        .map_err(join_err)?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn sync_library_source(
    source_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<svc::LibraryGitStatus, String> {
    let source = source_by_id(&state, &source_id)?;
    let managed_root = managed_sources_root();
    tauri::async_runtime::spawn_blocking(move || svc::sync_git_source(&managed_root, &source))
        .await
        .map_err(join_err)?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_library_source_status(
    source_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<svc::LibraryGitStatus, String> {
    let source = source_by_id(&state, &source_id)?;
    let managed_root = managed_sources_root();
    tauri::async_runtime::spawn_blocking(move || svc::git_status(&managed_root, &source))
        .await
        .map_err(join_err)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_library_source_statuses(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<svc::LibraryGitStatus>, String> {
    let settings = settings_snapshot(&state)?;
    let managed_root = managed_sources_root();
    let git_sources: Vec<LibrarySource> = settings
        .library_sources
        .into_iter()
        .filter(|source| source.kind == LibrarySourceKind::GitRepo)
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let git = crate::services::setup::git_cli();
        git_sources
            .iter()
            .map(|source| svc::git_status_with_git(&managed_root, source, &git))
            .collect()
    })
    .await
    .map_err(join_err)
}
