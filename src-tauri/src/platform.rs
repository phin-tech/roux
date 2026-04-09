#![allow(dead_code)]

use std::path::{Path, PathBuf};

const APP_DIR_NAME: &str = "roux";

pub fn app_config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR_NAME)
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

pub fn task_overrides_path() -> PathBuf {
    app_config_dir().join("task-overrides.json")
}

pub fn log_dir() -> PathBuf {
    app_config_dir().join("logs")
}

pub fn socket_path() -> PathBuf {
    app_config_dir().join("roux.sock")
}

#[cfg(windows)]
pub fn socket_addr_file_path() -> PathBuf {
    app_config_dir().join("roux-socket-addr")
}

pub fn resolve_socket_endpoint() -> Option<String> {
    if let Ok(endpoint) = std::env::var("ROUX_SOCKET") {
        let endpoint = endpoint.trim();
        if !endpoint.is_empty() {
            return Some(endpoint.to_string());
        }
    }

    #[cfg(windows)]
    {
        return std::fs::read_to_string(socket_addr_file_path())
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }

    #[cfg(not(windows))]
    {
        Some(socket_path().to_string_lossy().to_string())
    }
}

pub fn roux_cli_file_name() -> &'static str {
    roux_cli_file_name_for_platform(cfg!(windows))
}

fn roux_cli_file_name_for_platform(is_windows: bool) -> &'static str {
    if is_windows {
        "roux-cli.exe"
    } else {
        "roux-cli"
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

pub fn find_executable_on_path(file_name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|dir| dir.join(file_name)).find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn app_config_dir_ends_with_roux() {
        assert_eq!(app_config_dir().file_name().and_then(|n| n.to_str()), Some("roux"));
    }

    #[test]
    fn status_dir_lives_under_app_config_dir() {
        assert_eq!(status_dir(), app_config_dir().join("status"));
    }

    #[test]
    fn roux_cli_file_name_uses_exe_extension_on_windows() {
        assert_eq!(roux_cli_file_name_for_platform(true), "roux-cli.exe");
        assert_eq!(roux_cli_file_name_for_platform(false), "roux-cli");
    }

    #[test]
    fn quote_command_arg_quotes_spaces_and_quotes() {
        assert_eq!(quote_command_arg("roux-cli"), "roux-cli");
        assert_eq!(quote_command_arg(""), "\"\"");
        assert_eq!(
            quote_command_arg("C:\\Program Files\\Roux\\roux-cli.exe"),
            "\"C:\\\\Program Files\\\\Roux\\\\roux-cli.exe\""
        );
        assert_eq!(quote_command_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn command_string_quotes_program_path_and_args() {
        let command = command_string(
            Path::new("C:\\Users\\Sam\\App Data\\Roux\\roux-cli.exe"),
            &["hook", "working"],
        );
        assert_eq!(
            command,
            "\"C:\\\\Users\\\\Sam\\\\App Data\\\\Roux\\\\roux-cli.exe\" hook working"
        );
    }
}
