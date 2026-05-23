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

## Worktree Commands

`worktree-list`

Requires `args.repoPath`. Lists worktrees for that repository on the daemon
host, enriched with worktrunk metadata when `wt` is available to the daemon.
Returns an array of worktree records.

`worktree-create`

Requires `args.repoPath` and `args.branch`. Creates or reuses a worktree for
the branch on the daemon host. Optional `args.startPoint` selects the branch
start point, `args.fetchFirst` runs `git fetch origin` before creation, and
`args.basePath` overrides the daemon setting for the worktree base directory.
The daemon runs `pre-worktree-create` before the filesystem mutation and
`post-worktree-create` after success, using hook config visible on the daemon
host. Returns `{ "path": "..." }`.

`worktree-remove`

Requires `args.repoPath` and `args.worktreePath`. Optional `args.alsoBranch`
also deletes the checked-out branch after removing the worktree. Optional
`args.force` passes the force-delete intent through the selected provider.
The daemon runs `pre-worktree-remove` before removal and
`post-worktree-remove` after success, using the repository path as the
post-remove cwd because the worktree path may no longer exist. Returns
`{ "repoPath": "...", "worktreePath": "..." }`.

`worktree-list-branches`

Requires `args.repoPath`. Lists local branches for that repository on the
daemon host.

`git-init`

Requires `args.path`. Runs `git init` in that directory on the daemon host.

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

## Watch Commands

The daemon owns durable watch definitions and state. A GUI client may still
execute checks locally and sync the resulting `Watch` snapshot back through
`watch-replace`; desktop notifications remain client-routed UX. The daemon
does not call Tauri notification APIs.

`watch-list`

Returns all daemon watch records.

`watch-create`

Requires `args.config`, matching `CreateWatchConfig`. Creates an active watch
record in daemon state and returns it.

`watch-find-or-create`

Requires `args.config`. For `GithubPr` watches, atomically returns an existing
watch matching `(scope, repo, prNumber)` or inserts a new active watch. Other
watch kinds create a new active watch.

`watch-remove`

Requires `args.id`. Removes the watch and returns `{ "id": "..." }`.

`watch-pause`

Requires `args.id`. Sets `runtimeState` to `Paused` and returns the watch.

`watch-resume`

Requires `args.id`. Sets `runtimeState` to `Active` and returns the watch.

`watch-replace`

Requires `args.watch`. Replaces or inserts the full watch record. This is used
by clients that execute watch checks while the daemon owns durable state.

`watch-remove-for-session`

Requires `args.sessionId`. Removes all session-scoped watches for that session.

`watch-cleanup-orphans`

Removes session/project scoped watches whose owning session/project is no
longer present in the daemon runtime host.

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
- Worktree create/list/remove, branch listing, and git init filesystem
  operations.
- Worktree create/remove automation hooks for daemon-owned worktree commands.
- Durable watch definitions and runtime state.

Desktop-owned:

- Rendering, pane layout, xterm instances, and UX-only state.
- Watch check execution and notification presentation while daemon watch events
  are still being split into a client event stream.
- Pane-state cleanup and manual hook preview/run/list UX until those services
  move.
- Local fallback runtime when no daemon is connected.
