# Multi-AI Panes Per Session

## Summary

A Roux session becomes a generic workspace container that can hold any number of AI panes, mixing Claude and Codex freely alongside the existing shell, markdown, and command pane types. Provider moves from the session to the pane. Each AI pane owns its own PTY and lifecycle, and closing the last AI pane leaves the session open as a plain workspace.

This spec extends the in-flight Codex CLI support plan (`docs/plans/2026-04-10-codex-cli-support-via-services-commands.md`) and supersedes that plan's `Session.provider` field. Every other decision in the parent plan stands — the `providers/` module, the provider-aware spawn config, the normalized hook payload, the notification service, the provider-integrations setup UI.

Backward compatibility with existing persisted pane state is explicitly not a goal. On upgrade, old pane-state files whose descriptors still use the pre-rename shape may be discarded.

## Scope

**In scope**

- A session may contain zero or more AI panes, each with its own provider (Claude or Codex).
- `Session` has no `provider` field. Provider lives exclusively on AI panes.
- AI PTYs are pane-owned; closing an AI pane kills its process, with a confirm-on-close prompt if the pane is mid-turn (gated by the existing `confirmOnClose` setting).
- Session card status is a derived aggregate over that session's AI panes: `generating` if any pane is generating, else `disconnected` if any are disconnected and none are idle, else `idle` if any are idle, else no AI status at all.
- Notifications fire per-pane. One `generating → idle` transition on one pane is exactly one notification. The title/body include the pane name and provider so stacked notifications are distinguishable.
- Closing the last AI pane leaves the session open as a shells-only workspace.
- Hook routing is pane-aware: spawn always sets `ROUX_PANE_ID` in addition to `ROUX_SESSION_ID`; the status watcher matches on `roux_pane_id` first, with legacy fallbacks scoped to notification-only updates (defined in detail under "Hook Routing And Legacy Compatibility" below).
- The new-session dialog picks the initial AI pane's provider; additional AI panes are added via pane-menu split actions.
- On startup restore, AI panes come back in the disconnected state and wait for the user to click Reconnect in the dead-pane view — same as today's session-restore behavior. This spec explicitly does **not** auto-respawn AI panes on restore.
- The hard-coded "main" AI pane invariant (each session has a non-closeable `<sessionId>-main` Claude pane) is removed. Any AI pane can be closed, and a session may temporarily hold zero AI panes.

**Out of scope**

- Permission-request parity for Codex (deferred in the parent plan; nothing here changes that).
- Detach-and-reattach semantics for AI processes. AI panes live and die with their pane, like shells.
- Multiple UI views of a single AI process.
- Reconciling Roux's notion of "session" with Claude or Codex provider-side session files — each AI pane independently manages its own provider-side resume.
- Visual provider differentiation beyond the pane-header label (no color stripes, no branded backgrounds).
- Detection-only "adopt an unmanaged shell as an AI session" flow. Dedicated AI panes are the primary, authoritative model: Roux spawns the agent, owns the PTY, injects the env, and correlates hook events deterministically. A future layer may notice hook activity from a shell-launched agent and offer a best-effort "external agent activity" indicator or an "adopt session" affordance, but that lives on top of the managed model rather than replacing it, and is out of scope for this spec.

## Data Model

### Frontend

`src/lib/panes/instances.ts`, `src/lib/panes/persistence.ts`:

```ts
export type PaneType = "ai" | "shell" | "markdown" | "command";

export type Provider = "claude" | "codex";

export type AiPaneStatus = "idle" | "generating" | "disconnected";

export interface PaneInstance {
  id: string;
  type: PaneType;
  ptyId: string;
  provider?: Provider;        // required iff type === "ai"
  aiStatus?: AiPaneStatus;    // required iff type === "ai"; runtime-only
  // ...existing fields unchanged
}

export interface PaneDescriptor {
  id: string;
  type: PaneType;
  ptyId: string;
  provider?: Provider;        // persisted; required iff type === "ai"
  // ...existing fields unchanged
}
```

