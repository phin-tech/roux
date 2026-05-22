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

## Smol Machines panel doesn't appear in the activity rail

Roux gates the integration on a working `smolvm` binary. Confirm `smolvm --version` runs from your shell, then either restart Roux or refresh the **Smol Machines** detection in **Settings → Integrations**. If `smolvm` is installed at a non-standard path, set the override under **Settings → Integrations → Smol Machines**.

## Pane spawns into a guest shell but `claude` says "not found"

Fresh smolvm guests don't ship with Claude or Codex preinstalled. When a session bound to a VM tries to start `claude`, Roux writes a multi-line `# claude is not installed…` comment block instead of letting the shell error fire. Click **Install Claude (Run in VM)** on the machine row in the Smol Machines panel. To make it survive recreations, use **Install Claude (Persist via Smolfile)** — that line gets baked into the Smolfile's `[dev].init`.

## Session lands in `$HOME` instead of the worktree

By default smolvm doesn't expose host paths inside the guest, so `--workdir <host_worktree>` would fail. Roux only passes `--workdir` when the worktree path is mounted in the linked Smolfile.

When you bind a session to a machine that doesn't mount the worktree, Roux surfaces a yellow banner offering to add a same-path mount. Click **Add mount**, then recreate the machine — smolvm bakes volumes at create time, so a hot-add doesn't take effect.

## Managed HTTP proxy fails to start

The Smol Machines panel's Shield toggle reports failures inline. Common causes:

- **Port already in use** — another proxy or service is bound there. Pick a different port or stop the conflicting process.
- **Command not on `$PATH`** — Roux runs the command via `sh -lc`, so login shell PATH applies. Confirm `which tinyproxy` (or your chosen proxy) returns a path in your normal shell.
- **Config file missing** — many proxies refuse to start without a config file. Test the command manually in a terminal first; once it works there, paste it into **Settings → Integrations → Smol Machines → Managed HTTP proxy**.
- **Slow startup** — Roux waits 5 seconds for the proxy to bind its listen socket. If your proxy needs longer (cold-loading large allowlists, etc.), start it externally and use the Host HTTP proxy URL field on the create form directly.

## Private registry inside VM fails with auth/IP error

Two different problems often look the same:

- **IP-allowlisted registry** (e.g. AWS CodeArtifact behind a corp VPN) — the VM has a different egress IP. Solution: configure a managed proxy in **Settings → Integrations → Smol Machines** and set the create form's **Host HTTP proxy URL** to its address. Roux writes a `/etc/profile.d/roux-proxy.sh` into the guest so `npm`, `pip`, `curl`, `git`, etc. all pick up `HTTP_PROXY`.
- **Token-based registry with stale credentials** — your host's `~/.aws/sso` cache or `~/.npmrc` token expired. Refresh on the host first; future curated config-mount support (deferred) will cover the cross-cutting "tokens get pulled into the VM" workflow. For now, mount the relevant config file as `host:guest:ro` in the create form's mount paths.

## Smol machine deleted out from under a bound session

If a session is bound to a machine name that no longer exists (deleted via CLI or another tool), the next pane spawn for that session fails with a clear error rather than silently dropping back to host. Recover by either recreating the machine with the same name (`smolvm machine create <name> -s <smolfile>`) or unbinding the session.

## Reporting a bug

Please file an issue at [github.com/phin-tech/roux/issues](https://github.com/phin-tech/roux/issues) with:

- the version shown in **Settings → About**
- a short description of what you expected vs. what happened
- any relevant log output from the Console or from `~/Library/Logs/roux/`
