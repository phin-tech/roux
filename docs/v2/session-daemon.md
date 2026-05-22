# V2: Session Daemon (Persistent PTY Sessions)

**Status:** Design note — foundation started, full PTY daemon not yet implemented
**Goal:** Sessions survive app crashes/restarts with full terminal scrollback preserved

## Current state

Roux now has the first daemon-shaped entrypoint:

```sh
roux daemon
```

It lives in the standalone `crates/roux-cli` crate, starts the shared runtime service host, loads persisted projects/sessions from the canonical config paths, and runs until Ctrl-C. This deliberately keeps the command-line binary separate from the Tauri desktop package.

The daemon now binds the Roux command socket itself and exposes a daemon-only status endpoint:

```sh
roux daemon status
```

That endpoint is intentionally not implemented by the GUI socket server. It proves the daemon can own an observable capability independently of Roux.app. The daemon also serves the first headless durable-state commands over the same socket: session list, session poll, session rename, and project list.

The daemon writes its own runtime log to `~/.config/roux/logs/roux-daemon.log`, rotates existing daemon logs on startup, and reports the active log path from `roux daemon status`.

The daemon now owns a small headless process registry too. `roux daemon run "<command>"` starts a shell command inside the daemon process, `roux daemon output <id>` polls retained stdout/stderr and exit status, `roux daemon processes` lists daemon-owned processes, and `roux daemon kill <id>` stops one. This is the first runtime behavior owned by the daemon that the GUI does not render or spawn.

If Roux.app already owns the socket, `roux daemon` refuses to start instead of unlinking or replacing the GUI's live command channel.

If the daemon is already running when Roux.app starts, the desktop detects it, skips its own socket server, and uses the daemon for session/project metadata reads plus session rename. That makes the GUI a frontend for daemon-owned durable state. Interactive PTY ownership still remains in the desktop process until the next migration slice.

That is not the full design below yet. As of this note:

- Roux.app still owns interactive PTYs and xterm.js rendering
- pane socket commands such as split, send, and attach still expect the desktop app to be running
- top-level `roux run` still creates a GUI pane; daemon-owned processes use `roux daemon run`
- there is no `roux attach` command yet
- the daemon does not yet own scrollback replay or reconnectable PTY lifetimes

## Problem

Currently, when Roux closes:
- PTY processes are killed
- Terminal scrollback is lost
- Sessions are restored as "disconnected" with metadata only
- Users rely on Claude Code's `/resume` to continue conversations

This covers ~90% of the use case, but power users running long sessions want true persistence.

## Architecture: Thin Daemon + Unix Socket

A minimal `roux daemon` process that owns PTY sessions and relays bytes to connected clients (the Tauri GUI or a CLI attach command).

```text
roux daemon                  ← background process, owns all PTYs
  ├── PTY: claude (session 1)
  ├── PTY: claude (session 2)
  └── PTY: /bin/zsh (shell split)
       │
       └── Unix socket: ~/.config/roux/roux.sock
            │
            ├── Roux.app (Tauri GUI) — connects as a client
            └── roux attach <id> — CLI attach from any terminal
```

### Daemon responsibilities
- Spawn and own PTY processes
- Buffer scrollback (configurable, e.g., last 10k lines per session)
- Accept client connections via Unix socket
- Relay PTY output to connected clients
- Accept input from clients and write to PTY
- Handle resize requests
- Stay alive when all clients disconnect
- Auto-start on first `roux` or Roux.app launch
- Clean up dead sessions

### Client responsibilities (Tauri GUI)
- Connect to daemon socket on startup
- Receive PTY output and feed to xterm.js
- Send keystrokes to daemon
- Request session creation/destruction
- Handle reconnection if daemon restarts

### CLI client (`roux attach`)
- Connect to daemon socket
- Run a local terminal emulator (raw mode)
- Render PTY output directly to stdout
- Forward stdin to daemon
- Needs a terminal emulator or raw byte passthrough

### Protocol (simple, line-delimited JSON)

```jsonl
→ {"type":"create","session_id":"abc","command":"claude","cwd":"/path"}
← {"type":"created","session_id":"abc"}
→ {"type":"attach","session_id":"abc"}
← {"type":"output","session_id":"abc","data":"base64..."}
← {"type":"output","session_id":"abc","data":"base64..."}
→ {"type":"input","session_id":"abc","data":"base64..."}
→ {"type":"resize","session_id":"abc","cols":120,"rows":40}
→ {"type":"detach","session_id":"abc"}
→ {"type":"kill","session_id":"abc"}
← {"type":"exited","session_id":"abc","code":0}
```

## Estimated scope

~500-800 lines of Rust for the daemon:
- `crates/roux-cli/src/daemon.rs` — daemon entrypoint and process lifetime
- `crates/roux-runtime` — shared session/project/PTY runtime services used by both daemon and desktop
- socket server/client handling for daemon-owned PTYs
- `roux daemon` subcommand — start the daemon (initial version exists)
- `roux attach <id>` — attach from CLI

~200 lines of frontend changes:
- Replace direct PTY IPC with socket client
- Handle reconnection on daemon restart

## Alternatives Considered

### Embed Zellij

Zellij (Rust terminal multiplexer) could act as the session layer.

**Option A: Zellij as library** — `zellij-server` and `zellij-client` crates are tightly coupled and assume they own rendering. Not designed for embedding.

**Option B: Zellij as subprocess** — spawn `zellij` inside our PTY. Gets persistence for free but adds Zellij's chrome inside our terminal, double PTY overhead, escape sequence conflicts, and we lose control granularity.

**Option C: Bare Zellij layout** — `zellij --session roux-{id} --layout bare -- claude`. Minimal Zellij UI but still adds: dependency on Zellij installation, version coupling, double PTY, startup overhead (~50-100ms), and potential ANSI escape sequence conflicts.

**Verdict:** Own daemon is simpler, more maintainable, and gives full control. Zellij solves a broader problem than we need.

### tmux integration

Same tradeoffs as Zellij but in C. Less embeddable, more mature. `tmux new-session -d -s roux-{id} claude` works but has the same double-PTY and control issues.

## Prerequisites

- Stable V1 with hooks-based status detection
- Clear session lifecycle management
- standalone `roux` binary already in place (`daemon` exists; `attach` remains future work)

## Migration path

1. Build daemon with socket server
2. Move PTY spawning from Tauri process to daemon
3. Tauri becomes a socket client
4. Add `roux attach` for CLI access
5. Add `roux daemon --start` to launchd/systemd for auto-start
6. Scrollback buffer in daemon for replay on reconnect
