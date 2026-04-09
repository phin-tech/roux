use anyhow::anyhow;

pub(crate) fn list_branches(repo_path: &str) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| anyhow!("Failed to list branches: {}", e))?;
    if !output.status.success() {
        return Err(anyhow!("{}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub(crate) fn git_init(path: &str) -> anyhow::Result<()> {
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .map_err(|e| anyhow!("Failed to run git init: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!("{}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}
