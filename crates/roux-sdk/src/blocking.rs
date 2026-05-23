use crate::{CommandRequest, CommandResponse, RouxResult, SocketEndpoint};
use crate::{RouxBuilder, RouxError};
use serde_json::Value;
use std::io::{BufRead, Read, Write};
use std::time::Duration;

pub fn send_socket_command(request: Value) -> RouxResult<Value> {
    let client = RouxBuilder::default().connect()?;
    let request: CommandRequest = serde_json::from_value(request).map_err(RouxError::Decode)?;
    client.command_blocking(request)
}

pub fn stream_socket_command<F>(request: Value, on_line: F) -> RouxResult<()>
where
    F: FnMut(&str) -> bool,
{
    let client = RouxBuilder::default().connect()?;
    let request: CommandRequest = serde_json::from_value(request).map_err(RouxError::Decode)?;
    stream_request(&client.endpoint, client.auth_token.as_deref(), request, on_line)
}

pub(crate) fn stream_client_request<F>(
    endpoint: &SocketEndpoint,
    auth_token: Option<&str>,
    request: CommandRequest,
    on_line: F,
) -> RouxResult<()>
where
    F: FnMut(&str) -> bool,
{
    stream_request(endpoint, auth_token, request, on_line)
}

pub(crate) fn send_request(
    endpoint: &SocketEndpoint,
    auth_token: Option<&str>,
    timeout: Duration,
    request: CommandRequest,
) -> RouxResult<CommandResponse> {
    let request = request_with_auth(endpoint, auth_token, request)?;
    match endpoint {
        SocketEndpoint::Unix(path) => send_unix_socket_command(path, timeout, request),
        SocketEndpoint::Tcp(addr) => send_tcp_socket_command(addr, timeout, request),
    }
}

fn request_with_auth(
    endpoint: &SocketEndpoint,
    auth_token: Option<&str>,
    mut request: CommandRequest,
) -> RouxResult<CommandRequest> {
    if matches!(endpoint, SocketEndpoint::Tcp(_)) && request.auth_token.is_none() {
        let token = auth_token.ok_or_else(|| {
            RouxError::Transport("Roux command channel token not found".to_string())
        })?;
        request.auth_token = Some(token.to_string());
    }
    Ok(request)
}

fn map_connect_err(e: std::io::Error) -> RouxError {
    if matches!(
        e.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::AddrNotAvailable
    ) {
        RouxError::NotRunning
    } else {
        RouxError::Transport(format!("Failed to connect to Roux: {e}"))
    }
}

fn send_tcp_socket_command(
    addr: &str,
    timeout: Duration,
    request: CommandRequest,
) -> RouxResult<CommandResponse> {
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(addr).map_err(map_connect_err)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| RouxError::Transport(format!("Failed to set timeout: {e}")))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| RouxError::Transport(format!("Failed to set timeout: {e}")))?;

    write_request(&mut stream, &request)?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| RouxError::Transport(format!("Failed to shutdown write: {e}")))?;

    let mut response = String::new();
    let mut reader = std::io::BufReader::new(stream);
    reader
        .read_to_string(&mut response)
        .map_err(|e| RouxError::Transport(format!("Failed to read response: {e}")))?;

    serde_json::from_str(&response).map_err(RouxError::Decode)
}

#[cfg(not(windows))]
fn send_unix_socket_command(
    path: &std::path::Path,
    timeout: Duration,
    request: CommandRequest,
) -> RouxResult<CommandResponse> {
    use std::os::unix::net::UnixStream;

    let stream = UnixStream::connect(path).map_err(map_connect_err)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| RouxError::Transport(format!("Failed to set timeout: {e}")))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| RouxError::Transport(format!("Failed to set timeout: {e}")))?;

    let mut stream_ref = &stream;
    write_request(&mut stream_ref, &request)?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| RouxError::Transport(format!("Failed to shutdown write: {e}")))?;

    let mut response = String::new();
    let mut reader = std::io::BufReader::new(&stream);
    reader
        .read_to_string(&mut response)
        .map_err(|e| RouxError::Transport(format!("Failed to read response: {e}")))?;

    serde_json::from_str(&response).map_err(RouxError::Decode)
}

#[cfg(windows)]
fn send_unix_socket_command(
    _path: &std::path::Path,
    _timeout: Duration,
    _request: CommandRequest,
) -> RouxResult<CommandResponse> {
    Err(RouxError::Transport("Unix socket endpoints are not supported on Windows".to_string()))
}

fn write_request<W: Write>(writer: &mut W, request: &CommandRequest) -> RouxResult<()> {
    let json = serde_json::to_string(request).map_err(RouxError::Decode)?;
    writer
        .write_all(json.as_bytes())
        .and_then(|_| writer.write_all(b"\n"))
        .map_err(|e| RouxError::Transport(format!("Failed to send command: {e}")))
}

fn stream_request<F>(
    endpoint: &SocketEndpoint,
    auth_token: Option<&str>,
    request: CommandRequest,
    on_line: F,
) -> RouxResult<()>
where
    F: FnMut(&str) -> bool,
{
    let request = request_with_auth(endpoint, auth_token, request)?;
    match endpoint {
        SocketEndpoint::Unix(path) => stream_unix_socket(path, request, on_line),
        SocketEndpoint::Tcp(addr) => stream_tcp_socket(addr, request, on_line),
    }
}

fn stream_tcp_socket<F>(addr: &str, request: CommandRequest, on_line: F) -> RouxResult<()>
where
    F: FnMut(&str) -> bool,
{
    let stream = std::net::TcpStream::connect(addr).map_err(map_connect_err)?;
    stream_loop(stream, request, on_line)
}

#[cfg(not(windows))]
fn stream_unix_socket<F>(
    path: &std::path::Path,
    request: CommandRequest,
    on_line: F,
) -> RouxResult<()>
where
    F: FnMut(&str) -> bool,
{
    let stream = std::os::unix::net::UnixStream::connect(path).map_err(map_connect_err)?;
    stream_loop(stream, request, on_line)
}

#[cfg(windows)]
fn stream_unix_socket<F>(
    _path: &std::path::Path,
    _request: CommandRequest,
    _on_line: F,
) -> RouxResult<()>
where
    F: FnMut(&str) -> bool,
{
    Err(RouxError::Transport("Unix socket endpoints are not supported on Windows".to_string()))
}

fn stream_loop<S, F>(stream: S, request: CommandRequest, mut on_line: F) -> RouxResult<()>
where
    S: Read + Write + Send + 'static,
    F: FnMut(&str) -> bool,
{
    let mut writer = stream;
    write_request(&mut writer, &request)?;
    let mut reader = std::io::BufReader::new(writer);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()),
            Ok(_) => {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                if !on_line(trimmed) {
                    return Ok(());
                }
            }
            Err(e) => return Err(RouxError::Transport(format!("Read failed: {e}"))),
        }
    }
}
