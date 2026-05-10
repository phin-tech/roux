//! Tauri commands exposing the alias surface to the frontend.
//!
//! Read-only here for Phase 3 — the UI lists aliases, gets one by name,
//! and runs `whoami`. Mutations (set/unset/claim) keep flowing through
//! the CLI / socket so settings-style UI work can land later without
//! re-shaping the surface.

use roux_core::{validate_user_alias_name, AgentAlias, ConsumptionMode};
use roux_lib::aliases::ProjectFilter;

use crate::state::AppState;

fn project_filter<'a>(project_id: Option<&'a str>, global: bool) -> ProjectFilter<'a> {
    match (project_id, global) {
        (Some(p), _) => ProjectFilter::Exact(Some(p)),
        (None, true) => ProjectFilter::Exact(None),
        (None, false) => ProjectFilter::Any,
    }
}

#[tauri::command]
pub async fn aliases_list(
    project_id: Option<String>,
    global: Option<bool>,
    only_unbound: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AgentAlias>, String> {
    Ok(state.alias_manager.list(
        project_filter(project_id.as_deref(), global.unwrap_or(false)),
        only_unbound.unwrap_or(false),
    ))
}

#[tauri::command]
pub async fn aliases_get(
    alias: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<AgentAlias>, String> {
    Ok(state.alias_manager.get(&alias, project_id.as_deref()))
}

#[tauri::command]
pub async fn aliases_whoami(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AgentAlias>, String> {
    Ok(state.alias_manager.whoami(&session_id))
}

#[tauri::command]
pub async fn aliases_add_member(
    alias: String,
    pane_id: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<AgentAlias, String> {
    let canonical = validate_user_alias_name(&alias).map_err(|e| e.to_string())?;
    state
        .alias_manager
        .add_member(&canonical, project_id.as_deref(), &pane_id, Some(&app))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn aliases_remove_member(
    alias: String,
    pane_id: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let canonical = validate_user_alias_name(&alias).map_err(|e| e.to_string())?;
    state
        .alias_manager
        .remove_member(&canonical, project_id.as_deref(), &pane_id, Some(&app))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn aliases_set_mode(
    alias: String,
    mode: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<AgentAlias, String> {
    let canonical = validate_user_alias_name(&alias).map_err(|e| e.to_string())?;
    let parsed = match mode.as_str() {
        "competing" | "competingConsumer" | "competing-consumer" => {
            ConsumptionMode::CompetingConsumer
        }
        "broadcast" => ConsumptionMode::Broadcast,
        other => {
            return Err(format!(
                "invalid mode '{other}'; expected 'competing' or 'broadcast'"
            ))
        }
    };
    state
        .alias_manager
        .set_consumption_mode(&canonical, project_id.as_deref(), parsed, Some(&app))
        .map_err(|e| e.to_string())
}
