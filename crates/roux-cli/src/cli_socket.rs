use serde_json::Value;

use crate::platform::{self, SocketEndpoint};

/// Connect to Roux and stream a long-lived command (e.g. `mailbox-watch`).
/// Each newline-delimited JSON line the server emits is passed to
/// `on_line`. Returns when the server closes the connection or
/// `on_line` returns false. Read timeout is intentionally absent —
/// streaming commands block until events arrive.
pub fn stream_socket_command<F>(request: Value, mut on_line: F) -> Result<(), String>
where
    F: FnMut(&str) -> bool,
{
    use std::io::BufRead;

    let endpoint = platform::resolve_socket_endpoint_spec()
        .ok_or_else(|| "Roux is not running".to_string())?;
    let stream = connect_stream_socket(endpoint, request)?;

    stream.send_request()?;

    let reader = stream.into_reader();
    let mut reader = std::io::BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()), // server closed
            Ok(_) => {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                if !on_line(trimmed) {
                    return Ok(());
                }
            }
            Err(e) => return Err(format!("Read failed: {e}")),
        }
    }
}

fn map_connect_err(e: std::io::Error) -> String {
    if matches!(
        e.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::AddrNotAvailable
    ) {
        "Roux is not running".to_string()
    } else {
        format!("Failed to connect to Roux: {e}")
    }
}

/// Internal trait so `stream_socket_command` works for both Unix and
/// Windows transports without duplicating the loop. Each impl carries
/// the request JSON it was constructed with so `send_request` is a
/// straightforward "serialize + write line."
trait StreamSocket {
    fn send_request(&self) -> Result<(), String>;
    fn into_reader(self: Box<Self>) -> Box<dyn std::io::Read + Send>;
}

#[cfg(not(windows))]
struct UnixStreamHolder(std::os::unix::net::UnixStream, Value);

#[cfg(not(windows))]
impl StreamSocket for UnixStreamHolder {
    fn send_request(&self) -> Result<(), String> {
        use std::io::Write;
        let json = serde_json::to_string(&self.1).unwrap();
        let mut s = &self.0;
        s.write_all(json.as_bytes())
            .and_then(|_| s.write_all(b"\n"))
            .map_err(|e| format!("Failed to send command: {e}"))
    }
    fn into_reader(self: Box<Self>) -> Box<dyn std::io::Read + Send> {
        Box::new(self.0)
    }
}

struct TcpStreamHolder(std::net::TcpStream, Value);

impl StreamSocket for TcpStreamHolder {
    fn send_request(&self) -> Result<(), String> {
        use std::io::Write;
        let json = serde_json::to_string(&self.1).unwrap();
        let mut s = &self.0;
        s.write_all(json.as_bytes())
            .and_then(|_| s.write_all(b"\n"))
            .map_err(|e| format!("Failed to send command: {e}"))
    }
    fn into_reader(self: Box<Self>) -> Box<dyn std::io::Read + Send> {
        Box::new(self.0)
    }
}

fn connect_stream_socket(
    endpoint: SocketEndpoint,
    request: Value,
) -> Result<Box<dyn StreamSocket>, String> {
    match endpoint {
        SocketEndpoint::Unix(path) => connect_unix_stream_socket(path, request),
        SocketEndpoint::Tcp(addr) => {
            let request = add_auth_token(request)?;
            let s = std::net::TcpStream::connect(&addr).map_err(map_connect_err)?;
            Ok(Box::new(TcpStreamHolder(s, request)))
        }
    }
}

#[cfg(not(windows))]
fn connect_unix_stream_socket(
    path: std::path::PathBuf,
    request: Value,
) -> Result<Box<dyn StreamSocket>, String> {
    let s = std::os::unix::net::UnixStream::connect(&path).map_err(map_connect_err)?;
    Ok(Box::new(UnixStreamHolder(s, request)))
}

#[cfg(windows)]
fn connect_unix_stream_socket(
    _path: std::path::PathBuf,
    _request: Value,
) -> Result<Box<dyn StreamSocket>, String> {
    Err("Unix socket endpoints are not supported on Windows".to_string())
}

pub fn send_socket_command(request: Value) -> Result<Value, String> {
    let endpoint = platform::resolve_socket_endpoint_spec()
        .ok_or_else(|| "Roux is not running".to_string())?;
    match endpoint {
        SocketEndpoint::Unix(path) => send_unix_socket_command(path, request),
        SocketEndpoint::Tcp(addr) => send_tcp_socket_command(addr, request),
    }
}

fn add_auth_token(mut request: Value) -> Result<Value, String> {
    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "Roux command channel token not found".to_string())?;
    if let Some(request_obj) = request.as_object_mut() {
        request_obj.insert("auth_token".to_string(), Value::String(auth_token));
    }
    Ok(request)
}

fn send_tcp_socket_command(addr: String, request: Value) -> Result<Value, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let request = add_auth_token(request)?;
    let mut stream = TcpStream::connect(&addr).map_err(map_connect_err)?;

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("Failed to set timeout: {}", e))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("Failed to set timeout: {}", e))?;

    let json = serde_json::to_string(&request).unwrap();
    stream.write_all(json.as_bytes()).map_err(|e| format!("Failed to send command: {}", e))?;
    stream.write_all(b"\n").map_err(|e| format!("Failed to send command: {}", e))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Failed to shutdown write: {}", e))?;

    let mut response = String::new();
    let mut reader = std::io::BufReader::new(stream);
    reader.read_to_string(&mut response).map_err(|e| format!("Failed to read response: {}", e))?;

    serde_json::from_str(&response).map_err(|e| format!("Invalid response: {}", e))
}

#[cfg(not(windows))]
fn send_unix_socket_command(path: std::path::PathBuf, request: Value) -> Result<Value, String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let stream = UnixStream::connect(&path).map_err(map_connect_err)?;

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("Failed to set timeout: {}", e))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("Failed to set timeout: {}", e))?;

    let json = serde_json::to_string(&request).unwrap();
    let mut stream_ref = &stream;
    stream_ref.write_all(json.as_bytes()).map_err(|e| format!("Failed to send command: {}", e))?;
    stream_ref.write_all(b"\n").map_err(|e| format!("Failed to send command: {}", e))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Failed to shutdown write: {}", e))?;

    let mut response = String::new();
    let mut reader = std::io::BufReader::new(&stream);
    reader.read_to_string(&mut response).map_err(|e| format!("Failed to read response: {}", e))?;

    serde_json::from_str(&response).map_err(|e| format!("Invalid response: {}", e))
}

#[cfg(windows)]
fn send_unix_socket_command(_path: std::path::PathBuf, _request: Value) -> Result<Value, String> {
    Err("Unix socket endpoints are not supported on Windows".to_string())
}
