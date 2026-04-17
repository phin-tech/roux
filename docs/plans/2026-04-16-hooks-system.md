# Hooks System Plan

## Summary

- Add a generalized Roux hooks system that turns internal app activity and external agent/provider status changes into a single normalized event stream.
- Let users register locally-running hooks in KDL at both global and repo scope:
  - `~/.config/roux/hooks.kdl`
  - `<repo>/.roux/hooks.kdl`
- Execute hooks as supervised child processes owned by Roux, not as in-process plugins.
- Send the full event envelope to the hook process as JSON on `stdin`; keep KDL as the human-authored config surface.
- Require explicit execution policy on every hook run: timeout, concurrency behavior, and loop-prevention metadata.
- Use `roux-cli` as the automation control plane for hooks. Hook scripts should open sessions, panes, notifications, and future automation primitives through the CLI or socket boundary rather than private in-process APIs.

## Goals

- Unify today's Claude hook bridge with Roux-native events such as watch completion, session lifecycle, pane lifecycle, and agent attention transitions.
- Make hooks predictable and debuggable enough for real automation, especially background workflows like PR-watch-driven repair sessions.
- Keep the system safe by default for a desktop app with real filesystem, process, PTY, and worktree side effects.
- Reuse existing Roux architectural seams where possible:
  - the current `roux-cli` + socket boundary
  - existing event producers like watches and agent lifecycle sources
  - existing trusted-workspace settings

## Non-goals

- In-process plugin APIs for arbitrary user code.
- A full sandbox or container runtime in v1.
- A visual hook editor in v1.
- An arbitrary boolean or scripting expression language in the hook config.
- Full provider parity on day one. Claude compatibility and Roux-native events are enough for the first pass.

## Decisions Locked In

- Hook registration format is KDL. Do not introduce TOML or YAML for this feature.
- Hook process input is JSON on `stdin`. This is a wire format, not a second config surface.
- Hooks run out of process under Roux supervision.
- Repo hooks are gated by workspace trust. Untrusted repos do not get to execute `.roux/hooks.kdl`.
- Global and repo hooks both participate in matching. Repo hooks extend or override global hooks according to stable merge rules.
- Hooks should automate Roux through `roux-cli`, not internal Rust APIs.
- Hook execution must have timeouts and descendant cleanup from day one.
- Hook config should stay intentionally small in v1. Applicability logic belongs in the hook script unless real evidence proves otherwise.

## User-Facing Model

### Config locations

- Global hooks: `~/.config/roux/hooks.kdl`
- Repo hooks: `<repo>/.roux/hooks.kdl`

### Scope model

- Global hooks apply everywhere.
- Repo hooks apply to any session or event whose `repo_root` matches that repository.
- Worktrees inherit repo hooks from the owning repo root.
- The event payload should include both `repo_root` and `worktree_path` so hooks can reason about either.

### Trust model

- Repo hooks only load when the repo root is present in `RouxSettings.trusted_workspaces`.
- Global hooks are always user-trusted because they live under the user's Roux config directory.
- If a repo is not trusted, Roux should surface that fact clearly in logs and any future UI, but it should not execute the repo hook file.

## Proposed KDL Shape

Keep the grammar intentionally small in v1. The config should answer "when should Roux spawn this hook process?" not "how should Roux evaluate business logic?"

```kdl
hooks {
  hook "pr-watch-auto-fix" {
    on "watch.completed"

    run {
      command "python3"
      arg ".roux/scripts/pr_watch.py"
      cwd "worktree"
      timeout-ms 300000
    }

    policy {
      concurrency "replace"
      dedupe-key "pr:${payload.pr_number}"
    }
  }
}
```

### Minimal v1 grammar

- `hooks { ... }` top-level container
- `hook "<id>" { ... }`
- `on "<event-name>"`
- `run { command "..."; arg "..."; cwd "..."; timeout-ms 30000 }`
- `policy { concurrency "..."; dedupe-key "..." }`
- optional `enabled true|false`

### Applicability split

- Roux config should do coarse routing:
  - event kind
  - scope
  - execution policy
- The hook script should do fine-grained applicability:
  - draft PR or not
  - failed checks count
  - whether the target fixer session already exists
  - whether review comments are actually actionable

That keeps Roux from growing a second programming environment before it has any evidence that one is needed.

### Keep out of v1

- Arbitrary boolean expressions
- Nested `and` or `or` trees
- Reusable predicates or macros
- Embedded matcher runtimes such as Rhai or Scheme
- Shell snippets as the primary command model