- `aiStatus` is runtime-only. It is not persisted; on restore it defaults to `disconnected` (per the restore behavior described above) and is updated by incoming hook events.
- Every current `type === "claude"` check in the frontend is rewritten. Most become `type === "ai"`. A small number (the Claude Allow/Deny permission UI) become `type === "ai" && provider === "claude"`.
- Legacy `PaneType = "claude"` is removed, not kept as an alias.
- The frontend adds its own schema version marker to the pane-state payload (e.g. `{ schemaVersion: 2, layout, descriptors }`). The Rust `pane_state.rs` envelope version stays at `1` because the backend treats the blob opaquely. On load, the frontend rejects any payload whose `schemaVersion` is missing or older than 2 and starts that session empty — acceptable per the "no backward compat" scope rule.

### Backend

`src-tauri/src/services/sessions.rs`, `src-tauri/src/services/providers/`:

- `Session` does **not** gain a `provider` field. This supersedes the parent plan's corresponding change.
- `Session.status` is removed from the persisted session model. Aggregate session status is derived entirely on the frontend from pane-level `aiStatus`. This change ripples through `crates/roux-core/src/models/session.rs`, `src-tauri/src/session_service.rs`, `src/lib/stores/sessions.ts`, `src/App.svelte`, and `src/lib/sessions/close.ts`, each of which reads `session.status` today. The implementation plan will sequence those touchpoints explicitly; the spec's position is "no persisted session status, derived-only, one atomic change".
- PTY spawn moves to a single entry point at the service layer: `spawn_ai_pane { session_id, pane_id, provider, cwd, ... }`. The `providers/` module from the parent plan resolves provider-specific spawn args/env. Internally, `create_session` no longer spawns an AI process; the Tauri command layer calls `spawn_ai_pane` as a separate step for the first pane.
- The socket `session-create` command (`src-tauri/src/socket.rs`) keeps its one-shot create-and-spawn semantics as a convenience for external CLI callers — it internally calls both `create_session` and `spawn_ai_pane` and returns the same `session-created` response. The socket wire protocol does not change; only the internal plumbing does.
- `status_watcher.rs` currently hardcodes `claude_session_id` in its payload shape and emits a Claude-labeled attention notification source (`status_watcher.rs:24`, `:163`, `:256`). These are generalized as part of this spec: the payload carries `provider`, `roux_session_id`, `roux_pane_id`, and `provider_session_id`, and the attention source is set from the incoming event's provider.
- `hooks.rs` / `cli.rs` hook bridge is extended to read `ROUX_PANE_ID` from env and emit it in the status JSON alongside the existing `roux_session_id` and `claude_session_id` fields.
- Pane state persistence stays opaque to Rust. Only the in-memory frontend shape and the frontend's own schema version marker change.
- Settings-level Claude binary path, default model, and additional flags remain session-wide inputs consulted at each `spawn_ai_pane` call.

### Hook protocol

- Spawn env **always** sets `ROUX_PANE_ID` alongside the existing `ROUX_SESSION_ID`. This is unconditional for anything Roux spawns, which means every normally-spawned AI process produces unambiguous hook events.
- Hook bridge (`roux-cli hook`) writes `~/.config/roux/status/<uuid>.json` with a shape that includes both ids plus provider metadata:

  ```json
  {
    "provider": "claude",
    "roux_session_id": "...",
    "roux_pane_id": "...",
    "provider_session_id": "...",
    "cwd": "...",
    "status": "idle",
    "message": "...",
    "tool_name": "...",
    "tool_input": "..."
  }
  ```

- Frontend status event payload shape: `{ paneId, sessionId, provider, status, model?, cost?, toolName?, toolInput? }`. Claude-specific field names such as `claudeSessionId` are removed from the watcher output and the frontend listener.

### Hook Routing And Legacy Compatibility

Hook routing is built around the rule that **any event spawned by Roux carries `roux_pane_id`**, so the common path has no ambiguity.

The fallback tiers exist solely for legacy hook installs that predate `ROUX_PANE_ID` — i.e., older Claude hook configs written by earlier Roux versions that set only `ROUX_SESSION_ID`, or hooks installed by hand.

`status_watcher.rs` matches in order:

