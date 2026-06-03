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

## Reporting a bug

Please file an issue at [github.com/phin-tech/roux/issues](https://github.com/phin-tech/roux/issues) with:

- the version shown in **Settings → About**
- a short description of what you expected vs. what happened
- any relevant log output from the Console or from `~/Library/Logs/roux/`
