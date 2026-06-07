use crate::services::settings as svc;
use crate::state::AppState;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Emitter;

const KANBAN_WORKFLOW_EXAMPLE_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/examples/kanban-workflow.json"));
const KANBAN_WORKFLOW_EXAMPLE_FILE: &str = "kanban-workflow.json";

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KanbanWorkflowExampleResult {
    pub path: String,
    pub workflow_path: String,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn get_settings(state: tauri::State<AppState>) -> crate::settings::RouxSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn update_settings(
    settings: crate::settings::RouxSettings,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let settings = svc::update_settings(settings).map_err(|e| e.to_string())?;
    crate::pty::set_shell_binary_path_override(settings.shell_binary_path.clone());
    *state.settings.lock().unwrap() = settings.clone();
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_validate_kanban_workflow(
    settings: crate::settings::RouxSettings,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<crate::settings::RouxSettings, String> {
    let settings = svc::update_settings(settings).map_err(|e| e.to_string())?;
    crate::pty::set_shell_binary_path_override(settings.shell_binary_path.clone());
    *state.settings.lock().unwrap() = settings.clone();
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())?;
    Ok(settings)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_kanban_workflow_config_dir() -> Result<String, String> {
    let dir = crate::paths::roux_config_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create {}: {err}", dir.display()))?;
    Ok(dir.display().to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_create_kanban_workflow_example() -> Result<KanbanWorkflowExampleResult, String> {
    create_kanban_workflow_example_at(&crate::paths::roux_config_dir())
}

fn create_kanban_workflow_example_at(
    config_dir: &Path,
) -> Result<KanbanWorkflowExampleResult, String> {
    std::fs::create_dir_all(config_dir)
        .map_err(|err| format!("failed to create {}: {err}", config_dir.display()))?;
    let path = config_dir.join(KANBAN_WORKFLOW_EXAMPLE_FILE);
    if !path.exists() {
        std::fs::write(&path, KANBAN_WORKFLOW_EXAMPLE_JSON)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    }
    Ok(example_result(path))
}

fn example_result(path: PathBuf) -> KanbanWorkflowExampleResult {
    KanbanWorkflowExampleResult {
        path: path.display().to_string(),
        workflow_path: KANBAN_WORKFLOW_EXAMPLE_FILE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_kanban_workflow_example_creates_file_once() {
        let dir = tempfile::tempdir().unwrap();

        let created = create_kanban_workflow_example_at(dir.path()).unwrap();
        assert_eq!(created.workflow_path, KANBAN_WORKFLOW_EXAMPLE_FILE);
        assert!(Path::new(&created.path).exists());
        assert!(std::fs::read_to_string(&created.path).unwrap().contains("\"id\": \"personal\""));

        std::fs::write(&created.path, "{\"id\":\"custom\"}").unwrap();
        let existing = create_kanban_workflow_example_at(dir.path()).unwrap();

        assert_eq!(existing.path, created.path);
        assert_eq!(std::fs::read_to_string(&created.path).unwrap(), "{\"id\":\"custom\"}");
    }
}
