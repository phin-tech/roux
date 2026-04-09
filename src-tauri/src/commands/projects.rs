use crate::projects::Project;
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
pub(crate) async fn set_session_project(
    session_id: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let handle = state.session_handle.clone();
    handle.set_project(&session_id, project_id).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_project_notes(project_id: String) -> Result<String, String> {
    let path = notes_path(&project_id);
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read notes: {}", e))
    } else {
        Ok(String::new())
    }
}

#[tauri::command]
pub(crate) fn set_project_notes(project_id: String, content: String) -> Result<(), String> {
    let path = notes_path(&project_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create notes dir: {}", e))?;
    }
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write notes: {}", e))
}

fn notes_path(project_id: &str) -> std::path::PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("roux").join("notes").join(format!("{}.txt", project_id))
}