If shell convenience is needed, make it explicit with `command "sh"` + `arg "-c"` rather than adding a second execution mode.

## Event Model

Introduce a single normalized event envelope in `roux-core` so Rust, TypeScript, and `roux-cli` can all share the same type vocabulary.

```json
{
  "id": "evt_123",
  "kind": "watch.completed",
  "timestamp": "2026-04-16T20:30:00Z",
  "origin": {
    "kind": "watch",
    "causationId": null,
    "triggeredByHookId": null
  },
  "session": {
    "rouxSessionId": "sess_1",
    "paneId": "pane_2",
    "profile": "codex"
  },
  "workspace": {
    "repoRoot": "/repo",
    "worktreePath": "/repo/.worktrees/pr-123"
  },
  "payload": {
    "watchId": "watch_1",
    "outcome": "failure"
  }
}
```

### First event set

- `watch.completed`
- `session.created`
- `session.ended`
- `pane.opened`
- `agent.attention.entered`
- `agent.attention.exited`

### Event source mapping

- Claude hook bridge and future provider bridges become event sources, not special cases.
- Watch manager emits `watch.completed` on meaningful state transitions.
- Session and pane lifecycle emit normalized events from the Rust backend.
- Agent lifecycle registry emits attention enter or exit events in addition to its current notification work.

## Backend Architecture

Add a dedicated backend service, likely `HookManager`, and store it in `AppState`.

### Responsibilities

- Load and merge hook config from global and repo scopes
- Route hook definitions by normalized event kind and scope
- Enforce trust gating for repo hooks
- Spawn and supervise hook child processes
- Track hook runs, outcomes, timeouts, and captured logs
- Apply concurrency policy and dedupe policy
- Attach causation metadata to follow-on events

### Non-responsibilities

- Directly performing the automation itself
- Exposing private app internals to user scripts
- Owning business logic of watches, sessions, or agent state machines

### Relationship to current systems

- Keep the current provider-specific `hooks.rs` logic as an event source adapter initially.
- Reuse the existing socket and CLI boundary in `src-tauri/src/socket.rs` and `src-tauri/src/cli.rs`.
- Reuse trusted workspace settings already present in `RouxSettings`.
- Do not let watch manager or agent registry spawn user scripts directly. They should emit normalized events into the hook manager instead.

## Hook Execution Model

Hooks should run as supervised child processes. "Container process" here means a Roux-owned execution envelope, not Docker.

### Child process contract

- Full event envelope is written to `stdin` as JSON.
- Useful env vars are also provided:
  - `ROUX_HOOK_ID`
  - `ROUX_EVENT_ID`
  - `ROUX_EVENT_KIND`
  - `ROUX_REPO_ROOT`
  - `ROUX_WORKTREE_PATH`
  - `ROUX_SESSION_ID`
  - `ROUX_PANE_ID`
- Exit code `0` means success.
- Nonzero exit codes mean failure.
- `stdout` and `stderr` are captured for logs, not treated as structured protocol output.

### Script applicability semantics

- Hook scripts are expected to exit quickly when an event is not actually actionable.
- Exit code `0` should be treated as successful completion, including "not applicable" no-op exits.
- Nonzero exit codes should mean actual failure.
- If we later need better observability, add an optional reserved skip code or structured log convention only after real examples demand it.

### Mandatory execution controls

- Default timeout: `30000ms`
- Graceful shutdown window after timeout: `2000ms`
- Hard kill after grace period
- Max stdout capture: `64KB`
- Max stderr capture: `64KB`
- Default concurrency per hook: `1`

### Concurrency modes

- `drop`: if already running, drop the new invocation
- `queue`: enqueue and run later
- `replace`: terminate the old run and start the new one
- `parallel`: allow concurrent runs

`replace` or deduped latest-wins behavior is likely the correct default for noisy PR or watch-driven automations.

### Descendant cleanup

- On Unix, each hook run should own a fresh process group or session so Roux can terminate the full subprocess tree.
- On Windows, use the equivalent job-object-style containment.
- Killing only the direct child PID is not acceptable because hook scripts will often spawn Python, Node, `gh`, or shell subprocesses.

## Loop Prevention

This feature will be dangerous if Roux cannot distinguish user-originated events from hook-originated events.

### Required metadata

- every event gets an `id`
- every event gets `origin.kind`
- hook-triggered follow-on actions should carry `causation_id`
- events caused by hooks should record `triggered_by_hook_id`

### Default policy

