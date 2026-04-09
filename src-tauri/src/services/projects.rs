use std::path::PathBuf;

pub(crate) fn notes_path(project_id: &str) -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("roux").join("notes").join(format!("{}.txt", project_id))
}

pub(crate) fn get_notes(project_id: &str) -> anyhow::Result<String> {
    let path = notes_path(project_id);
    if path.exists() {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Ok(String::new())
    }
}

pub(crate) fn set_notes(project_id: &str, content: &str) -> anyhow::Result<()> {
    let path = notes_path(project_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}
