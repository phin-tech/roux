# Troubleshooting

Common issues and how to work around them.

!!! note "Stub page"
    This page is a placeholder. Real troubleshooting entries will land here as they come up in practice.

## Claude Code isn't found

Roux shells out to the `claude` command. If new sessions fail to start, verify Claude Code is installed and on your `PATH` by running `claude --version` in a regular terminal.

## A pane shows nothing after restart

Shell panes are respawned automatically on launch, but Claude sessions are not restarted by default. Open the command palette (++cmd+k++) and start a new session in that pane.

## Reporting a bug

Please file an issue at [github.com/phin-tech/roux/issues](https://github.com/phin-tech/roux/issues) with:

- the version shown in **Settings → About**
- a short description of what you expected vs. what happened
- any relevant log output from the Console or from `~/Library/Logs/roux/`