- Hooks ignore hook-caused events by default.
- Opting into hook-caused events should be explicit in config and probably not part of v1 unless a concrete need appears.

Without this, a hook that opens a session in response to a watch failure can recurse indefinitely via `session.created` or `pane.opened`.

## CLI and Automation Surface

The existing CLI already has enough basic primitives to make hooks useful:

- create sessions
- create panes
- open shell panes
- open command panes
- focus panes or sessions
- list and inspect sessions
- send input to sessions
- push notifications

However, reliable hook automation will need idempotent targeting. A hook should be able to say "ensure exactly one fixer session exists for this PR" rather than creating duplicate sessions.

### Likely follow-on CLI additions

- session or pane tags
- `ensure`-style commands for sessions or panes
- richer `session list` metadata to support external reconciliation

These additions are not required to start the hook system, but they are likely required to make hook-driven automation robust.

## Merge and Override Rules

Start simple and deterministic.

- Load global hooks first.
- Load repo hooks second.
- Hook ids are unique within the merged view.
- If a repo hook reuses a global hook id, the repo hook replaces the global hook definition entirely.
- Disabled hooks stay in the merged set only long enough to suppress inherited hooks with the same id; they do not match events.

Avoid partial field-level inheritance in v1. Full replacement is easier to reason about and easier to debug.

## Logging and Debuggability

Add persistent or session-visible run records early. Without them, a hook system will be opaque and difficult to trust.

### Record per run

- hook id
- event id
- event kind
- start time
- end time
- duration
- exit code
- timed out or not
- killed or not
- stdout truncated or not
- stderr truncated or not
- causation id

### Future UI

Not required in v1, but the backend should preserve enough data that Roux can eventually show:

- which hooks were considered for an event
- which ones ran
- which ones were skipped due to trust, timeout, dedupe, loop-prevention rules, or disabled state

## Phased Implementation Plan

### Phase 1: Core types and config parsing

- Add `HookDefinition`, `HookRunPolicy`, and `RouxEvent` types in `roux-core`
- Add a `hooks.kdl` parser with strong validation and good line or column errors
- Add tests for parsing, merge behavior, and trust gating

### Phase 2: Hook manager and process supervision

- Add `HookManager` to `AppState`
- Implement event-kind routing, spawning, timeout handling, descendant cleanup, and run logging
- Keep execution boundary local to the app first; only split into a dedicated `roux-hook-runner` binary if the supervision surface becomes too awkward in-process

### Phase 3: First event producers

- Route watch transitions into the hook manager
- Route session lifecycle into the hook manager
- Route pane lifecycle into the hook manager
- Route agent attention lifecycle into the hook manager
- Keep current Tauri frontend events intact while adding normalized hook events in parallel

### Phase 4: Provider integration

- Move the existing Claude-specific hook bridge into the generalized event model
- Define a provider-agnostic path for future Codex or other agent hooks

### Phase 5: CLI hardening for automation

- Add tags or ensure-style commands for idempotent session or pane creation
- Add any missing list or lookup APIs required by practical hook scripts

### Phase 6: UI and docs

- Show hook config paths in settings or docs
- Add docs for the hook schema and event envelope
- Consider a lightweight hook run log viewer once backend records exist

## Test Plan

- Parser tests for valid and invalid `hooks.kdl`
- Merge tests for global and repo hook replacement by id
- Trust-gating tests for untrusted repo hook suppression
- Event-routing tests across first-party event kinds
- Supervisor tests for timeout, graceful shutdown, and forced kill
- Loop-prevention tests ensuring hook-caused events do not re-trigger by default
- CLI integration tests for representative hook scripts where practical

## Open Questions

- Should the first implementation expose any UI for opening or reloading hook files, or should that wait until the backend contract stabilizes?
- Do we want per-hook cwd tokens beyond `repo` and `worktree`, such as `session` or explicit absolute paths?
- Should repo hook files be allowed to reference executables outside the repo root in v1, or should that be constrained later by sandbox policy?
- Do we want a lightweight `roux hook test` or `roux events emit` command for local hook development?

## Recommendation

Proceed with a design-first implementation. The first coding task should be the pure `roux-core` side:

- define normalized event types
- define hook config types
- parse `hooks.kdl`
- merge global and repo hook sets

Then, when the execution layer is added, keep the first implementation intentionally dumb about applicability:

- route by event kind
- supervise the child process well
- let the hook script decide whether the event is actionable

That keeps the first step testable and low-risk before touching PTY, watch, or session lifecycles, and it avoids designing an embedded matcher language before it is needed.
