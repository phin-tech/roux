use crate::services::projects as svc;
use crate::state::AppState;
use roux_core::{Project, ProjectUpdate};

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_projects(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Project>, String> {
    state.project_handle.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn create_project(
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<Project, String> {
    let project = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        repo_roots: Vec::new(),
        context_paths: Vec::new(),
        session_blueprints: Vec::new(),
        project_prompt: String::new(),
    };
    state.project_handle.add(project.clone()).await.map_err(|e| e.to_string())?;
    Ok(project)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_project(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let removed = state.project_handle.get(&id).await.map_err(|e| e.to_string())?;
    state.project_handle.remove(&id).await.map_err(|e| e.to_string())?;
    if let Err(e) = state.session_handle.clear_project_refs(&id).await {
        if let Some(project) = removed {
            let _ = state.project_handle.add(project).await;
        }
        return Err(e.to_string());
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn rename_project(
    id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.project_handle.rename(&id, &name).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn update_project(
    id: String,
    patch: ProjectUpdate,
    state: tauri::State<'_, AppState>,
) -> Result<Project, String> {
    state
        .project_handle
        .update(&id, patch)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project {} not found", id))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_project(
    session_id: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    svc::set_session_project(&state.session_handle, &session_id, project_id)
        .await
        .map_err(|e| e.to_string())
}
