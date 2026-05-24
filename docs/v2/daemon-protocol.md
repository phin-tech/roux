# Daemon Socket Protocol

**Status:** Experimental v0. This document describes the protocol currently
implemented by `roux daemon`; command names and payloads can still change.

The daemon owns durable runtime state and process/PTy lifetimes. The desktop
app is one client: it renders UI, attaches output streams, and forwards user
input over the socket when a daemon is connected.

## Transport

Requests are JSON objects written to the Roux command endpoint.

- Unix/macOS default: Unix socket at `~/.config/roux/roux.sock`.
- TCP: start the daemon with `ROUX_DAEMON_BIND=tcp://HOST:PORT`. Use port
  `0` to let the OS choose a local port; `daemon-status.socket` reports the
  actual `tcp://HOST:PORT` endpoint.
- Clients override discovery with `ROUX_SOCKET=tcp://HOST:PORT` or
  `ROUX_SOCKET=unix:///path/to/roux.sock`. Plain `ROUX_SOCKET` values remain
  platform-native: Unix paths on Unix/macOS, TCP addresses on Windows.
- TCP requests include `auth_token`. Clients load it from `ROUX_DAEMON_TOKEN`,
  `ROUX_AUTH_TOKEN`, or the local token file written by a locally-started TCP
  daemon. On Unix/macOS, explicit TCP daemon binds require
  `ROUX_DAEMON_TOKEN`; Windows local TCP binds generate and write a token when
  one is not supplied.
- Normal commands return one JSON response and close the connection.
- `daemon-pty-attach` switches to a line-delimited JSON streaming response.

## Request Envelope

