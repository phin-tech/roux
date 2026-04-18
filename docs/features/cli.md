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

Inside Roux-managed panes, both `roux` and `roux-cli` are injected automatically, so you can call them without adding your own PATH shim.

## What it can do

- Open / focus a session for a directory and raise the app window (`roux app .`)
- Create, list, and poll sessions (`roux session create|list|poll`)
- Send keystrokes to a specific session's PTY (`roux session send`)
- List and create panes inside a session (`roux session panes list|create`)
- Split the active pane horizontally or vertically (`roux split`)
- Focus a pane by id (`roux focus`)
- Run a shell command in a new pane (`roux run`)
- Push notifications (`roux notify`)
- Emit hook status transitions (`roux hook`)
- Read, append, write, or search the multi-scoped notes vault (`roux notes <scope> <verb>` — experimental; see [Notes](notes.md))

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

Inside a Roux-managed PTY, `ROUX_*` variables are set (including `$ROUX_SESSION_ID` and `$ROUX_PANE_ID`), so most `-s` / `-p` flags are optional and scripting context is preserved.

See `roux --help` and `roux <subcommand> --help` for the authoritative list of commands and flags.
