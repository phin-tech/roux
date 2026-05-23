# CLI bridge (`roux`)

Roux ships with a command-line tool, `roux`, that talks to the running app over a Unix socket. It lets you script Roux from the terminal — open sessions, split panes, send text, and focus panes.

## Installing

`roux` is bundled inside `Roux.app`. The easiest way to put it on your `PATH` is to symlink it:

```sh
ln -sf /Applications/Roux.app/Contents/MacOS/roux /usr/local/bin/roux
```

Then `roux --help` should work from any terminal.

Inside Roux-managed panes, both `roux` and the legacy `roux-cli` alias are injected automatically, so you can call either without adding your own PATH shim.

## Binary layout

The CLI is built from the standalone `crates/roux-cli` workspace crate. That crate produces the `roux` binary and does not link the Tauri desktop app. The desktop package lives separately under `src-tauri` as `roux-desktop` and bundles two sidecars:

- `roux` — the primary CLI binary
- `roux-cli` — compatibility alias for older hooks, scripts, and spawned panes

For source builds, use:

```sh
cargo build -p roux-cli --bin roux
```

For local development, `task cli:install` builds the same binary, installs `~/.local/bin/roux`, and points `~/.local/bin/roux-cli` at it on macOS/Linux.

## How it talks to Roux

Most `roux` commands are thin clients for the running desktop app:

- on macOS/Linux it talks to Roux over a Unix socket
- on Windows it talks to the app over a local TCP endpoint plus an auth token
- most commands print JSON payloads from the app, so they compose well with `jq`, shell scripts, and other agents

If the Roux app is not running, socket-backed commands fail with a direct `Roux is not running` error.

`roux daemon` is the main exception: it starts an experimental standalone runtime host. When the daemon owns the socket, top-level `roux run`, `roux split`, `roux shell`, `roux session create`, `roux session panes list|create`, `roux session send`, `roux session kill`, and `roux alias ...` are handled by daemon-owned runtime services instead of the desktop pane system. `roux attach` can connect a terminal directly to a daemon-owned PTY.

## Command groups

`roux` currently exposes these top-level commands:

- Open / focus a session for a directory and raise the app window (`roux app .`)
- Show or clear legacy hook status files (`roux status`, `roux clear`)
- Split the current pane (`roux split`)
- Create, list, and poll sessions (`roux session create|list|poll`)
- Send keystrokes to a specific session's PTY (`roux session send`)
- List and create panes inside a session (`roux session panes list|create`)
- Open a plain shell (`roux shell`)
- Focus a pane by id (`roux focus`)
- Run a shell command in a new pane (`roux run`)
- Attach this terminal to a daemon-owned PTY (`roux attach`)
- Run the Roux MCP stdio server (`roux mcp`)
- Start the experimental headless runtime host (`roux daemon`)
- Push notifications (`roux notify`)
- Emit hook status transitions and run automation hooks (`roux hook`)
- Read, append, write, or search the multi-scoped notes vault (`roux notes <scope> <verb>` — experimental; see [Notes](notes.md))

## Context-aware defaults

Inside a Roux-managed PTY, the app injects `ROUX_*` environment variables, including:

- `ROUX_SESSION_ID`
- `ROUX_PANE_ID`

That means many commands can omit explicit `--session` / `--pane` flags when run from inside an existing Roux pane.

Examples:

- `roux session send "continue"`
- `roux session panes list`
- `roux notes session show`

When you run the same commands outside Roux, pass explicit ids as needed.

## Command reference

### `roux app`

Open or focus a Roux session for a directory, then bring the app to the foreground.

```sh
roux app .
roux app ~/src/my-repo
```

If no path is given, it defaults to the current working directory.

### `roux split`

Split the current pane.

```sh
roux split
roux split --direction vertical
```

Defaults to `horizontal`.