```json
{
  "command": "session-list",
  "session_id": "optional-session-id",
  "pane_id": "optional-pane-id",
  "auth_token": "required-for-tcp",
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

`daemon-stop` requests graceful daemon shutdown and returns
`{ "stopping": true, "pid": ..., "socket": "...", "logPath": "..." }` before
the daemon closes its socket and runtime services. It is used by
`roux daemon stop` and `roux daemon restart`.

## Session Commands

`session-list`

Returns all daemon sessions, including archived sessions.

`session-poll`

Requires `session_id`. Returns one session.

`session-events` (streaming)

Opens a persistent stream that broadcasts `SessionStatus` changes for all
daemon-owned sessions. The daemon watches `~/.config/roux/status/*.json` for
hook status files written by `roux hook <status>` and routes changes to the
session service; only files that include a `roux_session_id` field are routed.

Frame shapes (newline-delimited JSON, `"type"` tag):
- `{ "type": "ready" }` — sent once when the stream is open and the subscriber
  is registered.
- `{ "type": "event", "event": { "sessionId": "...", "status": "..." } }` —
  emitted on every status change (compare-before-assign — no-ops are dropped).
- `{ "type": "warning", "message": "..." }` — emitted when the broadcast
  buffer overflows and events are dropped.

`status` values mirror `SessionStatus`: `idle`, `generating`, `attention`,
`error`, `disconnected`.

`session-create`

Compatibility alias for top-level `roux session create` when the daemon owns
the socket. Accepts the existing CLI arg names such as `working_dir`,
`worktree_branch`, and `start_point`, normalizes them into
`session-create-shell`, and returns `{ "session_id": "..." }`. Daemon session
creation currently rejects `prompt` and `flags`.

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
- `nonoProfile`, `nonoAllowDirs`.
- `notesEnv`: notes env snapshot for the primary PTY.

Returns the created session. If daemon PTY spawn fails after creating a new
worktree, the daemon attempts to remove the worktree before returning an error.

`session-reconnect-shell`

Requires `session_id`. Respawns the primary PTY using the existing session
record and returns the updated session. Supported `args`: `profile`,
`nonoProfile`, `nonoAllowDirs`, `initialSize`, `notesEnv`.

`session-archive`

Requires `session_id`. Removes daemon PTYs for the session and soft-archives
the session record. Returns the archived session.

`session-kill`

Compatibility alias for the CLI's existing `roux session kill` command. Same
behavior as `session-archive`.

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

`session-set-project`

Requires `session_id`, with nullable `args.projectId`. Sets or clears the
session's project id in daemon-owned session metadata.

`session-set-pinned-pr-url`

Requires `session_id`, with nullable `args.url`. Sets or clears the session's
pinned PR URL in daemon-owned session metadata.

`session-set-smol-machine`

Requires `session_id`, with nullable `args.machineName`. Sets or clears the
session's smol-machine binding in daemon-owned session metadata. Future PTY
spawns for that session inherit the binding from the daemon session record.

`session-panes-list`

Requires `session_id`. Returns the same snapshot shape as the desktop socket
handler:

```json
{
  "sessionId": "session-id",
  "layout": null,
  "descriptors": [
    {
      "id": "pane-id",
      "type": "shell",
      "ptyId": "pty-id",
      "workingDir": "/repo",
      "profileId": "plain-shell"
    }
  ]
}
```

The daemon currently returns `layout: null` because GUI pane layout is a client
responsibility. `descriptors` is derived from daemon-owned PTY metadata.

`session-panes-create`

Compatibility command for `roux session panes create` and MCP clients when the
daemon owns the socket. Requires `session_id`. Supported `args`:

- `profile`: spawn profile id; defaults to `plain-shell`.
- `direction`: accepted for compatibility; must be `horizontal` or `vertical`.
- `workingDir`: override working directory; defaults to the session worktree.
- `initialSize`: `[cols, rows]`.

Creates a secondary daemon-owned PTY and returns
`{ "pane_id": "...", "pty_id": "..." }`. It does not mutate GUI layout; a
client can render it or attach with `roux attach <pty_id>`.

`shell`

Compatibility alias for top-level `roux shell` when the daemon owns the socket.
Requires `session_id`. Accepts optional `args.workingDir` / `args.working_dir`
and otherwise behaves like `session-panes-create` with `profile:
plain-shell`. Returns `{ "pane_id": "...", "pty_id": "..." }`.

`split`

Compatibility alias for top-level `roux split` when the daemon owns the socket.
Requires `session_id`. Accepts `args.direction` for compatibility and otherwise
behaves like `session-panes-create` with `profile: plain-shell`. Returns
`{ "pane_id": "...", "pty_id": "..." }`. The daemon does not persist or render
split layout.

## Alias Commands

The daemon handles the same alias socket commands as the desktop socket when it
owns the socket. Alias state is loaded from and persisted to
`aliases.json` in the daemon host's Roux config directory.

- `alias-set`
- `alias-unset`
- `alias-claim`
- `alias-list`
- `alias-get`
- `alias-whoami`
- `alias-add-member`
- `alias-remove-member`
- `alias-mode`
- `alias-events`

The command payloads match the CLI/MCP adapter fields. `alias-set` and
`alias-claim` bind aliases to the calling or explicit session/pane. `alias-get`
keeps bare-name ambiguity behavior for aliases that exist in multiple project
scopes. Group membership and consumption mode commands mutate the same durable
alias records.

`alias-events`

Streaming command. Sends live alias mutation frames from daemon-owned alias
state. Desktop clients forward these into the existing `alias-event` Tauri
channel so the frontend alias mirror stays live when aliases change from CLI,
MCP, mailbox materialization, or another GUI client.

Frames:

```json
{ "type": "ready" }
{ "type": "event", "event": { "kind": "set", "alias": {} } }
{ "type": "event", "event": { "kind": "unset", "canonical": "reviewer", "projectId": null } }
{ "type": "warning", "message": "dropped 2 buffered alias event(s)" }
{ "type": "error", "error": "message" }
```

## Mailbox And Bus Commands

The daemon owns the durable mailbox event log, per-recipient read state, and
bus subscription records when it owns the socket. Desktop and CLI clients are
frontends over the same state.

`mailbox-post`

Requires `args.body` and at least one of `args.to` or `args.topic`. Optional
fields: `from`, `kind`, `subject`, `project_id`, `correlation_id`, and
`structured`. Returns the created event. Posting to an alias ensures the alias
record exists in daemon-owned alias state.

`mailbox-peek`

Requires `args.alias` unless the request has enough pane/session context to
resolve one alias. Optional `args.unread`, `args.project_id`, `args.global`,
and `args.limit`. Returns matching recipient events without marking them read.

`mailbox-read`

Same target fields as `mailbox-peek`, but returns unread events and marks them
read. Optional `args.ack` also acks each returned event.

`mailbox-get`

Requires `args.event_id`. Returns the event or `null`.

`mailbox-read-state`

Requires `args.event_id` and `args.recipient`. Returns the recipient read state
or `null`.

`mailbox-mark-read`

Requires `args.event_id` and `args.recipient`. Returns `{ "changed": true }`
when state changed.

`mailbox-ack`

Requires `args.event_id` and `args.alias`; optional `args.result`. Returns
`{ "changed": true }` when state changed.

`mailbox-retract`

Requires `args.event_id` and `args.alias`. Retracts a sent event when allowed
by mailbox rules and returns the updated event.

`mailbox-dismiss`

Requires `args.event_id` and `args.alias`. Hides the event from that recipient
and returns `{ "changed": true }` when state changed.

`mailbox-count`

Requires `args.alias`; optional `args.project_id` and `args.global`. Returns
`{ "unread": 0 }`.

`mailbox-clear`

Requires `args.alias`; optional `args.project_id` and `args.global`. Clears
read events for that recipient and returns `{ "cleared": 0 }`.

`mailbox-reply`

Requires `args.event_id` and `args.body`; optional `from`, `kind`, `subject`,
and `structured`. Replies to the original sender and preserves/creates the
thread correlation id.

`mailbox-sent`

Resolves the sender from `args.sender` or request context. Optional `args.to`
and `args.limit`. Returns `{ "event": ..., "state": ... }` rows.

`mailbox-events`

Streaming command. Sends live mailbox mutation frames from daemon-owned
mailbox state. The daemon does not turn these into desktop notifications; each
client decides how to render or notify.

Frames:

```json
{ "type": "ready" }
{ "type": "event", "event": { "kind": "posted", "event": {} } }
{ "type": "warning", "message": "dropped 2 buffered mailbox event(s)" }
{ "type": "error", "error": "message" }
```

`bus-publish`

Requires `args.topic` and either non-empty `args.body` or non-null
`args.structured`. Optional `from`, `kind`, `subject`, and `project_id`.
Returns the created topic event.

`bus-tail`

Optional `args.topic`, `args.project_id`, `args.global`, and `args.limit`.
Returns topic-filtered events or the full firehose when no topic is supplied.

`bus-subscribe`

Requires `args.alias` or pane context plus `args.pattern`; optional
`args.project_id`. Creates a durable subscription and returns it.

`bus-unsubscribe`

Requires `args.id`. Returns `{ "removed": true }` when the subscription was
present.

`bus-subscriptions`

Optional `args.alias`, `args.project_id`, and `args.global`. Returns durable
subscriptions, filtered when requested.

`subscription-events`

Streaming command. Sends live bus subscription create/remove frames from
daemon-owned subscription state. Desktop clients forward these into the
existing `subscription-event` Tauri channel.

Frames:

```json
{ "type": "ready" }
{ "type": "event", "event": { "kind": "created", "subscription": {} } }
{ "type": "event", "event": { "kind": "removed", "id": "subscription-id" } }
{ "type": "warning", "message": "dropped 2 buffered subscription event(s)" }
{ "type": "error", "error": "message" }
```

## Project Commands

`project-list`

Returns all daemon projects.

`project-create`

Requires `args.name`; optional `args.id`. Creates a daemon-owned project and
returns it.

`project-remove`

Requires `args.id`. Removes the daemon-owned project and clears matching
session project references.

`project-rename`

Requires `args.id` and `args.name`. Renames a daemon-owned project.

`project-update`

Requires `args.id` and `args.patch`. Applies a `ProjectUpdate` patch and
returns the updated daemon-owned project.

## Notes Commands

The daemon handles the same notes socket commands as the desktop socket:

- `notes-read`
- `notes-write`
- `notes-append`
- `notes-path`
- `notes-search`
- `notes-vault-root`

These commands use the daemon host's configured notes vault root
(`settings.notes_vault_root`, or `~/Documents/Roux` by default). They share the
same `roux-runtime::notes_service` implementation as the desktop adapter, so
scope resolution, slug freezing, frontmatter, append formatting, and tag search
stay consistent across clients. When Roux.app is connected to a daemon, its
notes panel commands forward to these daemon commands instead of reading or
writing the desktop host's local vault.

`notes-read`

`args` is a notes target: `{ "scope": "global|project|repo|session",
"sessionId": "...", "topic": null }`. Returns
`{ "path": "...", "content": "..." }`.

`notes-write`

Requires `args.target`, `args.content`, and optional `args.tags`. Replaces the
markdown body while preserving frontmatter fields managed by the notes service.

`notes-append`

Requires `args.target`, `args.content`, optional `args.timestamped`, and
optional `args.tags`.

`notes-path`

Requires `args.target`; optional `args.dir` returns the containing scope
directory instead of the note file path.

`notes-search`

Requires non-empty `args.tags`; optional `args.scope` restricts the search and
optional `args.exact` disables hierarchical prefix matching.

`notes-vault-root`

Returns the daemon host's notes vault root path.

## Automation Hook Commands

The daemon handles automation hook management and execution for connected
clients:

- `hook-show`
- `hook-preview`
- `hook-run`
- `hook-approve`
- `hook-clear-approvals`
- `hook-log-list`
- `hook-log-read`

These commands use the daemon host's hook config root, project hook files,
approval file, and hook log directory. Roux.app forwards its automation hook
panel commands to these endpoints when connected to a daemon; the desktop still
owns hook-install/setup UI.

`hook-show`

Optional `args.repoPath` includes project hooks from
`<repoPath>/.config/roux/hooks.toml`. Returns hook definitions with approval
metadata.

`hook-preview`

`args` is a hook run request with `event` and optional `repoPath`,
`worktreePath`, `branch`, `sessionId`, `projectId`, `taskId`, `scope`,
`provider`, and `args`. Returns rendered hook previews and match/approval
metadata.

`hook-run`

Uses the same request shape as `hook-preview`. Blocking `pre-*` hooks are run
synchronously; non-blocking hooks run in the daemon background. Returns
`{ "event": "...", "ran": N }`.

`hook-approve`

Requires `args.approvalId`. Records approval in the daemon host's hook approval
store.

`hook-clear-approvals`

Clears the daemon host's hook approval store.

`hook-log-list`

Returns hook log metadata from the daemon host.

`hook-log-read`

Requires `args.path`. Returns the hook log content after validating that the
path belongs to the daemon hook log directory.

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

`run`

Compatibility alias for top-level `roux run` when the daemon owns the socket.
Same behavior as `daemon-process-start`.

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

The daemon owns durable watch definitions, check execution, watch hooks, and
runtime state. Clients subscribe to watch updates and decide how those events
become desktop notifications, badges, logs, or no visible UX. The daemon does
not call Tauri notification APIs.

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

Requires `args.watch`. Replaces or inserts the full watch record. This is
primarily for external/admin clients that need to sync a full watch snapshot.

`watch-remove-for-session`

Requires `args.sessionId`. Removes all session-scoped watches for that session.

`watch-cleanup-orphans`

Removes session/project scoped watches whose owning session/project is no
longer present in the daemon runtime host.

`watch-events`

Streaming command. Optional `args.backlog` defaults to `true`; when enabled,
the daemon sends the current watch list as `changed: false` update frames after
the ready frame.

Frames:

```json
{ "type": "ready" }
{ "type": "update", "event": { "watch": {}, "changed": true, "previousOutcome": null } }
{ "type": "warning", "message": "dropped 2 buffered watch event(s)" }
{ "type": "error", "error": "message" }
```

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

`send`

Compatibility alias for `roux session send` when the daemon owns the socket.
Requires `args.text`; `args.enter` defaults to `true` and appends carriage
return. Targets `pane_id` when present, otherwise `args.pane_type` within the
session, otherwise the session's primary daemon PTY.

`latest-output`

Compatibility alias for MCP/desktop latest-output reads when the daemon owns
the socket. Targets `pane_id` when present, otherwise resolves the same daemon
PTY target shape as `send`. Optional `args.max_bytes` / `args.maxBytes` controls
retained replay size, capped by the daemon. Returns `session_id`, `pane_id`,
`pty_id`, `max_bytes`, `byte_count`, `replay_bytes_base64`, and `text` when the
replay bytes are valid UTF-8.

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

The `roux attach` CLI command is a terminal frontend for this stream. It
resolves either a direct PTY id or a session's primary PTY, writes replay/live
bytes to stdout, and forwards stdin through separate `daemon-pty-write`
requests.

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
- Durable watch definitions, check execution, watch hooks, and runtime state.
- Durable alias, mailbox event, read-state, and bus subscription state.
- Notes vault operations.
- Automation hook list/preview/run/approval state and hook logs.

Desktop-owned:

- Rendering, pane layout, xterm instances, and UX-only state.
- Watch notification presentation from daemon `watch-events`.
- Alias mirror updates from daemon `alias-events`.
- Mailbox panel rendering, notification presentation from daemon
  `mailbox-events`, subscription UI updates from `subscription-events`, and
  last-mile deliver-to-pane UX.
- Pane-state cleanup and frontend-owned pane layout restore files.
- Explicit development fallback runtime when `ROUX_DAEMON_AUTOSTART=0`.
