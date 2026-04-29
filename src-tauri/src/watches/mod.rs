pub mod checks;
pub mod flap;
pub mod manager;
pub mod store;

pub use manager::WatchManager;
pub use store::load_persisted as load_persisted_watches;

// Re-export core types used by other modules
pub use roux_core::{CreateWatchConfig, RuntimeState, Watch, WatchKind, WatchScope};

// Tauri commands
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn cmd_create_watch(
    config: CreateWatchConfig,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Watch, String> {
    let watch = Watch {
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
    };
    Ok(state.watch_manager.create_watch(watch, app).await)
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_remove_watch(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.watch_manager.remove_watch(&id).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_list_watches(state: tauri::State<'_, AppState>) -> Result<Vec<Watch>, String> {
    state.watch_manager.store().list().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_pause_watch(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
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
    state.watch_manager.resume_watch(&id, app).await;
    Ok(())
}

/// Idempotent variant of [`cmd_create_watch`] for `GithubPr` watches:
/// returns an existing watch with the same `(scope, repo, pr_number)`
/// rather than creating a duplicate. Used by the auto-PR-watch flow,
/// where session activation, manual refresh, and concurrent settings
/// changes can all race to create the same watch.
#[tauri::command]
#[specta::specta]
pub async fn cmd_find_or_create_watch(
    config: CreateWatchConfig,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Watch, String> {
    if let WatchKind::GithubPr { repo, pr_number } = &config.kind {
        let existing = state
            .watch_manager
            .store()
            .list()
            .await
            .map_err(|e| e.to_string())?;
        for w in existing {
            if scope_matches(&w.scope, &config.scope)
                && matches!(&w.kind, WatchKind::GithubPr { repo: r, pr_number: n } if r == repo && n == pr_number)
            {
                return Ok(w);
            }
        }
    }
    cmd_create_watch(config, state, app).await
}

fn scope_matches(a: &WatchScope, b: &WatchScope) -> bool {
    match (a, b) {
        (WatchScope::Global, WatchScope::Global) => true,
        (
            WatchScope::Session { session_id: x },
            WatchScope::Session { session_id: y },
        ) => x == y,
        (
            WatchScope::Project { project_id: x },
            WatchScope::Project { project_id: y },
        ) => x == y,
        _ => false,
    }
}
