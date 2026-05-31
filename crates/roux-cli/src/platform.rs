#![allow(dead_code)]

use std::path::{Path, PathBuf};

const APP_DIR_NAME: &str = "roux";

pub fn app_config_dir() -> PathBuf {
    crate::paths::roux_config_dir()
}

pub fn status_dir() -> PathBuf {
    app_config_dir().join("status")
}

pub fn ensure_status_dir() -> Result<PathBuf, String> {
    let dir = status_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create status dir: {}", e))?;
    Ok(dir)
}

pub fn settings_path() -> PathBuf {
    app_config_dir().join("settings.json")
}

pub fn sessions_path() -> PathBuf {
    app_config_dir().join("sessions.json")
}

pub fn projects_path() -> PathBuf {
    app_config_dir().join("projects.json")
}

pub fn watches_path() -> PathBuf {
    app_config_dir().join("watches.json")
}

pub fn work_items_db_path() -> PathBuf {
    app_config_dir().join("board.db")
}

pub fn task_overrides_path() -> PathBuf {
    app_config_dir().join("task-overrides.json")
}

pub fn log_dir() -> PathBuf {
    app_config_dir().join("logs")
}

pub fn socket_path() -> PathBuf {
    app_config_dir().join("roux.sock")
}

pub fn socket_addr_file_path() -> PathBuf {
    app_config_dir().join("roux-socket-addr")
}

pub fn socket_auth_token_file_path() -> PathBuf {
    app_config_dir().join("roux-socket-token")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketEndpoint {
    Unix(PathBuf),
    Tcp(String),
}

impl SocketEndpoint {
    pub fn display_value(&self) -> String {
        match self {
            SocketEndpoint::Unix(path) => path.to_string_lossy().into_owned(),
            SocketEndpoint::Tcp(addr) => format!("tcp://{addr}"),
        }
    }

    pub fn tcp_addr(&self) -> Option<&str> {
        match self {
            SocketEndpoint::Tcp(addr) => Some(addr.as_str()),
            SocketEndpoint::Unix(_) => None,
        }
    }
}

pub fn parse_socket_endpoint(raw: &str) -> Option<SocketEndpoint> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(addr) = trimmed.strip_prefix("tcp://") {
        let addr = addr.trim();
        return (!addr.is_empty()).then(|| SocketEndpoint::Tcp(addr.to_string()));
    }
    if let Some(path) = trimmed.strip_prefix("unix://") {
        let path = path.trim();
        return (!path.is_empty()).then(|| SocketEndpoint::Unix(PathBuf::from(path)));
    }

    #[cfg(windows)]
    {
        Some(SocketEndpoint::Tcp(trimmed.to_string()))
    }
    #[cfg(not(windows))]
    {
        Some(SocketEndpoint::Unix(PathBuf::from(trimmed)))
    }
}

pub fn resolve_socket_endpoint_spec() -> Option<SocketEndpoint> {
    if let Ok(endpoint) = std::env::var("ROUX_SOCKET") {
        if let Some(endpoint) = parse_socket_endpoint(&endpoint) {
            return Some(endpoint);
        }
    }

    #[cfg(windows)]
    {
        return std::fs::read_to_string(socket_addr_file_path())
            .ok()
            .and_then(|value| parse_socket_endpoint(&value));
    }

    #[cfg(not(windows))]
    {
        Some(SocketEndpoint::Unix(socket_path()))
    }
}

pub fn resolve_socket_endpoint() -> Option<String> {
    resolve_socket_endpoint_spec().map(|endpoint| endpoint.display_value())
}

pub fn daemon_bind_endpoint() -> SocketEndpoint {
    std::env::var("ROUX_DAEMON_BIND")
        .ok()
        .and_then(|value| parse_socket_endpoint(&value))
        .unwrap_or_else(default_daemon_bind_endpoint)
}

fn default_daemon_bind_endpoint() -> SocketEndpoint {
    #[cfg(windows)]
    {
        SocketEndpoint::Tcp("127.0.0.1:0".to_string())
    }
    #[cfg(not(windows))]
    {
        SocketEndpoint::Unix(socket_path())
    }
}

pub fn load_socket_auth_token() -> Option<String> {
    for key in ["ROUX_DAEMON_TOKEN", "ROUX_AUTH_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    std::fs::read_to_string(socket_auth_token_file_path())
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn roux_cli_file_name() -> &'static str {
    roux_cli_file_name_for_platform(cfg!(windows))
}

fn roux_cli_file_name_for_platform(is_windows: bool) -> &'static str {
    if is_windows {
        "roux.exe"
    } else {
        "roux"
    }
}

pub fn sibling_roux_cli_path(exe_path: &Path) -> Option<PathBuf> {
    exe_path.parent().map(|dir| dir.join(roux_cli_file_name()))
}

pub fn quote_command_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }

    if !arg.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '\\')) {
        return arg.to_string();
    }

    let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

