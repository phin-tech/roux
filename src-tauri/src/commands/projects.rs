use crate::services::projects as svc;
use crate::state::AppState;
use roux_core::Project;

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_projects(state: tauri::State<'_, AppState>) -> Result<Vec<Project>, String> {
    state.project_handle.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn create_project(name: String, state: tauri::State<'_, AppState>) -> Result<Project, String> {
    let project = Project { id: uuid::Uuid::new_v4().to_string(), name };
    state.project_handle.add(project.clone()).await.map_err(|e| e.to_string())?;
    Ok(project)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_project(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.project_handle.remove(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn rename_project(id: String, name: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.project_handle.rename(&id, &name).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_project(session_id: String, project_id: Option<String>, state: tauri::State<'_, AppState>) -> Result<(), String> {
    svc::set_session_project(&state.session_handle, &session_id, project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn get_project_notes(project_id: String) -> Result<String, String> {
    svc::get_notes(&project_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_project_notes(project_id: String, content: String) -> Result<(), String> {
    svc::set_notes(&project_id, &content).map_err(|e| e.to_string())
}
