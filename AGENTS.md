# AGENTS.md

## Project Overview

Roux is a Tauri 2 + Svelte 5 desktop app plus a Rust headless runtime daemon, for managing multiple Claude Code terminal sessions in one native window or from CLI/socket clients.

It provides split panes and stacked tabs, persistent Claude and shell terminals, git worktree support, a daemon-owned Unix/TCP socket + CLI bridge, and daemon-owned runtime services (sessions, projects, PTYs, processes, watches, aliases, mailbox/bus state, notes, automation hooks), plus project notes/docs/tasks.

This codebase has real filesystem, process, and terminal side effects. **The cost of being wrong is higher than the cost of being slow.** Defensive reasoning is the default.

## Build & Run

```bash
npm install          # frontend deps
task dev             # frontend + Tauri
npm run test         # tests once
npm run test:watch   # tests in watch mode
npm run check        # Svelte type check
```

## Architecture

- **Frontend**: Svelte 5 runes (`$state`/`$derived`/`$effect`/`$props`), Tailwind 4, xterm.js. Writable stores in `src/lib/stores/`.
- **Desktop shell**: Tauri 2 command adapters, native windowing, tray/menu, notifications, pane layout, xterm.js rendering.
- **Runtime daemon**: `roux daemon` (in `crates/roux-cli`) starts the shared `roux-runtime` host and owns durable runtime state plus PTY/process lifetimes.
- **Shared runtime**: `crates/roux-runtime` holds the session/project/pane/process/PTY/watch services used by the daemon and by the desktop fallback.
- **Daemon client**: `src-tauri/src/daemon_client.rs` connects to or autostarts a daemon; Tauri commands route through it when the daemon advertises the capability.

**Persistence:** pane layouts in localStorage; runtime state is daemon-owned when connected; settings remain desktop-readable Rust state.

### Core Invariant

> **The daemon owns durable/runtime state when connected; the desktop adapts or renders it.** Desktop-local socket server and services run **only** as fallback when no daemon is connected. Never create split-brain ownership where both daemon and desktop persist or mutate the same state: sessions, projects, PTYs, watches, aliases, mailbox/bus, notes, hooks.

## Key Files

- `docs/v2/daemon-protocol.md` — daemon socket command surface
- `docs/v2/session-daemon.md` — daemon architecture + migration notes
- `src/lib/stores/panes.ts` — pane-tree model (splits, stacking, persistence); `SplitNode` recursive union
- `src/lib/components/SplitPane.svelte` — recursive pane renderer (pane-tree invariants live here)
- `src/lib/components/Terminal.svelte` — Claude Code terminal pane (xterm lifecycle)
- `src/lib/panes/terminalRegistry.ts` — keeps xterm instances alive across re-mounts
- `src/lib/commands/index.ts` — command palette + keybindings
- `src-tauri/src/main.rs` — Tauri bootstrap + command registration
- `src-tauri/src/daemon_client.rs` — daemon connect/autostart client
- `src-tauri/src/socket.rs` — desktop-local socket protocol (fallback only)
- `crates/roux-cli/src/daemon.rs` — daemon entrypoint, socket server, command routing
- `crates/roux-runtime/src/host.rs` — shared runtime host construction
- (other `*_service.rs` files under `roux-runtime`, `pty.rs`, `worktree.rs`, `watches.rs` exist; discover as needed)

## Conventions

### Frontend

- Runes only — never introduce legacy `$:` reactivity.
- Props via `$props()` with typed `Props` interfaces.
- Prefer immutable updates over in-place mutation.
- Keep pane-tree invariants intact; same-direction splits flatten.
- Capture stable non-reactive copies when wiring xterm callbacks.
- Tests live in adjacent `__tests__/` (Vitest).

### Backend

- Typed internal errors, not `Result<_, String>`.
- Keep Tauri command handlers thin — adapters, not the home for logic.
- Daemon-first: check `state.daemon_client` + daemon capabilities (from `daemon-status`) before falling back to desktop-local services. Don't hardcode protocol assumptions while the protocol is experimental.
- Keep GUI-only concerns (pane layout, xterm rendering, native menus/tray, notification presentation) in the desktop process.
- Be conservative with PTY/socket/process/daemon/filesystem lifecycles.
- **Never blindly kill `node`/`node.exe`** — Claude Code is a Node app. Avoid broad process-kill commands.
- **Never `git add .`** — stage files explicitly. Treat worktrees, branches, and local repo state as user data.

