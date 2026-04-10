pub mod checks;
pub mod flap;
pub mod manager;
pub mod store;

pub use manager::WatchManager;
pub use store::load_persisted as load_persisted_watches;

// Re-export core types used by other modules
pub use roux_core::{CreateWatchConfig, RuntimeState, Watch};

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
pub async fn cmd_pause_watch(id: String, state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
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
