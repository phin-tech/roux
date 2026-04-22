# CLI bridge (`roux-cli`)

Roux ships with a command-line tool, `roux-cli`, that talks to the running app over a Unix socket. It lets you script Roux from the terminal — open sessions, split panes, send text, and focus panes.

## Installing

`roux-cli` is bundled inside `Roux.app`. The easiest way to put it on your `PATH` is to symlink it:

```sh
ln -sf /Applications/Roux.app/Contents/MacOS/roux-cli /usr/local/bin/roux
```

Then `roux --help` should work from any terminal.

Inside Roux-managed panes, both `roux` and `roux-cli` are injected automatically, so you can call them without adding your own PATH shim.

## How it talks to Roux

`roux-cli` is a thin client for the running desktop app:

- on macOS/Linux it talks to Roux over a Unix socket
- on Windows it talks to the app over a local TCP endpoint plus an auth token
- most commands print JSON payloads from the app, so they compose well with `jq`, shell scripts, and other agents

If the Roux app is not running, socket-backed commands fail with a direct `Roux is not running` error.

## Command groups

`roux-cli` currently exposes these top-level commands:

- Open / focus a session for a directory and raise the app window (`roux app .`)
- Show or clear legacy hook status files (`roux status`, `roux clear`)
- Split the current pane (`roux split`)
- Create, list, and poll sessions (`roux session create|list|poll`)
- Send keystrokes to a specific session's PTY (`roux session send`)
- List and create panes inside a session (`roux session panes list|create`)
- Open a plain shell (`roux shell`)
- Focus a pane by id (`roux focus`)
- Run a shell command in a new pane (`roux run`)
- Push notifications (`roux notify`)
- Emit hook status transitions (`roux hook`)
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

### `roux shell`

Open a plain shell pane in the current Roux session.

```sh
roux shell
roux shell --working-dir ~/src/my-repo
```

This is primarily useful from inside an existing Roux pane, where session context is already available.

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

#### `roux session send`

Send text to a session or pane PTY.

```sh
roux session send "continue"
roux session send "review this diff" --session "$SID"
roux session send $'\x03' --session "$SID" --no-enter
```

By default, Roux appends Enter. Use `--no-enter` for raw bytes or partial input.

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

Handle a Claude Code hook event. This is intended for automation/hooks, not normal interactive use.

```sh
roux hook working
roux hook idle
roux hook attention
roux hook error
roux hook disconnected
```

It reads a JSON payload from stdin and writes provider/session status state for Roux to consume.

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