## High-Risk Failure Modes

Guard against these specifically:
- Breaking pane-tree invariants.
- Corrupting persisted session/project/settings state.
- Leaking PTYs, watchers, or background tasks.
- Split-brain daemon/desktop ownership of runtime state.
- Changing daemon socket payloads/capabilities without updating all callers (see protocol-change checklist).
- Wrong filesystem assumption that deletes or moves the wrong thing.
- Changing a Tauri/Rust contract without updating the frontend.

## Protocol & Contract Changes

When changing the daemon protocol, update **together**: `docs/v2/daemon-protocol.md`, the CLI socket callers, the Tauri `DaemonClient`, capability checks, and any MCP/frontend callers.

Before changing any Tauri command or backend payload, identify: which frontend caller depends on it, which persisted shape changes, and whether the CLI/socket and daemon protocol/capabilities depend on it.

If daemon autostart/detection changes, verify all three startup modes: (1) existing daemon connected, (2) daemon autostarted, (3) explicit local fallback with `ROUX_DAEMON_AUTOSTART=0`.

Don't guess Tauri behavior — check [tauri.app/start](https://tauri.app/start/) when unsure about commands, plugins, config, permissions, lifecycle, or packaging.

## Testing

TDD is the primary method. Prefer red-green-refactor: write/identify a failing test, make the smallest change to green, refactor after. One test at a time — run it, read it, then decide. If the correct test surface is unclear, ask before proceeding.

Test surface here may be frontend unit/component tests, store tests, Rust unit tests, integration-level Tauri/backend checks, or manual runtime verification.

Before marking any testing task complete, state:
```text
VERIFY: Ran [exact command] — Result: [PASS/FAIL/DID NOT RUN]
```
If it did not run, it is not complete. Common commands: `npm run test`, `npm run check`, targeted Vitest runs, targeted Rust builds/checks in `src-tauri`.

## Working Principles

**Rule 0 — Reality wins.** When reality contradicts your model, stop and update the model. On unexpected failure: (1) report raw output, (2) state your theory, (3) state next step and expected result, (4) wait for human confirmation before another risky step. Never silently retry or bury a failure.

**Predict, then check.** Before a risky or irreversible action, state what you expect to happen; after it, compare to what actually did. A mismatch means your model is wrong — stop and say so plainly (e.g. "I assumed pane close killed the PTY, but the manager keeps it alive until explicit kill").

**Verify vs. believe.** Say "I verified X" only after observing it in code, output, logs, or tests — otherwise call it a belief. "I don't know yet" beats fake confidence.

**Small batches.** Max ~3 meaningful actions, then checkpoint against observable reality (test/build output, logs, runtime behavior, code inspection). Commit checkpoints with meaningful messages. Plans and TODOs are not checkpoints.

**Root cause, not symptom.** Ask: what failed, why was the system in a state that allowed it, what invariant permitted that state.

**Chesterton's fence.** Don't delete code you can't explain. Extra caution: pane-tree logic, xterm lifecycle, terminal registry, PTY buffering/attach, daemon/client fallback boundaries, socket compatibility, settings migration.

**Review Frequently** Every few commits stop and use /roborev fix to clean up issues.

**No premature abstraction.** Stay concrete; extract only at the third real repetition.

**Estimate** Before you start a batch of work, estimate how many turns it will take. Try to do mini planning.

## When To Stop And Ask

Stop and ask the human when: requirements are ambiguous and blast radius is meaningful; there are multiple valid approaches with real tradeoffs; the repo is in an unexpected state; the action is destructive or hard to undo; or you're changing public contracts, stored data, or architectural direction.

Be direct. Surface contradictions instead of silently picking an interpretation. Push back when there's concrete evidence the requested path is unsafe. Don't say "you're absolutely right."

## Handoff

When stopping, leave: what's done, what's in progress, what's blocked, recommended next step, and which files were touched.
