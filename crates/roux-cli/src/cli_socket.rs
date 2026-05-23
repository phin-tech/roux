use serde_json::Value;

pub fn send_socket_command(request: Value) -> roux_sdk::RouxResult<Value> {
    roux_sdk::blocking::send_socket_command(request)
}

pub fn stream_socket_command<F>(request: Value, on_line: F) -> roux_sdk::RouxResult<()>
where
    F: FnMut(&str) -> bool,
{
    roux_sdk::blocking::stream_socket_command(request, on_line)
}