When the daemon owns the socket, `roux split` creates a secondary daemon-owned
PTY and returns `{ "pane_id": "...", "pty_id": "..." }`. It does not mutate GUI
layout; `direction` is accepted for compatibility with GUI clients.

### `roux shell`

Open a plain shell pane in the current Roux session.

```sh
roux shell
roux shell --working-dir ~/src/my-repo
```

This is primarily useful from inside an existing Roux pane, where session context is already available.

When the daemon owns the socket, `roux shell` creates a secondary daemon-owned
PTY in the session and returns `{ "pane_id": "...", "pty_id": "..." }`. It
does not mutate GUI layout.

### `roux focus`

Focus a pane or a session by id.

```sh
roux focus --pane "$ROUX_PANE_ID"
roux focus --session "$OTHER_SESSION_ID"
```

### `roux run`

Run a shell command in a new command pane.

```sh
roux run "npm run test"
roux run "cargo test" --working-dir ~/src/my-repo
```

When Roux.app owns the command socket, this opens a GUI command pane. When `roux daemon` owns the socket, the same command starts a daemon-owned headless process; use `roux daemon output <id>` to poll retained output.

### `roux attach`

Attach the current terminal to a daemon-owned PTY.

```sh
roux attach daemon-pty-1
roux attach --session "$ROUX_SESSION_ID"
```

`roux attach` replays retained daemon output, streams live output, forwards stdin through `daemon-pty-write`, and resizes the PTY to the current terminal size on attach. When a PTY id is omitted, `--session` or `$ROUX_SESSION_ID` resolves to that session's primary daemon PTY.

### `roux mcp`

Run Roux's MCP server over stdio.

```sh
roux mcp
```

Most people do not run this command by hand. Enable MCP in **Settings → Agent Integrations**, then use the host setup button for a supported MCP client. The host launches `roux mcp` when it needs the server.

The MCP server is a thin adapter over the same socket bridge as the CLI:

- the Roux desktop app must be running
- **Enable Roux MCP** must be on in Settings
- the host config uses the installed/current `roux` path
- session- and pane-targeted tools require explicit ids where mutation is possible, especially `roux_send_text`

The v1 MCP server exposes inspection and safe action tools:

- `roux_list_sessions`
- `roux_get_session`
- `roux_list_panes`
- `roux_create_session`
- `roux_create_pane`
- `roux_send_text`
- `roux_get_latest_output`
- `roux_focus`
- `roux_read_notes`
- `roux_search_notes`
- `roux_append_notes`
- `roux_notes_vault_root`

The v1 server intentionally does not expose arbitrary shell execution, PTY kill, worktree removal, permanent session deletion, or broad filesystem mutation.

`roux_get_latest_output` returns the exact PTY replay bytes as `replay_bytes_base64`. It also includes `text` when the replay bytes are valid UTF-8; clients that need byte-for-byte fidelity should decode `replay_bytes_base64`. When the daemon owns the socket, the tool reads retained daemon PTY replay instead of GUI-owned terminal state.

### `roux daemon`

Start Roux's experimental headless runtime host.

```sh
roux daemon
```

This is a foundation for moving long-lived runtime services out of the Tauri process. Today it loads persisted projects and sessions, starts the shared runtime service host, binds the Roux command socket, and runs until Ctrl-C.

The daemon exposes daemon-only CLI commands:

```sh
roux daemon status
```

When `roux daemon` owns the socket, `roux daemon status` returns the daemon PID, uptime, socket path, log path, loaded session/project/process counts, and daemon capabilities. The daemon also answers headless session, project, PTY, process, and worktree commands over the socket.

The implemented socket protocol is documented in [`../v2/daemon-protocol.md`](../v2/daemon-protocol.md).

Daemon runtime logs are written to `~/.config/roux/logs/roux-daemon.log` and mirrored to stderr. Existing daemon logs rotate to `roux-daemon.1.log` through `roux-daemon.5.log` on daemon startup.

The daemon also owns a headless process registry. This is intentionally separate from GUI panes for now:

```sh
roux daemon run "printf hello-from-daemon"
roux daemon output daemon-process-1
roux daemon processes
roux daemon kill daemon-process-1
```

`roux daemon run` starts the command inside the daemon process, retains stdout/stderr output in the daemon, and returns a daemon process id. `roux daemon output` polls the retained output and current exit status.

If Roux.app already owns the command socket, `roux daemon` refuses to start instead of replacing the live GUI socket.

If `roux daemon` is already running when Roux.app starts, the desktop app detects it, skips its own socket server, and routes daemon-backed sessions, PTYs, project/session metadata, process commands, core worktree filesystem operations, durable alias state, durable notes vault commands, durable watch state, and watch execution through the daemon. Worktree create/remove automation hooks run on the daemon host for daemon-owned worktree operations. `roux run`, `roux split`, `roux shell`, `roux session create`, `roux session panes list|create`, `roux session send`, `roux session kill`, `roux alias ...`, MCP latest-output reads, and `roux notes ...` also work against the daemon socket owner. Watch notification presentation and manual hook-management UX still run in the desktop process for now.

Current limits:

- the desktop app still owns pane layout, xterm.js rendering, and GUI PTY attachment
- pane layout commands such as `roux focus` still expect Roux.app to be running
- daemon `roux split`, `roux shell`, and `roux session panes create` create daemon PTYs, not visible GUI splits
- `roux attach` is an initial single-terminal daemon client; resize-on-SIGWINCH and richer reconnect UX remain future work

Use it for daemon development and validation, not as a replacement for launching Roux.app.

### `roux session`

Session lifecycle and introspection commands.

#### `roux session create`

Create a new session.

```sh
roux session create
roux session create --name "review"
roux session create --working-dir ~/src/my-repo
roux session create --worktree-branch feat/review
roux session create --profile codex
```

Useful flags:

- `--name` — session name
- `--working-dir` — existing repo/worktree path; defaults to the current directory unless you are already inside a Roux session
- `--worktree-branch` — create a new worktree session from that branch name
- `--profile` / `-P` — spawn profile id, defaulting to `claude`
- `--flag` / `-f` — repeatable extra flags passed to the agent profile
- `--nono-profile` and `--nono-allow-dir` — sandbox controls

When the daemon owns the socket, `roux session create` creates a daemon-owned session and primary PTY. `--prompt`, `--flag`, `--nono-profile`, and `--nono-allow-dir` are currently rejected by daemon session creation instead of being silently ignored.

#### `roux session send`

Send text to a session or pane PTY.

```sh
roux session send "continue"
roux session send "review this diff" --session "$SID"
roux session send $'\x03' --session "$SID" --no-enter
```

By default, Roux appends Enter. Use `--no-enter` for raw bytes or partial input.

When the daemon owns the socket, `roux session send` writes to the session's primary daemon PTY by default, or to the attached daemon PTY matching `$ROUX_PANE_ID` / `--pane`.

#### `roux session poll`

Get the current state of one session as JSON.

```sh
roux session poll --session "$SID"
```

#### `roux session list`

List every session as JSON.

```sh
roux session list
```

#### `roux session panes list`

List panes for a session as JSON.

```sh
roux session panes list --session "$SID"
```

When the daemon owns the socket, this lists daemon-owned PTYs for the session
using the same snapshot shape. `layout` is `null` because the daemon does not
own GUI pane layout.

#### `roux session panes create`

Create a new pane inside a session.

```sh
roux session panes create --session "$SID"
roux session panes create --session "$SID" --profile plain-shell
roux session panes create --session "$SID" --profile codex --direction vertical
roux session panes create --session "$SID" --working-dir ~/src/other-repo
```

Defaults:

- profile: `plain-shell`
- direction: `horizontal`
- working directory: the session worktree path

When the daemon owns the socket, this creates a secondary daemon-owned PTY and
returns `{ "pane_id": "...", "pty_id": "..." }`. It does not mutate GUI layout;
attach with `roux attach <pty_id>` or let another frontend render it.

