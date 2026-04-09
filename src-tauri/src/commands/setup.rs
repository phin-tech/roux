#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupStatus {
    cli_installed: bool,
    gh_available: bool,
}

#[tauri::command]
pub(crate) fn check_setup_status() -> SetupStatus {
    let user_path = crate::pty::get_user_path();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let gh_available = std::process::Command::new(&shell)
        .args(["-c", "command -v gh"])
        .env("PATH", &user_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    SetupStatus { cli_installed: crate::hooks::cli_is_installed(), gh_available }
}

// Backwards compat: kept as alias used nowhere else
#[tauri::command]
pub(crate) fn check_setup_needed() -> bool {
    !crate::hooks::cli_is_installed()
}

#[tauri::command]
pub(crate) fn run_setup() -> Result<(), String> {
    crate::hooks::install_hooks().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn check_nono_installed() -> bool {
    let user_path = crate::pty::get_user_path();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    std::process::Command::new(&shell)
        .args(["-c", "command -v nono"])
        .env("PATH", &user_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tauri::command]
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
            // Use file stem (without extension) as the profile name
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                profiles.push(name.to_string());
            }
        }
    }
    profiles.sort();
    profiles
}
