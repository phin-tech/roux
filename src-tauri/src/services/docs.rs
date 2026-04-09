use std::path::Path;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocFile {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) relative_path: String,
    pub(crate) modified: u64,
}

pub(crate) fn read_file(path: &str) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

pub(crate) fn write_file(path: &str, contents: &str) -> anyhow::Result<()> {
    std::fs::write(path, contents)?;
    Ok(())
}

pub(crate) fn list_docs(dir: &str) -> anyhow::Result<Vec<DocFile>> {
    let base = Path::new(dir);
    if !base.is_dir() {
        return Err(anyhow::anyhow!("Not a directory: {}", dir));
    }

    let skip_dirs: std::collections::HashSet<&str> =
        ["node_modules", ".git", "target", "dist", ".svelte-kit", ".superpowers"]
            .iter()
            .copied()
            .collect();

    let mut docs = Vec::new();
    let mut stack = vec![base.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !skip_dirs.contains(name) {
                        stack.push(path);
                    }
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let modified = path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let relative =
                    path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();

                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

                docs.push(DocFile {
                    path: path.to_string_lossy().to_string(),
                    name,
                    relative_path: relative,
                    modified,
                });
            }
        }
    }

    docs.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(docs)
}
