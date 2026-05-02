use serde_json::Value;

use crate::platform;

pub fn send_socket_command(request: Value) -> Result<Value, String> {
    #[cfg(windows)]
    {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let mut request = request;
        let auth_token = platform::load_socket_auth_token()
            .ok_or_else(|| "Roux command channel token not found".to_string())?;
        if let Some(request_obj) = request.as_object_mut() {
            request_obj.insert("auth_token".to_string(), Value::String(auth_token));
        }

        let endpoint =
            platform::resolve_socket_endpoint().ok_or_else(|| "Roux is not running".to_string())?;
        let mut stream = TcpStream::connect(&endpoint).map_err(|e| {
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::AddrNotAvailable
            ) {
                "Roux is not running".to_string()
            } else {
                format!("Failed to connect to Roux: {}", e)
            }
        })?;

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
        reader
            .read_to_string(&mut response)
            .map_err(|e| format!("Failed to read response: {}", e))?;

        return serde_json::from_str(&response).map_err(|e| format!("Invalid response: {}", e));
    }

    #[cfg(not(windows))]
    {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        let path = platform::socket_path();
        let stream = UnixStream::connect(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::ConnectionRefused
            {
                "Roux is not running".to_string()
            } else {
                format!("Failed to connect to Roux: {}", e)
            }
        })?;

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;

        let json = serde_json::to_string(&request).unwrap();
        let mut stream_ref = &stream;
        stream_ref
            .write_all(json.as_bytes())
            .map_err(|e| format!("Failed to send command: {}", e))?;
        stream_ref.write_all(b"\n").map_err(|e| format!("Failed to send command: {}", e))?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|e| format!("Failed to shutdown write: {}", e))?;

        let mut response = String::new();
        let mut reader = std::io::BufReader::new(&stream);
        reader
            .read_to_string(&mut response)
            .map_err(|e| format!("Failed to read response: {}", e))?;

        serde_json::from_str(&response).map_err(|e| format!("Invalid response: {}", e))
    }
}
