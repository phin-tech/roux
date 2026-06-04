# Troubleshooting

Common issues and how to work around them.

!!! note "Stub page"
This page is a placeholder. Real troubleshooting entries will land here as they come up in practice.

## Claude Code isn't found

Roux shells out to the `claude` command. If new sessions fail to start, verify Claude Code is installed and on your `PATH` by running `claude --version` in a regular terminal.

## A pane shows nothing after restart

Shell panes are respawned automatically on launch, but Claude sessions are not restarted by default. Open the command palette (++cmd+k++) and start a new session in that pane.

## MCP host says Roux is disabled

Open **Settings → Agent Integrations** and turn on **Enable Roux MCP**. MCP hosts may still be able to launch `roux mcp` when this is off, but the server will reject tool calls until Roux MCP is enabled.

## MCP host cannot connect to Roux

`roux mcp` talks to the running Roux app over the local socket bridge. Make sure the Roux desktop app is open, then retry the MCP host action.

If the host was configured before a Roux update, open **Settings → Agent Integrations** and check the CLI status. If the CLI is missing or stale, update/reinstall the CLI from Roux's setup or Doctor controls, then run **Configure** for the host again.

## MCP host config preview shows an error

Roux reads the host's existing MCP config before writing. If the config is malformed JSON or has a non-object `mcpServers` field, Roux will show the error instead of overwriting the file.

Fix the host config manually, then return to **Settings → Agent Integrations** and use **Preview** again. Roux only adds or updates its own `roux` server entry and preserves unrelated host config.

## `roux_get_latest_output` has no `text` field

Latest output is backed by raw PTY replay bytes. Roux always returns the exact bytes as `replay_bytes_base64`; it only includes `text` when those bytes are valid UTF-8. Decode `replay_bytes_base64` if your MCP client needs exact terminal output.

## Work-item DB migration status looks wrong

The Advanced settings panel shows the daemon's live `board.db` migration
status under **Runtime → Database**. This is read-only diagnostic state. Roux
migrates the work-item database automatically when the daemon opens it; there
is no manual migration button.

Normal state:

- **Storage:** `board.db`
- **Current migration:** same as **Target migration**
- **Pending:** `None`
- **Migration error:** absent

Anything else means you should first prove which database and daemon you are
looking at. Most false alarms are caused by reading the wrong config root or a
stale daemon.

### Compare UI and daemon status

Run:

```bash
roux daemon status
```

Inspect `workItemMigrationStatus`, `pid`, `socket`, and `logPath`. These should
match the Advanced panel. If they do not, your CLI is talking to a different
daemon/socket than the desktop app.

For dev runs started by `task dev`, use the same base path as the dev daemon:

```bash
ROUX_BASE_PATH="$HOME/.config/roux-dev" roux daemon status
```

### Check the on-disk database version

Default database path:

```text
~/.config/roux/board.db
```

Dev database path, when using the default `task dev` environment:

```text
~/.config/roux-dev/board.db
```

Check SQLite's schema version:

```bash
sqlite3 "$HOME/.config/roux/board.db" 'PRAGMA user_version;'
```

If this differs from the Advanced panel while the panel says **Storage:
board.db**, you are almost certainly inspecting a different file than the
daemon opened. Check `ROUX_BASE_PATH`, the daemon socket, and the `logPath`
from `roux daemon status`.

### Interpret failure states

- **Storage is `In-memory fallback`**: the daemon could not open or migrate
  the persisted `board.db`. The board is running on temporary in-memory state
  for this process. Read the **Migration error** row and the daemon log shown
  in Advanced before changing anything.
- **Pending is not `None`**: this should not happen after startup in the
  current implementation, because migrations run during `board.db` open. Treat
  it as a bug in migration/status plumbing unless you have changed the code
  locally.
- **CLI and UI disagree on target/current versions**: you are probably running
  mixed binaries. Compare the desktop build and the `roux` CLI on your `PATH`,
  then restart the daemon so it reopens `board.db` with the expected binary.

Do not delete `board.db` as a first debugging step. Copy it aside first if you
need to test recovery:

```bash
cp "$HOME/.config/roux/board.db" "$HOME/.config/roux/board.db.backup"
```

## Reporting a bug

Please file an issue at [github.com/phin-tech/roux/issues](https://github.com/phin-tech/roux/issues) with:

- the version shown in **Settings → About**
- a short description of what you expected vs. what happened
- any relevant log output from the Console or from `~/Library/Logs/roux/`
