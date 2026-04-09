pub(crate) fn is_command_available(command: &str) -> bool {
    let user_path = crate::pty::get_user_path();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    std::process::Command::new(&shell)
        .args(["-c", &format!("command -v {}", command)])
        .env("PATH", &user_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub(crate) fn is_cli_installed() -> bool {
    crate::hooks::cli_is_installed()
}

pub(crate) fn install_hooks() -> anyhow::Result<()> {
    crate::hooks::install_hooks()?;
    Ok(())
}

pub(crate) fn list_nono_profiles() -> Vec<String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let profiles_dir = home.join(".config").join("nono").join("profiles");
    if !profiles_dir.is_dir() {
        return Vec::new();
    }
    let mut profiles = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                profiles.push(name.to_string());
            }
        }
    }
    profiles.sort();
    profiles
}
