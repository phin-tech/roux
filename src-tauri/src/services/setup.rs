pub(crate) fn is_command_available(command: &str) -> bool {
    crate::platform::find_executable_on_path(command).is_some()
}

/// Resolve the `gh` binary Roux should invoke.
///
/// Precedence:
///   1. `settings.gh_binary_path` override (trimmed, non-empty).
///   2. First match in the login-shell `PATH` (`pty::get_user_path()` spawns
///      the user's shell — including fish — so Homebrew and other
///      shell-managed prefixes are visible to GUI launches).
///   3. Process `PATH` (minimal on macOS GUI launches, but fine for CLI-dev).
///   4. Bare `"gh"` — lets `Command::new` error naturally.
pub(crate) fn gh_command() -> String {
    if let Some(path) = gh_override_path() {
        return path;
    }
    if let Some(path) = find_gh_via_login_shell() {
        return path;
    }
    if let Some(path) = crate::platform::find_executable_on_path("gh") {
        return path.to_string_lossy().to_string();
    }
    "gh".to_string()
}

/// True iff a usable `gh` can be found via any of the resolution steps in
/// [`gh_command`].
pub(crate) fn is_gh_available() -> bool {
    if let Some(path) = gh_override_path() {
        return std::path::Path::new(&path).is_file();
    }
    find_gh_via_login_shell().is_some() || is_command_available("gh")
}

fn gh_override_path() -> Option<String> {
    crate::settings::load_settings()
        .gh_binary_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn find_gh_via_login_shell() -> Option<String> {
    use std::sync::OnceLock;
    // Cache the login-shell PATH lookup — `pty::get_user_path` spawns a
    // shell and costs tens of ms. Cache the *resolved gh path*, not just
    // the PATH, so we pay the lookup once per process. If the user moves
    // gh mid-session, restart Roux.
    static CACHED: OnceLock<Option<String>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let path = crate::pty::get_user_path();
            if path.is_empty() {
                return None;
            }
            crate::platform::find_executable_in_paths(path.as_str(), "gh")
                .map(|p| p.to_string_lossy().to_string())
        })
        .clone()
}

pub(crate) fn is_cli_installed() -> bool {
    crate::hooks::cli_is_installed()
}

pub(crate) fn is_cli_current() -> bool {
    crate::hooks::cli_is_current()
}

pub(crate) fn installed_cli_version() -> Option<String> {
    crate::hooks::installed_cli_version()
}

pub(crate) fn bundled_cli_version() -> &'static str {
    crate::hooks::bundled_cli_version()
}

pub(crate) fn install_hooks() -> anyhow::Result<()> {
    crate::hooks::install_hooks().map_err(anyhow::Error::msg)?;
    Ok(())
}

pub(crate) fn install_skill() -> anyhow::Result<()> {
    crate::skill::install_skill().map_err(anyhow::Error::msg)?;
    Ok(())
}

pub(crate) fn is_skill_installed() -> bool {
    crate::skill::skill_is_installed()
}

pub(crate) fn is_hooks_installed() -> bool {
    crate::hooks::setup_is_complete()
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
