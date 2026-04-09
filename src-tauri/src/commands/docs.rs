use crate::services::docs as svc;

#[tauri::command]
pub(crate) fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))
}

#[tauri::command]
pub(crate) fn write_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, &contents).map_err(|e| format!("Failed to write file: {}", e))
}

#[tauri::command]
pub(crate) fn list_docs(dir: String) -> Result<Vec<svc::DocFile>, String> {
    svc::list_docs(&dir).map_err(|e| e.to_string())
}