### `roux notify`

Push a notification into Roux's in-app notification service.

```sh
roux notify --title "Build finished" --body "All checks passed" --level success
roux notify --title "Needs attention" --level attention --session "$SID"
```

You can also send a full JSON payload via stdin:

```sh
cat payload.json | roux notify --json
```

Session resolution order is:

1. explicit `--session`
2. `sessionId` already present in the JSON payload
3. `ROUX_SESSION_ID`
4. global notification with no session binding

### `roux hook`

Handle Claude Code status hooks and Roux automation hooks.

Claude status events are installed by Roux's setup flow:

```sh
roux hook working
roux hook idle
roux hook attention
roux hook error
roux hook disconnected
```

It reads a JSON payload from stdin and writes provider/session status state for Roux to consume.

Automation hook commands talk to the running app over the socket:

```sh
roux hook show
roux hook show --repo-path ~/src/my-repo
roux hook run post-watch-success --repo-path ~/src/my-repo
roux hook run post-worktree-create --repo-path ~/src/my-repo --branch feat/x --provider worktrunk
```

See [Automation hooks](hooks.md) for config files, event names, conditions, templates, logs, and Worktrunk differences.

### `roux status` and `roux clear`

Legacy status-file helpers for hook debugging:

```sh
roux status
roux clear
```

`status` prints the known hook status files, and `clear` removes them.

### `roux notes`

The notes commands are documented in more detail on [Notes](notes.md), but the CLI surface is:

```sh
roux notes root
roux notes session show
roux notes repo append "remember to update the fixture"
roux notes project write --topic rollout-plan --content "..."
roux notes search --tag api --scope repo
```

When the daemon owns the socket, notes commands read and write the daemon
host's configured notes vault. The Tauri app remains responsible for
presenting live notes-change UI events to its own windows.

Supported scopes:

- `global`
- `project`
- `repo`
- `session`

Supported verbs per scope:

- `show`
- `append`
- `write`
- `path`

Global commands:

- `root`
- `search`

## Scripting & agent-to-agent examples

Open (or focus) Roux at the current directory:

```sh
roux app .
```

Create a new session with a fresh worktree:

```sh
roux session create --name "review" --worktree-branch feat/review
```

Poll a session's status from another agent:

```sh
status=$(roux session poll -s "$OTHER_SESSION_ID" | jq -r .status)
if [ "$status" = "idle" ]; then
    roux session send "review this PR" -s "$OTHER_SESSION_ID"
fi
```

Log a timestamped entry to this session's notes from an agent:

```sh
echo "retried after clearing token cache, still 401" \
    | roux notes session append --timestamp --tag api --tag tls
```

Find every note in the current repo tagged `#api` (prefix-matches
`#api/tls`, `#api/grpc`, etc.):

```sh
roux notes search --tag api --scope repo
```

Send raw bytes without appending Enter:

```sh
roux session send $'\x03' -s "$SID" --no-enter   # ctrl-c
```

List panes in a session (live snapshot from the UI):

```sh
roux session panes list -s "$SID"
```

Open a new shell pane alongside the main agent:

```sh
roux session panes create -s "$SID" --profile plain-shell --direction vertical
```

Create a new Codex session from inside an existing Roux pane without repeating the working directory:

```sh
roux session create --name "codex pass" --profile codex
```

Push a scoped in-app notification from a script:

```sh
roux notify \
  --title "Review finished" \
  --subtitle "docs PR" \
  --body "Ready for another pass" \
  --level success \
  --session "$SID"
```

Inside a Roux-managed PTY, `ROUX_*` variables are set (including `$ROUX_SESSION_ID` and `$ROUX_PANE_ID`), so most `-s` / `-p` flags are optional and scripting context is preserved.

See `roux --help` and `roux <subcommand> --help` for the authoritative list of commands and flags.
