use std::ffi::{OsStr, OsString};

pub(crate) fn nonempty_path(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Login-shell `PATH` for binary discovery on GUI launches.
///
/// macOS launches Roux with launchd's minimal PATH, which excludes
/// `/opt/homebrew/bin` and other shell-managed prefixes. Spawning the
/// user's login shell once and capturing its `PATH` is the only reliable
/// way to find tools the user has actually installed.
fn login_shell_path() -> Option<&'static str> {
    use std::sync::OnceLock;
    static CACHED: OnceLock<String> = OnceLock::new();
    let path = CACHED.get_or_init(crate::pty::get_user_path);
    (!path.is_empty()).then_some(path.as_str())
}

pub(crate) fn login_shell_path_os() -> Option<OsString> {
    login_shell_path().map(OsString::from)
}

fn find_binary_on_process_path(binary: &str) -> Option<String> {
    crate::platform::find_executable_on_path(binary).map(|p| p.to_string_lossy().to_string())
}

fn resolve_binary_path(binary: &str, override_path: Option<String>) -> Option<String> {
    if let Some(path) = override_path {
        return Some(path);
    }
    let extra = login_shell_path_os();
    if let Some(path_env) = extra.as_deref() {
        if let Some(found) = find_in_path_env(path_env, binary) {
            return Some(found);
        }
    }
    find_binary_on_process_path(binary)
}

pub(crate) fn find_in_path_env(path_env: &OsStr, binary: &str) -> Option<String> {
    for dir in std::env::split_paths(path_env) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// Resolve the `gh` binary Roux should invoke. Delegates to
/// [`roux_gh::resolve_bin`], threading the `gh_binary_path` settings
/// override and the user's login-shell `PATH` so GUI launches see
/// Homebrew etc.
pub(crate) fn gh_command() -> String {
    let override_path = gh_override_path();
    let extra = login_shell_path_os();
    roux_gh::resolve_bin(override_path.as_deref(), extra.as_deref())
        .to_string_lossy()
        .into_owned()
}

/// Resolve the `git` binary Roux should invoke for native git operations.
/// Threads the `git_binary_path` settings override and the user's
/// login-shell `PATH` through to [`roux_git::resolve_bin`].
pub(crate) fn git_cli() -> roux_git::GitCli {
    let override_path = git_override_path();
    let extra = login_shell_path_os();
    roux_git::GitCli::new(roux_git::resolve_bin(override_path.as_deref(), extra.as_deref()))
}

fn git_override_path() -> Option<String> {
    nonempty_path(crate::settings::load_settings().git_binary_path.as_deref())
}

/// True iff a usable `gh` can be located via [`gh_command`]'s precedence.
pub(crate) fn is_gh_available() -> bool {
    let override_path = gh_override_path();
    let extra = login_shell_path_os();
    roux_gh::is_available(override_path.as_deref(), extra.as_deref())
}

fn gh_override_path() -> Option<String> {
    nonempty_path(crate::settings::load_settings().gh_binary_path.as_deref())
}

/// Resolve the `wt` (worktrunk) binary as a typed `WtBinary`.
///
/// Precedence:
///   1. `settings.worktrunk_binary_path` override (trimmed, non-empty).
///   2. First match in the login-shell `PATH`.
///   3. Process `PATH`.
///
/// Returns `None` when no binary is found, `wt --version` is unparseable,
/// or the resolved version is below `roux_worktrunk::MIN_WT_VERSION`.
pub(crate) fn resolve_wt_binary() -> Option<roux_worktrunk::WtBinary> {
    let override_path =
        nonempty_path(crate::settings::load_settings().worktrunk_binary_path.as_deref());

    if let Some(path) = override_path.as_deref() {
        return roux_worktrunk::detect_wt(Some(path));
    }
    if let Some(extra) = login_shell_path_os() {
        if let Some(path) = find_in_path_env(extra.as_os_str(), "wt") {
            return roux_worktrunk::detect_wt(Some(&path));
        }
    }
    roux_worktrunk::detect_wt(None)
}

/// Resolve the `gh` binary and probe its version. Sync because it's only
/// called on startup.
pub(crate) fn detect_gh() -> Option<(String, String)> {
    if !is_gh_available() {
        return None;
    }
    let path = gh_command();
    roux_gh::GhCli::new(&path).version_blocking()
}

/// Resolve the `git` binary and probe its version. Sync because it's only
/// called on startup.
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
