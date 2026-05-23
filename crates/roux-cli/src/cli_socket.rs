use serde_json::Value;

pub fn send_socket_command(request: Value) -> Result<Value, String> {
    roux_sdk::blocking::send_socket_command(request).map_err(|err| err.to_string())
}

pub fn stream_socket_command<F>(request: Value, on_line: F) -> Result<(), String>
where
    F: FnMut(&str) -> bool,
{
    roux_sdk::blocking::stream_socket_command(request, on_line).map_err(|err| err.to_string())
}
