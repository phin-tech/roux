# Daemon Socket Protocol

**Status:** Experimental v0. This document describes the protocol currently
implemented by `roux daemon`; command names and payloads can still change.

The daemon owns durable runtime state and process/PTy lifetimes. The desktop
app is one client: it renders UI, attaches output streams, and forwards user
input over the socket when a daemon is connected.

## Transport

Requests are JSON objects written to the Roux command socket.

- Unix/macOS: Unix socket at `~/.config/roux/roux.sock`.
- Windows: localhost TCP endpoint recorded in daemon status files; requests
  include `auth_token`.
- Normal commands return one JSON response and close the connection.
- `daemon-pty-attach` switches to a line-delimited JSON streaming response.

## Request Envelope

```json
{
  "command": "session-list",
  "session_id": "optional-session-id",
  "pane_id": "optional-pane-id",
  "auth_token": "windows-only-token",
  "args": {}
}
```

`args` uses camelCase for new fields. Some handlers also accept snake_case for
compatibility.

## Response Envelope

```json
{ "ok": true, "data": {} }
```

```json
{ "ok": false, "error": "message" }
```

## Discovery

`daemon-status` returns daemon metadata and a `capabilities` array. Clients
should prefer capability checks over hardcoded assumptions while this protocol
is experimental.

## Session Commands

`session-list`

Returns all daemon sessions, including archived sessions.

`session-poll`

Requires `session_id`. Returns one session.

`session-create-shell`

Creates a session record and primary PTY as one daemon-owned transaction.
Supported `args`:

- `id`: optional session id; generated when omitted.
- `repoPath`: repository path.
- `name`: display name.
- `worktreePath`: existing worktree path.
- `branch`: create/use a new worktree branch.
- `base`: optional branch start point.
- `fetchFirst`: run `git fetch origin` before resolving `base`.
- `profile`: spawn profile id.
- `initialSize`: `[cols, rows]`.
- `projectId`, `blueprintId`, `smolMachineName`.
- `notesEnv`: notes env snapshot for the primary PTY.

Returns the created session. If daemon PTY spawn fails after creating a new
worktree, the daemon attempts to remove the worktree before returning an error.

`session-reconnect-shell`

Requires `session_id`. Respawns the primary PTY using the existing session
record and returns the updated session. Supported `args`: `profile`,
`initialSize`, `notesEnv`.

`session-archive`

Requires `session_id`. Removes daemon PTYs for the session and soft-archives
the session record. Returns the archived session.

`session-restore`

Requires `session_id`. Restores the archived session and marks it
`Disconnected`. Returns the restored session.

`session-delete`

Requires `session_id`. Removes daemon PTYs for the session and permanently
removes the session record. Returns `{ "session_id": "..." }`.

`session-worktree-exists`

Requires `session_id`. Returns `{ "session_id": "...", "exists": true }`.

`session-refresh-branch`

Requires `session_id`. Reads the session worktree branch and updates the
daemon session record when it changed. Returns `{ "branch": "..." }`.

`session-rename`

Requires `session_id`, with `args.name`. Sets or clears `name_override`.

## Project Commands

`project-list`

Returns all daemon projects.

## Process Commands

`daemon-process-start`

`args.command` is required. `args.workingDir` is optional. Starts a headless
daemon-owned process and returns a process record.

`daemon-process-output`

Requires `args.id`. Optional `args.maxBytes`. Returns retained output and the
current process record.

`daemon-process-list`

Returns daemon process records.

`daemon-process-kill`

Requires `args.id`. Stops the process and returns its record.

## PTY Commands

`daemon-pty-spawn-shell`

Starts a daemon-owned shell PTY. Common `args`:

- `id`, `workingDir`, `sessionId`, `paneId`
- `projectId`, `worktreePath`, `notesEnv`
- `profile`, `initialSize`
- `role`: `sessionPrimary` or `secondary`

Returns a PTY record.

`daemon-pty-spawn-task`

Same as shell spawn, plus required `args.command`. Runs a one-shot PTY task.

`daemon-pty-output`

Requires `args.id`. Optional `args.maxBytes`. Returns a PTY snapshot with
retained text and raw output bytes.

`daemon-pty-list`

Returns all daemon PTY records.

`daemon-pty-write`

Requires `args.id` and `args.data`. Writes UTF-8 text into the PTY.

`daemon-pty-resize`

Requires `args.id`; accepts `args.cols` and `args.rows`. Returns the resized
PTY record.

`daemon-pty-detach`

Requires `args.id`. Marks the PTY detached and returns its record.

`daemon-pty-attach-pane`

Requires `args.id` and `args.paneId`. Marks the PTY attached to that pane and
returns its record.

`daemon-pty-mark-read`

Requires `args.id`. Clears unread/bell flags and returns its record.

`daemon-pty-set-name`

Requires `args.id` and `args.name`. `name: null` clears the name.

`daemon-pty-kill`

Requires `args.id`. Kills the PTY process and returns its record. The record
can remain available for output polling until removed by session lifecycle.

## PTY Attach Stream

`daemon-pty-attach` is the streaming command. It requires `args.id`; optional
`args.maxBytes` controls replay size.

After the request, the daemon writes newline-delimited frames:

```json
{ "type": "ready", "id": "pty-id", "record": {}, "replay_offset": 0, "replay_bytes": [] }
{ "type": "output", "offset": 10, "bytes": [] }
{ "type": "exit", "code": 0, "generation": 1 }
{ "type": "error", "error": "message" }
```

`bytes` and `replay_bytes` are JSON byte arrays. Desktop clients de-duplicate
frames by offset so replay and live output can overlap safely.

## Ownership Boundary

Daemon-owned:

- Session and project service state loaded from canonical config paths.
- Process and PTY lifetimes.
- Session create/reconnect/archive/restore/delete state transitions.
- Worktree creation for daemon-created sessions.

Desktop-owned:

- Rendering, pane layout, xterm instances, and UX-only state.
- Automation hooks, watches, and pane-state cleanup until those services move.
- Local fallback runtime when no daemon is connected.

