pub(crate) fn is_command_available(command: &str) -> bool {
    crate::platform::find_executable_on_path(command).is_some()
}

pub(crate) fn is_cli_installed() -> bool {
    crate::hooks::cli_is_installed()
}

pub(crate) fn is_setup_complete() -> bool {
    crate::hooks::setup_is_complete()
}

pub(crate) fn install_hooks() -> anyhow::Result<()> {
    crate::hooks::install_hooks().map_err(anyhow::Error::msg)?;
    Ok(())
}

pub(crate) fn list_nono_profiles() -> Vec<String> {
    if !is_command_available("nono") {
        return Vec::new();
    }

    let profiles_dir = match dirs::config_dir() {
        Some(dir) => dir.join("nono").join("profiles"),
        None => return Vec::new(),
    };
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
