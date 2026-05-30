use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketEndpoint {
    Unix(PathBuf),
    Tcp(String),
}

impl SocketEndpoint {
    pub fn display_value(&self) -> String {
        match self {
            Self::Unix(path) => path.to_string_lossy().into_owned(),
            Self::Tcp(addr) => format!("tcp://{addr}"),
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

pub fn resolve_socket_endpoint() -> Option<SocketEndpoint> {
    if let Ok(endpoint) = std::env::var("ROUX_SOCKET") {
        if let Some(endpoint) = parse_socket_endpoint(&endpoint) {
            return Some(endpoint);
        }
    }

    #[cfg(windows)]
    {
        std::fs::read_to_string(socket_addr_file_path())
            .ok()
            .and_then(|value| parse_socket_endpoint(&value))
    }

    #[cfg(not(windows))]
    {
        Some(SocketEndpoint::Unix(socket_path()))
    }
}

pub(crate) fn load_socket_auth_token() -> Option<String> {
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

fn socket_path() -> PathBuf {
    roux_core::paths::roux_config_dir().join("roux.sock")
}

#[cfg(windows)]
fn socket_addr_file_path() -> PathBuf {
    roux_core::paths::roux_config_dir().join("roux-socket-addr")
}

fn socket_auth_token_file_path() -> PathBuf {
    roux_core::paths::roux_config_dir().join("roux-socket-token")
}