pub fn command_string(program: &Path, args: &[&str]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(quote_command_arg(&program.to_string_lossy()));
    parts.extend(args.iter().map(|arg| quote_command_arg(arg)));
    parts.join(" ")
}

fn windows_command_candidates(file_name: &str) -> Vec<String> {
    let mut candidates = vec![file_name.to_string()];
    if Path::new(file_name).extension().is_some() {
        return candidates;
    }

    let extensions = std::env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .filter_map(|ext| {
                    let ext = ext.trim();
                    if ext.is_empty() {
                        None
                    } else {
                        Some(ext.to_string())
                    }
                })
                .collect::<Vec<_>>()
        })
        .filter(|exts| !exts.is_empty())
        .unwrap_or_else(|| vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()]);

    for ext in extensions {
        candidates.push(format!("{}{}", file_name, ext));
    }

    candidates
}

pub fn find_executable_on_path(file_name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    find_executable_in_paths(&paths, file_name)
}

/// Search a specific `PATH` string (colon-separated on Unix, semicolon on
/// Windows) for `file_name`. Used when the process-inherited PATH is not
/// sufficient — notably for macOS GUI apps where the user's login-shell
/// PATH is richer than what Launch Services hands us.
pub fn find_executable_in_paths<P: AsRef<std::ffi::OsStr>>(
    paths: P,
    file_name: &str,
) -> Option<PathBuf> {
    let candidates = if cfg!(windows) {
        windows_command_candidates(file_name)
    } else {
        vec![file_name.to_string()]
    };

    std::env::split_paths(&paths)
        .flat_map(|dir| candidates.iter().map(move |candidate| dir.join(candidate)))
        .find(|path| is_executable_file(path))
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn app_config_dir_ends_with_roux() {
        assert_eq!(app_config_dir().file_name().and_then(|n| n.to_str()), Some("roux"));
    }

    #[test]
    fn status_dir_lives_under_app_config_dir() {
        assert_eq!(status_dir(), app_config_dir().join("status"));
    }

    #[test]
    fn parse_socket_endpoint_accepts_tcp_scheme() {
        assert_eq!(
            parse_socket_endpoint(" tcp://127.0.0.1:7777 "),
            Some(SocketEndpoint::Tcp("127.0.0.1:7777".to_string()))
        );
    }

    #[test]
    fn parse_socket_endpoint_accepts_unix_scheme() {
        assert_eq!(
            parse_socket_endpoint("unix:///tmp/roux.sock"),
            Some(SocketEndpoint::Unix(PathBuf::from("/tmp/roux.sock")))
        );
        assert_eq!(parse_socket_endpoint(""), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn parse_socket_endpoint_treats_plain_value_as_unix_path_on_unix() {
        assert_eq!(
            parse_socket_endpoint("/tmp/roux.sock"),
            Some(SocketEndpoint::Unix(PathBuf::from("/tmp/roux.sock")))
        );
    }

    #[cfg(windows)]
    #[test]
    fn parse_socket_endpoint_treats_plain_value_as_tcp_addr_on_windows() {
        assert_eq!(
            parse_socket_endpoint("127.0.0.1:7777"),
            Some(SocketEndpoint::Tcp("127.0.0.1:7777".to_string()))
        );
    }

    #[test]
    fn roux_cli_file_name_uses_exe_extension_on_windows() {
        assert_eq!(roux_cli_file_name_for_platform(true), "roux.exe");
        assert_eq!(roux_cli_file_name_for_platform(false), "roux");
    }

    #[test]
    fn quote_command_arg_quotes_spaces_and_quotes() {
        assert_eq!(quote_command_arg("roux"), "roux");
        assert_eq!(quote_command_arg(""), "\"\"");
        assert_eq!(
            quote_command_arg("C:\\Program Files\\Roux\\roux.exe"),
            "\"C:\\\\Program Files\\\\Roux\\\\roux.exe\""
        );
        assert_eq!(quote_command_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn command_string_quotes_program_path_and_args() {
        let command = command_string(
            Path::new("C:\\Users\\Sam\\App Data\\Roux\\roux.exe"),
            &["hook", "working"],
        );
        assert_eq!(command, "\"C:\\\\Users\\\\Sam\\\\App Data\\\\Roux\\\\roux.exe\" hook working");
    }

    #[test]
    fn windows_command_candidates_include_executable_extensions() {
        let candidates = windows_command_candidates("gh");
        assert!(candidates.iter().any(|candidate| candidate.eq_ignore_ascii_case("gh.exe")));
    }

    #[cfg(unix)]
    #[test]
    fn find_executable_in_paths_skips_non_executable_files() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("roux");
        fs::write(&candidate, "").unwrap();
        let paths = std::env::join_paths([dir.path()]).unwrap();

        assert_eq!(find_executable_in_paths(&paths, "roux"), None);

        let mut permissions = fs::metadata(&candidate).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&candidate, permissions).unwrap();

        assert_eq!(find_executable_in_paths(&paths, "roux"), Some(candidate));
    }
}