1. **Exact `roux_pane_id`.** This is the expected path for every Roux-spawned AI process going forward. Updates the named pane's `aiStatus` directly.
2. **`roux_session_id` only (legacy).** Event has `roux_session_id` but no `roux_pane_id`. Routed to the session *as an aggregate hint only* — specifically, a legacy event does **not** update any individual pane's `aiStatus`. Instead it is forwarded to the notification service so window-attention / idle chimes still fire, and the session card reflects it only if the session currently has exactly one AI pane. If the session has zero or multiple AI panes, the event is logged and discarded (we refuse to guess which pane it belongs to).
3. **`cwd` alone (last resort).** Event has neither id. Treated identically to tier 2: notification-only, applied to a session at that cwd if and only if it has exactly one AI pane at that cwd; otherwise logged and discarded.

This rule set is deterministic, never silently updates the wrong pane, and degrades cleanly in the one case it genuinely can't resolve. The cost is that legacy same-cwd multi-pane users lose fine-grained status until they reinstall their hook config — acceptable because anyone with multi-pane sessions will already have the new spawn path.

This differs from the parent plan, which proposed `provider + cwd` as the fallback (no session id). Keeping session id in tier 2 is strictly more information and lets us distinguish two same-cwd sessions belonging to different Roux sessions — which is the much more common form of collision than two same-cwd sessions inside the same Roux session. The parent plan's rule is subsumed by tier 3.

## Session Creation And Pane Lifecycle

### Creating a session

1. User opens the new-session dialog, picks provider, cwd, worktree options.
2. Frontend calls `create_session` (no provider in the payload); receives `session_id`.
3. Frontend immediately calls `spawn_ai_pane { session_id, pane_id, provider, cwd }`, receives `pty_id`.
4. Frontend creates the pane instance (`type: "ai"`, `provider`, `pty_id`) and a single-leaf layout pointing at it. Persists pane state.

The session-creation and first-AI-pane steps are distinct calls, even though the UI presents them as one action. This keeps `spawn_ai_pane` the only path that creates an AI pane, so the code path is exercised and tested once.

### Adding another AI pane

- Pane menu and command palette gain `Split right → Claude`, `Split right → Codex`, `Split down → Claude`, `Split down → Codex`, grouped under an "AI" submenu next to the existing `Split → Shell` commands.
- Action: `spawn_ai_pane { session_id, pane_id: <new>, provider, cwd: <from focused pane> }`; insert a new leaf into the layout next to the focused pane.
- cwd defaults to the focused pane's cwd (matching how splitting a shell works today), not the session's original cwd.

### Closing an AI pane

- `disposePane` stops skipping PTY kill for AI panes. AI panes kill their PTY on dispose, same as shells.
- Mid-turn guard: if `aiStatus === "generating"` and `confirmOnClose` is set, prompt. Otherwise kill immediately.
- If this leaves zero AI panes in the session, the session stays open. Aggregate session status becomes null (no AI).

### Closing a session

- Iterate pane descriptors, dispose each. AI PTYs are killed pane-by-pane. There is no longer any session-level AI process to clean up separately.
- Delete pane state and remove the session record per today's flow.

### Reconnect after a PTY exit

- When the backend observes an AI pane's PTY exit, it emits a new `pane-disconnected` event `{ paneId, sessionId, provider, exitCode? }`. The frontend listener flips that pane instance into a disconnected state: `aiStatus = "disconnected"`, and `PaneAI.svelte` renders a dead-pane view (shared with the existing shell-restore-error dead-pane view) with a "Reconnect" button.
- `reconnect_ai_pane(pane_id)` spawns a fresh PTY for that pane's provider and calls `replacePty` on the instance. On success, the dead-pane overlay comes down and `aiStatus` resets to `idle` until the next hook event.
- A "Reconnect session" convenience command loops over the session's AI panes in the disconnected state and calls `reconnect_ai_pane` on each. Optional for v1.
- The dead-pane transition is driven by the backend PTY event, not by missing hook events. Hook events that never arrive have no effect on `aiStatus`; only an actual process exit flips a pane to disconnected.

### Startup restore

- Existing session-restore path loads pane state, walks descriptors, and rehydrates instances. AI pane descriptors carry `provider`.
- On restore, AI panes come back **in the disconnected state with no PTY**, matching current behavior. They render the dead-pane view with a Reconnect button; clicking it calls `reconnect_ai_pane` which runs the `spawn_ai_pane` path.
- This is a deliberate choice to match today's session-restore flow (sessions restore disconnected and wait for user action — see `src/App.svelte` and `src-tauri/src/session.rs`). Auto-respawn on restore is explicitly out of scope; if we want it later, it's a one-line change in the restore loop.
- Shell panes on restore continue to behave as they do today (auto-respawn their shell PTY), unchanged.

