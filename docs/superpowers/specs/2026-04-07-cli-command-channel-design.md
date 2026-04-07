# Roux CLI Command Channel

## Summary

Extend `roux-cli` to send commands back to the Roux app over a Unix domain socket. This enables terminals running inside Roux to split panes, create sessions, open shells, and send text to Claude -- all from the command line.

## Architecture

```
┌─────────────┐    Unix Socket     ┌──────────────────┐    Tauri Event    ┌──────────┐
│  roux-cli   │ ──── JSON ──────▶  │  Socket Server    │ ───────────────▶  │ Frontend │
│  (std sync) │ ◀── JSON ───────   │  (tokio, Rust)    │                   │ (Svelte) │
└─────────────┘                    └──────────────────┘                    └──────────┘
```

- **Transport:** Unix domain socket at `~/.config/roux/roux.sock`
- **Protocol:** JSON request/response, one per connection (connect, send, receive, close)
- **Server:** `tokio::net::UnixListener` running inside the existing Tauri Tokio runtime
- **Client:** `std::os::unix::net::UnixStream` (synchronous, no Tokio dependency in CLI)
- **Platform:** macOS only for now

## Environment Variables

Set on every PTY spawned by Roux (in `pty.rs` spawn functions):

| Variable | Value | Purpose |
|----------|-------|---------|
| `ROUX_SESSION` | `1` | Indicates terminal is inside Roux (already implemented) |
| `ROUX_SOCKET` | `~/.config/roux/roux.sock` | Socket path for CLI to connect to |
| `ROUX_PANE_ID` | `<pane-id>` | Current pane, used for implicit targeting |
| `ROUX_SESSION_ID` | `<session-id>` | Current session, used for implicit targeting |

## Protocol

### Request

```json
{
  "command": "split",
  "session_id": "abc123",
  "pane_id": "pane-1",
  "args": { "direction": "horizontal" }
}
```

The CLI auto-populates `session_id` and `pane_id` from environment variables. Explicit flags override.

### Response

Success:
```json
{ "ok": true, "data": { "pane_id": "pane-2" } }
```

Error:
```json
{ "ok": false, "error": "no active session" }
```

## Commands

| Command | CLI Usage | Args | Description |
|---------|-----------|------|-------------|
| `split` | `roux split -d horizontal` | `direction` (horizontal/vertical) | Split the current pane |
| `session-create` | `roux session create --name "backend"` | `name?`, `working_dir?` | Create a new Claude session |
| `shell` | `roux shell` | `working_dir?` | Open a shell pane |
| `focus` | `roux focus --session abc123` | `pane_id?`, `session_id?` | Focus a pane or session tab |
| `run` | `roux run "npm test"` | `command` | Run a command in a new pane |
| `send` | `roux send "fix the tests"` | `text` | Send text to the active Claude pane |

## Components

### 1. Socket Server (`src-tauri/src/socket.rs`)

New module. Tokio-based `UnixListener` that:

- Binds to `~/.config/roux/roux.sock` on app startup
- Removes stale socket file before binding (handles unclean shutdown)
- Accepts connections, reads JSON request, dispatches to command handler, writes JSON response
- Cleans up socket file on app shutdown
- Started from `main.rs` as a Tokio task alongside the Tauri app

### 2. Command Dispatcher

Lives within `socket.rs` or a dedicated `socket_commands.rs`. Maps incoming command strings to actions:

- **Frontend-driven actions** (`split`, `focus`): Emits Tauri events that the Svelte frontend listens for and executes (since pane tree state lives in the frontend)
- **Backend-driven actions** (`session-create`, `send`, `shell`, `run`): Calls existing PTY/session functions in Rust directly, returns result

### 3. CLI Extension (`src-tauri/src/cli.rs`)

Extend existing clap-based CLI with new subcommands. Each subcommand:

1. Reads `ROUX_SOCKET` env var (falls back to default path)
2. Reads `ROUX_SESSION_ID` and `ROUX_PANE_ID` env vars for implicit targeting
3. Connects to unix socket
4. Sends JSON request
5. Reads JSON response
6. Prints result or error to stdout/stderr
7. Exits with appropriate code (0 for success, 1 for error)

### 4. Environment Variables (`src-tauri/src/pty.rs`)

Add `ROUX_SOCKET`, `ROUX_PANE_ID`, and `ROUX_SESSION_ID` to all three spawn functions (`spawn`, `spawn_shell`, `spawn_task`).

### 5. Frontend Event Handlers (`src/lib/tauri.ts` + stores)

Listen for new Tauri events emitted by the socket server:

- `roux-command:split` — calls pane store split logic
- `roux-command:focus` — calls pane/session focus logic

These reuse existing store functions, just triggered from a new event source.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Socket file doesn't exist | CLI prints "Roux is not running" and exits 1 |
| Connection refused | CLI prints "Roux is not running" and exits 1 |
| Unknown command | Response: `{ "ok": false, "error": "unknown command: foo" }` |
| Missing/invalid pane or session | Response with descriptive error message |
| Socket file exists from previous crash | Server removes stale file before binding |
| Read/write timeout | CLI uses a 5-second timeout, prints error on expiry |

## Security

- Unix socket with `0600` permissions (owner-only access)
- No network exposure -- not accessible from other machines or browser-based attacks
- No authentication needed since filesystem permissions restrict access to current user

## Testing

- Unit tests for command parsing and dispatch in Rust
- Integration test: spawn socket server, connect with std UnixStream, verify round-trip
- CLI tests: run `roux-cli` subcommands against a test socket
- Frontend tests: verify event handlers trigger correct store mutations
