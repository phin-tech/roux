# CLI bridge (`roux-cli`)

Roux ships with a command-line tool, `roux-cli`, that talks to the running app over a Unix socket. It lets you script Roux from the terminal — open sessions, split panes, send text, and focus panes.

!!! note "Stub page"
    The full command reference is still being written. This page covers the basics.

## Installing

`roux-cli` is bundled inside `Roux.app`. The easiest way to put it on your `PATH` is to symlink it:

```sh
ln -sf /Applications/Roux.app/Contents/MacOS/roux-cli /usr/local/bin/roux
```

Then `roux --help` should work from any terminal.

## What it can do

- Open / focus a session for a directory and raise the app window (`roux app .`)
- Create, list, and poll sessions (`roux session create|list|poll`)
- Send keystrokes to a specific session's PTY (`roux session send`)
- List and create panes inside a session (`roux session panes list|create`)
- Split the active pane horizontally or vertically (`roux split`)
- Focus a pane by id (`roux focus`)
- Run a shell command in a new pane (`roux run`)
- Push notifications (`roux notify`)

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

Inside a Roux-managed PTY, `$ROUX_SESSION_ID` and `$ROUX_PANE_ID` are set, so most `-s` / `-p` flags are optional.

See `roux --help` and `roux <subcommand> --help` for the authoritative list of commands and flags.