### Main-pane invariant removal

Today's code hard-codes a non-closeable `<sessionId>-main` Claude pane per session. This spec removes that invariant. Concretely:

- The `mainPaneId(sessionId)` helper and every check of the form `paneId === \`${sessionId}-main\`` are deleted. AI pane ids become opaque like shell pane ids.
- `src/lib/panes/actions.ts` no longer refuses to close a pane just because it is the session's main pane.
- `src/lib/queries/index.ts` lookups that assume a main pane exists per session are rewritten to "find the first AI pane in the session, or null".
- `src/lib/sessions/reconnect.ts` targets an explicit pane id instead of the session's main pane.
- Any UI surface that needs "the session's primary AI pane" (e.g., focus-on-activate) uses "most-recently-focused AI pane in this session, falling back to the first AI pane in layout order, falling back to any pane".

This is a foundational invariant change, not a mechanical rename. The implementation plan should sequence it early (before the `claude → ai` type rename) so that subsequent changes operate on the new model.

## Status Aggregation And Notifications

The session card and the notification service serve different jobs and use different rules.

### Session card (aggregate)

- A new frontend derived store, `sessionAiStatus`, walks each session's AI panes and returns:
  - `generating` if any pane is generating
  - else `disconnected` if any pane is disconnected and none are idle
  - else `idle` if any pane is idle
  - else `null` (no AI status — the session has no AI panes or they are all in an indeterminate state)
- The sidebar card subscribes to `sessionAiStatus` instead of reading a session-level `status` field. Dot color uses the aggregate. No provider badge on the card; a session with mixed providers should not have to pick one to display.
- Hover or an expanded view could later show per-pane dots, but that is not in v1.

### Notifications (per-pane)

- The notification service receives `pane-status-update` events and fires on each pane's `generating → idle` transition independently. Two panes finishing in the same session produces two notifications.
- Notification title and body include the pane name and provider, for example `Claude (main) — turn complete` or `Codex (codex-2) — turn complete`, so stacked notifications are distinguishable.
- Disconnect events fire per pane.
- Window-focus suppression remains global and unchanged.

The aggregate and per-pane rules are deliberately different abstractions: the card answers "is this session busy?" at a glance, while the notification answers "did the specific work I was waiting on just finish?"

### Permission UI (Claude only)

- The inline Allow/Deny permission UI is scoped to `type === "ai" && provider === "claude"`. Codex panes never render it.
- The concurrent notification-pane plan is already retiring the inline buttons in favor of the notification service. This spec does not need to solve that, only to avoid making it worse. The permission UI remains pane-scoped, which is the direction the notification plan is already moving.

## UI Changes

### New-session dialog

- Provider picker (Claude, Codex) as a segmented control. No "none" option; a session always starts with one AI pane, per the creation flow.
- Claude-only controls (default model, additional flags) hide when Codex is selected. Codex binary path lives in settings, not the dialog.

### Pane menu and command palette

- New split commands: `Split right → Claude`, `Split right → Codex`, `Split down → Claude`, `Split down → Codex`, grouped under an "AI" submenu alongside the existing shell split commands.
- Command palette entries mirror the menu.
- Default new-AI-pane provider: most-recently-focused AI pane's provider in that session, falling back to Claude if the session has no AI panes.

### Session card

- Subscribes to `sessionAiStatus`. Dot color and text come from the aggregate.
- No provider badge in v1.

### Pane header (AI panes)

- Displays provider name, pane name, and current `aiStatus` dot.
- Pane names default to `claude`, `codex`, `claude-2`, `codex-2`, auto-numbered within the session. Users can rename.
- Claude panes render the existing Allow/Deny permission UI where applicable; Codex panes do not.

### Claude-only UX surfaces that get provider-scoped

Today's Claude-specific surfaces that need to be scoped behind `provider === "claude"` (or rethought as provider-aware in a later spec):

- `src/lib/components/SessionPicker.svelte` — Claude-specific "resume a past Claude session" picker. In v1, it only opens from a Claude pane header; Codex panes have no equivalent.
- `src/lib/components/PaneShell.svelte` — currently embeds Claude-specific UI branches; these are guarded behind `provider === "claude"` so a Codex AI pane renders a minimal header without Claude controls.

