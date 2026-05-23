pub mod checks;
pub mod flap;
pub mod manager;
pub mod store;

pub use manager::WatchManager;
pub use store::load_persisted as load_persisted_watches;

// Re-export core types used by other modules
pub use roux_core::{CreateWatchConfig, RuntimeState, Watch, WatchKind};

// Tauri commands
use crate::state::AppState;

fn watch_from_config(config: CreateWatchConfig) -> Watch {
    Watch {
        id: uuid::Uuid::new_v4().to_string(),
        name: config.name,
        kind: config.kind,
        mode: config.mode,
        scope: config.scope,
        runtime_state: RuntimeState::Pending,
        last_result: None,
        last_checked: None,
        notify: config.notify.unwrap_or_default(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_create_watch(
    config: CreateWatchConfig,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Watch, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("watch-create"))
    {
        let watch = client.create_watch(config).await?;
        state.watch_manager.adopt_watch(watch.clone(), app).await;
        return Ok(watch);
    }

    let watch = watch_from_config(config);
    Ok(state.watch_manager.create_watch(watch, app).await)
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_remove_watch(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("watch-remove"))
    {
        client.remove_watch(id.clone()).await?;
        state.watch_manager.remove_watch(&id).await;
        return Ok(());
    }

    state.watch_manager.remove_watch(&id).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_list_watches(state: tauri::State<'_, AppState>) -> Result<Vec<Watch>, String> {
    if let Some(client) = state.daemon_client.clone().filter(|client| client.supports("watch-list"))
    {
        return client.list_watches().await;
    }

    state.watch_manager.store().list().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_pause_watch(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("watch-pause"))
    {
        let watch = client.pause_watch(id.clone()).await?;
        state.watch_manager.adopt_watch(watch, app).await;
        return Ok(());
    }

    state.watch_manager.pause_watch(&id, &app).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_resume_watch(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("watch-resume"))
    {
        let watch = client.resume_watch(id.clone()).await?;
        state.watch_manager.adopt_watch(watch, app).await;
        return Ok(());
    }

    state.watch_manager.resume_watch(&id, app).await;
    Ok(())
}

/// Idempotent variant of [`cmd_create_watch`] for `GithubPr` watches.
/// Find-or-insert is performed atomically inside the watch-store actor,
/// so concurrent callers (session activation + manual refresh + settings
/// toggle) resolve to the same watch instead of creating duplicates.
/// For non-`GithubPr` kinds the call falls through to `cmd_create_watch`
/// since dedupe semantics for those aren't defined.
#[tauri::command]
#[specta::specta]
pub async fn cmd_find_or_create_watch(
    config: CreateWatchConfig,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Watch, String> {
    if let Some(client) =
        state.daemon_client.clone().filter(|client| client.supports("watch-find-or-create"))
    {
        let watch = client.find_or_create_watch(config).await?;
        state.watch_manager.adopt_watch(watch.clone(), app).await;
        return Ok(watch);
    }

    if matches!(config.kind, WatchKind::GithubPr { .. }) {
        let watch = watch_from_config(config);
        return Ok(state.watch_manager.find_or_create_github_pr_watch(watch, app).await);
    }
    cmd_create_watch(config, state, app).await
}
