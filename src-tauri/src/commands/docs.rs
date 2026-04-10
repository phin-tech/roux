use crate::services::docs as svc;

#[tauri::command]
#[specta::specta]
pub(crate) fn read_file(path: String) -> Result<String, String> {
    svc::read_file(&path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn write_file(path: String, contents: String) -> Result<(), String> {
    svc::write_file(&path, &contents).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn list_docs(dir: String) -> Result<Vec<svc::DocFile>, String> {
    svc::list_docs(&dir).map_err(|e| e.to_string())
}
