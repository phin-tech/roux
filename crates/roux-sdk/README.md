# roux-sdk

Typed Rust SDK for Roux's daemon-first local API.

The SDK talks to the headless Roux daemon over the same JSON line protocol used
by the CLI. It does not target the desktop-only socket server. The desktop
socket can be retired separately once the app fully relies on the daemon for
externally addressable behavior.

## Rust usage

```rust
use roux_sdk::Roux;

# async fn example() -> roux_sdk::RouxResult<()> {
let roux = Roux::connect()?;

let status = roux.status().await?;
let sessions = roux.sessions().await?;

let pty = roux
    .spawn_task("printf hello")
    .working_dir("/tmp")
    .initial_size(100, 30)
    .spawn()
    .await?;

let snapshot = pty.snapshot(4096).await?;
# Ok(())
# }
```

`Roux::builder()` can override the endpoint, auth token, and request timeout.
The default endpoint resolution matches the CLI:

- `ROUX_SOCKET=tcp://127.0.0.1:7777` uses TCP.
- `ROUX_SOCKET=unix:///path/to/roux.sock` uses a Unix socket.
- `ROUX_SOCKET=/path/to/roux.sock` uses a Unix socket on Unix/macOS.
- Without `ROUX_SOCKET`, Unix/macOS uses `~/.config/roux/roux.sock`.
- Windows reads the daemon TCP endpoint and token files from `~/.config/roux`.

TCP requests require an auth token. The SDK uses an explicit builder token
first, then `ROUX_DAEMON_TOKEN`, then `ROUX_AUTH_TOKEN`, then the daemon token
file.

## Protocol shape

One-shot requests are newline-terminated JSON objects:

```json
{"command":"daemon-status","args":null}
```

Successful responses are:

```json
{"ok":true,"data":{"kind":"roux-daemon","capabilities":["daemon-status"]}}
```

Failures are:

```json
{"ok":false,"error":"daemon pty not found"}
```

Streaming commands use newline-delimited JSON frames after the initial request.
For PTY attach:

```json
{"command":"daemon-pty-attach","args":{"id":"pty-1","maxBytes":65536}}
{"type":"ready","id":"pty-1","record":{},"replayOffset":0,"replayBytes":[]}
{"type":"output","offset":0,"bytes":[104,101,108,108,111]}
{"type":"exit","code":0,"generation":1}
```

The Rust SDK owns the typed request, response, model, and frame shapes. Other
language clients can use these protocol examples immediately; a C ABI/Python
package can wrap the same protocol or a later FFI layer.
