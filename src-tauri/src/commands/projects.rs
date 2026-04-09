use crate::projects::Project;
use crate::services::projects as svc;
use crate::state::AppState;

#[tauri::command]
pub(crate) fn list_projects(state: tauri::State<AppState>) -> Vec<Project> {
    state.project_store.list()
}

#[tauri::command]
pub(crate) fn create_project(name: String, state: tauri::State<AppState>) -> Project {
    let project = Project { id: uuid::Uuid::new_v4().to_string(), name };
    state.project_store.add(project.clone());
    project
}

#[tauri::command]
pub(crate) fn remove_project(id: String, state: tauri::State<AppState>) {
    state.project_store.remove(&id);
}

#[tauri::command]
pub(crate) fn rename_project(id: String, name: String, state: tauri::State<AppState>) {
    state.project_store.rename(&id, &name);
}

#[tauri::command]
pub(crate) async fn set_session_project(session_id: String, project_id: Option<String>, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.session_handle.set_project(&session_id, project_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn get_project_notes(project_id: String) -> Result<String, String> {
    svc::get_notes(&project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_project_notes(project_id: String, content: String) -> Result<(), String> {
    svc::set_notes(&project_id, &content).map_err(|e| e.to_string())
}