These guards are part of the mechanical `type === "claude"` → `type === "ai"` rewrite pass and do not need their own work item in the plan.

### Settings

- Per the parent plan: `claudeBinaryPath`, `codexBinaryPath`, `defaultModel`, `additionalFlags`, `sessionNotificationsEnabled`. Unchanged from parent plan.
- `defaultModel` and `additionalFlags` remain Claude-scoped in v1.

### What is explicitly not changing

- Pane-tree invariants, split flattening, tab stacking, worktree UX, notes, watches, tasks.
- Shell, markdown, and command panes.
- The socket / CLI protocol. The parent plan already covers provider-awareness there.

## Test Plan

### Frontend unit (Vitest)

- `panes/instances.ts`:
  - Creating an AI pane requires `provider`. Creating without it is a type error and a runtime guard.
  - `disposePane` on an AI pane kills its PTY.
- `panes/persistence.ts`:
  - Round-trip save then load of a layout containing `{ type: "ai", provider: "claude" }`, `{ type: "ai", provider: "codex" }`, shells, and markdown panes.
  - Command panes still strip on restore; AI panes do not.
- Pane actions:
  - `Split right → Claude` and `Split right → Codex` insert a new AI pane leaf next to the focused pane with the correct provider.
  - Default-new-provider selection: most-recently-focused AI pane wins; falls back to Claude when no AI panes exist.
- `sessionAiStatus` derived store:
  - `generating` when any pane is generating, regardless of the others.
  - `disconnected` when any pane is disconnected and no pane is idle.
  - `idle` when at least one pane is idle and none is generating (a disconnected pane alongside an idle one still yields `idle`).
  - `null` when the session has no AI panes.
  - Updating one pane's `aiStatus` propagates to exactly one session's aggregate and no others.
- Notifications:
  - `generating → idle` on one pane fires one notification for that pane. Two panes finishing fires two notifications.
  - Notification title and body include the pane name and provider.
  - Window-focus suppression still applies.
  - Disconnect fires per pane.

### Backend unit (Rust)

- `providers/` spawn config generation for Claude and Codex (inherited from the parent plan).
- `spawn_ai_pane` sets `ROUX_SESSION_ID` and `ROUX_PANE_ID` in the PTY env; returns the expected pty id; rejects a missing or unknown provider.
- `status_watcher`:
  - Updates the named pane's `aiStatus` when `roux_pane_id` is present.
  - For legacy events (only `roux_session_id`, or only `cwd`), routes to the session as an aggregate/notification hint — never updates a specific pane's `aiStatus`. Applies only when the session has exactly one AI pane at that cwd; otherwise the event is logged and discarded.
  - Drops events that match nothing, with a log line.
- `pane_state` envelope round-trips opaquely — no schema work server-side.

### Integration and manual

- Create a session with Claude; verify pane spawns, status updates, notification fires on idle.
- Split right → Codex in the same session. Both panes run independently. Card shows aggregate status. Notifications distinguish the two panes by name and provider.
- Close the Claude pane mid-turn: confirm prompt appears, accept, PTY dies, session remains open with Codex pane alive.
- Close the last AI pane: session stays open as shells-only, card shows null AI status.
- Kill a Codex process out-of-band: dead-pane view shows Reconnect; clicking restores the pane.
- Full quit plus relaunch with restore-sessions enabled: both AI panes restore in the disconnected state with Reconnect buttons; clicking each one runs `reconnect_ai_pane` and brings the process back.

### What is not tested here

- Codex permission-request parity (out of scope; deferred in the parent plan).
- Detach-and-reattach semantics (intentionally not supported).

## Open Questions

None blocking. Three items left to the implementation plan:

1. Exact shape of the `spawn_ai_pane` command and how it interleaves with the existing `create_session` / `reconnect_session` surface. The parent plan's `providers/` module design determines most of this.
2. Whether the "Reconnect session" convenience command ships in v1 or waits for a follow-up. Trivial to add later.
3. Sequencing of the main-pane invariant removal against the `claude → ai` type rename — the plan should order them so each step leaves the app in a runnable state.
