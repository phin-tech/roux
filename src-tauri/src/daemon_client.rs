use serde::Deserialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::time::Duration;

use roux_core::{Project, Session};

use crate::platform;

const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonStatus {
    pub(crate) kind: String,
    pub(crate) pid: u32,
    pub(crate) socket: String,
    pub(crate) started_at_ms: u64,
    pub(crate) uptime_ms: u64,
    pub(crate) session_count: usize,
    pub(crate) project_count: usize,
    pub(crate) capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonClient {
    status: DaemonStatus,
}

impl DaemonClient {
    pub(crate) fn detect() -> Option<Self> {
        let data =
            send_command_blocking(serde_json::json!({ "command": "daemon-status" }), PROBE_TIMEOUT)
                .ok()?;
        let status: DaemonStatus = serde_json::from_value(data).ok()?;
        if status.kind == "roux-daemon" {
            Some(Self { status })
        } else {
            None
        }
    }

    pub(crate) fn status(&self) -> &DaemonStatus {
        &self.status
    }

    pub(crate) async fn list_sessions(&self) -> Result<Vec<Session>, String> {
        let value = send_command_async(serde_json::json!({ "command": "session-list" })).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon session-list: {err}"))
    }

    pub(crate) async fn list_projects(&self) -> Result<Vec<Project>, String> {
        let value = send_command_async(serde_json::json!({ "command": "project-list" })).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon project-list: {err}"))
    }

    pub(crate) async fn set_session_name_override(
        &self,
        session_id: String,
        name_override: Option<String>,
    ) -> Result<(), String> {
        let name = name_override.unwrap_or_default();
        let _ = send_command_async(serde_json::json!({
            "command": "session-rename",
            "session_id": session_id,
            "args": { "name": name },
        }))
        .await?;
        Ok(())
    }
}

async fn send_command_async(request: Value) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || send_command_blocking(request, COMMAND_TIMEOUT))
        .await
        .map_err(|err| format!("daemon client task failed: {err}"))?
}

#[derive(Debug, Deserialize)]
struct Response {
    ok: bool,
    data: Option<Value>,
    error: Option<String>,
}

fn decode_response(raw: &str) -> Result<Value, String> {
    let response: Response = serde_json::from_str(raw.trim())
        .map_err(|err| format!("invalid daemon response: {err}"))?;
    if response.ok {
        Ok(response.data.unwrap_or(Value::Null))
    } else {
        Err(response.error.unwrap_or_else(|| "daemon command failed".to_string()))
    }
}

fn send_command_blocking(request: Value, timeout: Duration) -> Result<Value, String> {
    #[cfg(windows)]
    {
        send_tcp_command(request, timeout)
    }
    #[cfg(not(windows))]
    {
        send_unix_command(request, timeout)
    }
}

#[cfg(not(windows))]
fn send_unix_command(request: Value, timeout: Duration) -> Result<Value, String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("set daemon read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;

    write_request(&mut stream, request)?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|err| format!("read daemon response: {err}"))?;
    decode_response(&raw)
}

#[cfg(windows)]
fn send_tcp_command(mut request: Value, timeout: Duration) -> Result<Value, String> {
    use std::net::{Shutdown, TcpStream};

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("set daemon read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;

    write_request(&mut stream, request)?;
    let _ = stream.shutdown(Shutdown::Write);
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|err| format!("read daemon response: {err}"))?;
    decode_response(&raw)
}

fn write_request(stream: &mut impl Write, request: Value) -> Result<(), String> {
    let json = serde_json::to_string(&request).map_err(|err| format!("encode request: {err}"))?;
    stream
        .write_all(json.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|err| format!("write daemon request: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_response_returns_data_on_success() {
        let data = decode_response(r#"{"ok":true,"data":{"kind":"roux-daemon"}}"#).unwrap();
        assert_eq!(data["kind"], "roux-daemon");
    }

    #[test]
    fn decode_response_returns_error_message() {
        let err = decode_response(r#"{"ok":false,"error":"nope"}"#).unwrap_err();
        assert_eq!(err, "nope");
    }
}
