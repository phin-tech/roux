pub(crate) fn is_command_available(command: &str) -> bool {
    crate::platform::find_executable_on_path(command).is_some()
}

fn nonempty_path(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn login_shell_path() -> Option<&'static str> {
    use std::sync::OnceLock;
    static CACHED: OnceLock<String> = OnceLock::new();
    let path = CACHED.get_or_init(crate::pty::get_user_path);
    (!path.is_empty()).then_some(path.as_str())
}

fn find_binary_via_login_shell(binary: &str) -> Option<String> {
    let path = login_shell_path()?;
    crate::platform::find_executable_in_paths(path, binary).map(|p| p.to_string_lossy().to_string())
}

fn find_binary_on_process_path(binary: &str) -> Option<String> {
    crate::platform::find_executable_on_path(binary).map(|p| p.to_string_lossy().to_string())
}

fn resolve_binary_path(binary: &str, override_path: Option<String>) -> Option<String> {
    override_path
        .or_else(|| find_binary_via_login_shell(binary))
        .or_else(|| find_binary_on_process_path(binary))
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
    resolve_binary_path("gh", gh_override_path()).unwrap_or_else(|| "gh".to_string())
}

/// Resolve the `git` binary Roux should invoke for native git operations.
///
/// Precedence mirrors [`gh_command`]:
///   1. `settings.git_binary_path` override (trimmed, non-empty).
///   2. `ROUX_GIT` env override for dev/support sessions.
///   3. First match in the login-shell `PATH`.
///   4. Process `PATH` plus `roux-git`'s common macOS fallbacks.
pub(crate) fn git_cli() -> roux_git::GitCli {
    if let Some(path) = git_override_path() {
        return roux_git::GitCli::new(path);
    }
    if let Some(path) = std::env::var_os("ROUX_GIT").filter(|path| !path.is_empty()) {
        return roux_git::GitCli::new(path);
    }
    if let Some(path) = find_binary_via_login_shell("git") {
        return roux_git::GitCli::new(path);
    }
    roux_git::GitCli::default()
}

fn git_override_path() -> Option<String> {
    nonempty_path(crate::settings::load_settings().git_binary_path.as_deref())
}

/// True iff a usable `gh` can be found via any of the resolution steps in
/// [`gh_command`].
pub(crate) fn is_gh_available() -> bool {
    if let Some(path) = gh_override_path() {
        return std::path::Path::new(&path).is_file();
    }
    find_binary_via_login_shell("gh").is_some() || is_command_available("gh")
}

fn gh_override_path() -> Option<String> {
    nonempty_path(crate::settings::load_settings().gh_binary_path.as_deref())
}

/// Resolve the `wt` (worktrunk) binary as a typed `WtBinary`.
///
/// Precedence mirrors [`gh_command`]:
///   1. `settings.worktrunk_binary_path` override (trimmed, non-empty).
///   2. First match in the login-shell `PATH`.
///   3. Process `PATH`.
///
/// Returns `None` when no binary is found, `wt --version` is unparseable,
/// or the resolved version is below `roux_worktrunk::MIN_WT_VERSION`. A
/// caller receiving `None` should fall back to the native git path.
pub(crate) fn resolve_wt_binary() -> Option<roux_worktrunk::WtBinary> {
    let override_path =
        nonempty_path(crate::settings::load_settings().worktrunk_binary_path.as_deref());

    if let Some(path) = override_path.as_deref() {
        return roux_worktrunk::detect_wt(Some(path));
    }
    if let Some(path) = find_binary_via_login_shell("wt") {
        return roux_worktrunk::detect_wt(Some(&path));
    }
    roux_worktrunk::detect_wt(None)
}

/// Resolve the `gh` binary and probe its version.
///
/// Returns `(binary_path, version_string)` when gh is found and responsive,
/// `None` otherwise.
pub(crate) fn detect_gh() -> Option<(String, String)> {
    if !is_gh_available() {
        return None;
    }
    let path = gh_command();
    let out = std::process::Command::new(&path).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // "gh version 2.60.1 (2024-12-11)"
    let version = stdout.lines().next()?.split_whitespace().nth(2)?.to_string();
    Some((path, version))
}

/// Resolve the `git` binary and probe its version.
///
/// Returns `(binary_path, version_string)` when git is found and responsive,
/// `None` otherwise.
pub(crate) fn detect_git() -> Option<(String, String)> {
    let cli = git_cli();
    let path = cli.git_bin().to_string_lossy().into_owned();
    let out = std::process::Command::new(&path).arg("--version").output().ok()?;
    // "git version 2.47.2" or "git version 2.47.2 (Apple Git-148)"
    let stdout = String::from_utf8_lossy(&out.stdout);
    let version = stdout.lines().next()?.split_whitespace().nth(2)?.to_string();
    Some((path, version))
}

/// Resolve the `code` (VS Code) binary Roux should invoke for "Open in Code".
///
/// Mirrors [`gh_command`] precedence (minus the settings override — there is
/// no editor setting yet):
///   1. First match in the login-shell `PATH` — GUI launches on macOS get a
///      minimal launchd PATH that excludes `/opt/homebrew/bin` etc., so the
///      `code` shim is invisible without this step.
///   2. Process `PATH`.
///   3. Bare `"code"` — lets `Command::new` error naturally.
pub(crate) fn code_command() -> String {
    resolve_binary_path("code", None).unwrap_or_else(|| "code".to_string())
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
