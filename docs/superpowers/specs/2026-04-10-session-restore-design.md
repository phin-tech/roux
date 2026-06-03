# Session Restore — Full Pane/Shell Rehydration

**Status:** Design
**Date:** 2026-04-10
**Author:** Sam Phinizy (with Claude)

## Problem

When Roux is quit and reopened, sessions load in a "disconnected" state. Clicking **Reconnect** on a session currently only respawns the Claude terminal — all other panes the user had set up (shells, split layouts, stacked tabs, markdown panes) are lost.

The persistence infrastructure for pane trees and pane descriptors already exists in `src/lib/panes/persistence.ts`, backed by `localStorage`. It auto-saves on every layout change. But on app startup, `App.svelte` loads the persisted layout and immediately discards it with a `clearLayout(s.id)` call and a TODO comment:

> _"Full shell pane restore (spawn fresh PTYs for each persisted shell) is deferred to a later iteration."_

This spec closes that gap.

## Goals

1. Reopening Roux and clicking Reconnect restores the **full pane layout** of a session, including all shell panes and splits.
2. Shell panes are respawned fresh at their previous `workingDir`. No scrollback restore (deferred — see Non-Goals).
3. If a shell fails to spawn during restore (typically: worktree deleted), the user sees a clear explanation and a Retry button rather than a silent failure.
4. Pane state persistence moves from `localStorage` to a Rust-managed per-session file on disk, consistent with how Roux already stores sessions.

## Non-Goals

- **Scrollback restore.** Not in this release. The on-disk schema is versioned so it can be added later without a migration.
- **Migrating existing `localStorage` pane state.** The existing data is effectively broken (users never got their shells back anyway), so we drop it on first launch after upgrade.
- **Auto-retry of failed shell spawns.** User clicks Retry.
- **A dedicated "restore workspace" button.** Restore is triggered by the existing Reconnect action.
- **Save-on-create.** A brand-new session with just the main pane doesn't need a pane_state file; the first debounced save fires naturally on first split.

## Key Decisions

| Decision                | Choice                                                           | Rationale                                                                                                   |
| ----------------------- | ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Shell restore semantics | Fresh shell at same `workingDir`, no scrollback                  | Matches tmux/iTerm behavior on restart; minimum honest implementation                                       |
| Persistence layer       | Rust/disk, per-session JSON file                                 | Consistency with `sessions.json`, durability, inspectability, test-friendliness                             |
| On-disk schema          | Versioned opaque: `{ version: 1, data: <frontend-shaped JSON> }` | Rust stays dumb about contents; forward-compatible for scrollback and future fields                         |
| Migration               | None — delete old localStorage keys on first launch              | Existing data is already broken; nothing worth rescuing                                                     |
| Write cadence           | 1500ms debounce + flush on quit                                  | Balances live-feel with IPC/IO overhead; flush guarantees no loss on clean quit                             |
| Failure handling        | Dead placeholder pane with error message and Retry button        | Preserves the layout, explains the cause, enables recovery without app restart                              |
| Restore timing          | On reconnect click only (not on startup)                         | Session is disconnected-as-a-unit or restored-as-a-unit; no half-live state; all restore logic in one place |

## Architecture

### Data flow (high level)

```
User splits pane ─┐
User closes pane ─┼─→ sessionLayouts writable store
User resizes    ─┘          │
                            ├─→ initPersistence subscription
                            │          │
                            │          └─→ 1500ms debounce
                            │                   │
                            │                   └─→ savePaneState(sessionId, payload)
                            │                              │
                            │                              └─→ Tauri: save_pane_state
                            │                                     │
                            │                                     └─→ ~/.config/roux/pane_state/<id>.json
                            │
Quit event ─────────────────┴─→ flushPaneState() → forced immediate write

Reconnect click → loadPaneState(sessionId) → rehydratePanes() → sessionLayouts.set(...)
                                                    │
                                                    └─→ spawnShell for each shell descriptor
                                                          │
                                                          ├─→ success → live PaneInstance
                                                          └─→ failure → PaneInstance with restoreError set
```

### Storage layer (Rust)

**New module:** `src-tauri/src/pane_state.rs`

**File location:** `~/.config/roux/pane_state/<session_id>.json` (one file per session, directory auto-created on first write).

**On-disk format:**

```json
{
  "version": 1,
  "data": {
    "layout": { "kind": "leaf", "paneId": "sess123-main" },
    "descriptors": [
      { "id": "sess123-main", "type": "claude", "ptyId": "sess123" },
      {
        "id": "pane-abc",
        "type": "shell",
        "ptyId": "...",
        "workingDir": "/Users/you/code/project"
      }
    ]
  }
}
```

Rust treats `data` as `serde_json::Value` — opaque. Only `version` is inspected. Any file with a version Rust doesn't recognize is ignored (returns `None`, not an error) and overwritten on next save.

**Tauri commands:**

| Command             | Args                                         | Returns                     | Notes                                                                   |
| ------------------- | -------------------------------------------- | --------------------------- | ----------------------------------------------------------------------- |
| `load_pane_state`   | `sessionId: String`                          | `Option<serde_json::Value>` | Returns `data` field, or `None` if missing / unreadable / wrong version |
| `save_pane_state`   | `sessionId: String, data: serde_json::Value` | `Result<(), String>`        | Atomic write: tmp file + rename. Wraps `data` in `{version: 1, data}`   |
| `delete_pane_state` | `sessionId: String`                          | `Result<(), String>`        | Best-effort delete; non-existent file is not an error                   |

**Atomic write pattern:** write to `<file>.tmp`, `fsync`, rename over target. Standard pattern; prevents half-written files on crash.

**Session deletion hook:** in `src-tauri/src/services/sessions.rs::kill_session` (around line 179, right after `session_handle.remove(id).await?`), add a call to `delete_pane_state(session_id)`. Best-effort; logged on error but non-fatal. _Not_ in `session.rs` — that module only handles loading/path helpers.

**Loader diagnostics:** `load_pane_state` returns `None` on unreadable / corrupt / wrong-version files to keep the UX fallback clean, but **every failure path logs the cause** (file IO error, JSON parse error, version mismatch with the expected vs. actual version number). Without this, "my layout vanished" becomes undebuggable.

### Frontend persistence layer

**Changes to `src/lib/panes/persistence.ts`:**

1. **Delete** localStorage-backed functions and key constants: `saveLayout`, `loadLayout`, `clearLayout`, `savePaneDescriptors`, `loadPaneDescriptors`, `clearPaneDescriptors`, `LAYOUT_KEY`, `DESCRIPTOR_KEY`.

2. **Replace** with a unified per-session API wrapping the Tauri commands:

   ```ts
   export interface PaneStatePayload {
     layout: LayoutNode;
     descriptors: PaneDescriptor[];
   }

   export async function loadPaneState(
     sessionId: string,
   ): Promise<PaneStatePayload | null>;
   export async function savePaneState(
     sessionId: string,
     payload: PaneStatePayload,
   ): Promise<void>;
   export async function deletePaneState(sessionId: string): Promise<void>;
   ```

3. **Update `scheduleSave` / `initPersistence`:**
   - Debounce window: **300ms → 1500ms**
   - Save path becomes async; errors logged via `log()` and swallowed (same as existing silent-catch behavior)
   - Per-session saves now happen inside the debounced callback

4. **New export — `flushPaneState()`:** cancels the pending debounce timer and writes immediately for all dirty sessions. Called on `close-requested` / `quit-requested`.

5. **Combined payload rationale:** layout and descriptors are written and read together, and any skew between them is a bug. One file per session with both fields eliminates the consistency problem.

**Changes to `src/lib/tauri.ts`:**

Add wrappers around the three new Tauri commands:

```ts
export function loadPaneStateRaw(sessionId: string): Promise<unknown | null>;
export function savePaneStateRaw(
  sessionId: string,
  data: unknown,
): Promise<void>;
export function deletePaneStateRaw(sessionId: string): Promise<void>;
```

**Changes to `App.svelte`:**

- `onMount`: one-shot localStorage cleanup at the top:
  ```ts
  try {
    localStorage.removeItem("roux:pane-layouts-v2");
    localStorage.removeItem("roux:pane-descriptors");
  } catch {}
  ```
- `onMount`: **remove** the `loadLayout` / `clearLayout` block at lines 219–227 entirely. Startup no longer touches pane state.
- Inside the existing `close-requested` / `quit-requested` handlers (lines 185–187): add `await flushPaneState()` as the first line in each handler so any pending debounce lands before close/quit.

**What stays unchanged:**

- The `sessionLayouts` subscription pattern — `initPersistence()` still subscribes and schedules saves on change; only the save target changes.
- Auto-save triggers: every split / close / resize / rename still fires a debounced save.
- `PaneDescriptor` and `LayoutNode` type definitions.
- `stripCommandPanes` helper — unchanged, still used at restore time.

### Restore flow (reconnect)

**File:** `src/lib/sessions/reconnect.ts` (extend the existing `reconnect` function).

**When to rehydrate vs. plain reconnect.** `reconnectSession` is called from several places — not just after app restart. It's also triggered when a session disconnects mid-use (Claude CLI crashes, SessionCard button, SessionTabs, PaneShell). If the user already has splits open and Claude crashes, we **must not** rehydrate from disk — that would stomp existing live pane instances. So the first thing reconnect does is inspect the current in-memory layout:

- If the current `sessionLayouts` entry for this session is a **single leaf** matching `<sessionId>-main` (the default post-startup state), we _may_ rehydrate from disk.
- If the current layout has **any splits already rendered**, rehydration is skipped entirely and we fall through to today's main-pane-only reconnect. The disk state is stale relative to runtime; we trust runtime.

**New behavior for `reconnectSession(session, extraFlags)`:**

```
1. Set per-session `reconnecting` flag; bail if already set (prevents double-click
   races). Implementation: module-scoped Set<string> in reconnect.ts, added on entry,
   removed in a finally block so it's cleared even on error.

2. Inspect current in-memory layout for sessionId.
   If it is NOT a single main-only leaf → skip rehydration, run today's main-pane-only
   reconnect (unchanged), clear flag, return. This protects mid-session disconnects.

3. Load persisted pane state from disk:
   └─ loadPaneState(sessionId) → { layout, descriptors } | null

4. If no persisted state → main-pane-only reconnect, clear flag, return.

5. Validate the persisted payload (preflight, see below). If invalid → log the reason,
   main-pane-only reconnect, clear flag, return. Do NOT apply bad state.

6. Fast-path: if the persisted tree is a single leaf matching `<sessionId>-main`,
   run the main-pane reconnect, clear flag, return.

7. Otherwise, full rehydration:
   a. Strip command panes from the tree via stripCommandPanes helper.
      Command panes represent one-shot processes; they cannot be restarted.
   b. Reconnect the main Claude PTY (existing reconnectSessionPty logic).
      If this fails → abort the restore, clear flag, let existing error handling run.
   c. Collect non-claude leaves in tree-walk order (use collectLeafIds from layout.ts,
      filter out the main pane).
   d. For each leaf's descriptor, call rehydratePane(descriptor, session.worktreePath):
        - shell    → spawn fresh shell PTY at descriptor.workingDir, create PaneInstance
        - markdown → create PaneInstance directly (no PTY, preserve docPath)
        - command  → already stripped, never reached
   e. Apply the restored layout tree to sessionLayouts using the store's update
      pattern (see below).
   f. For each newly-created non-main instance, in tree-walk order:
        initTerminal(paneId)   // creates the xterm Terminal + FitAddon on the instance
        await attachPtyListeners(paneId)  // wires PTY output → terminal.write
      The initTerminal call MUST precede attachPtyListeners — otherwise early PTY
      output is dropped because instance.terminal is still null when the output
      channel callback fires. This matches the existing pattern in App.svelte:216-217
      and commands/panes.ts.
   g. Clear `reconnecting` flag.
```

**Integrity preflight (step 5).** Before applying any persisted tree, validate:

1. `layout` is a well-formed `LayoutNode` (recursive check).
2. `descriptors` has **exactly one** entry with `type === "claude"` and id `<sessionId>-main`.
3. Every leaf `paneId` in the tree maps to exactly one descriptor.
4. No duplicate descriptor ids.
5. No descriptor types outside `"claude" | "shell" | "command" | "markdown"`.

If any check fails → log the specific reason, return null from the load path, fall back to main-pane-only reconnect. Reason: without this, a corrupt file (e.g., partial write from a crash older than this design, or an old-schema file that slipped through the version guard) would render empty leaves in `SplitPane.svelte`/`PaneShell.svelte` _and_ the auto-save subscription would then persist the bad state back to disk, compounding the problem.

**Store mutation — the correct pattern.** `sessionLayouts` is a `writable<Map<string, LayoutNode>>`. The existing codebase (layout.ts, actions.ts) always mutates per-session entries via the `update` pattern, not `.set(...)`:

```ts
sessionLayouts.update((m) => {
  const next = new Map(m);
  next.set(sessionId, strippedTree);
  return next;
});
```

Step 7e uses exactly this pattern. Using `sessionLayouts.set(sessionId, strippedTree)` would replace the entire map with the wrong type and break everything — do not write that.

**Ordering constraint:** all `PaneInstance` objects must exist in `paneInstances` **before** the layout tree is applied in step 7e, otherwise `SplitPane.svelte` (which renders leaves reactively) will try to resolve pane ids that don't exist yet and log missing-instance errors. Instance creation happens in step 7d, tree apply happens in 7e. Terminal init and PTY listener attachment (7f) run after the tree apply has triggered the reactive render — at that point each pane's DOM container is mounted and `attachToContainer` can be called.

**Why preserve the original pane id:** the saved layout tree references pane ids. Generating new ids at rehydration time would require rewriting the tree. Keep the ids stable, generate fresh PTY ids only.

**`rehydratePane` signature:**

```ts
async function rehydratePane(
  descriptor: PaneDescriptor,
  sessionWorktreePath: string,
): Promise<{ paneId: string; error?: string }>;
```

Logic per descriptor type:

- `"claude"` → skip (main pane already exists from startup).
- `"shell"` → generate fresh `ptyId` via `crypto.randomUUID()`, call `spawnShell(ptyId, descriptor.workingDir ?? sessionWorktreePath)`. On success: create `PaneInstance` with the descriptor's original id, fresh ptyId, copied metadata. On failure: create the instance anyway with `restoreError` set to the error message.
- `"markdown"` → create instance directly, preserve `docPath`.
- `"command"` → never reached.

**Startup flow unchanged:** `App.svelte` still creates only the main pane per session on launch. The session shows disconnected. All restore work happens on reconnect click.

### Dead-pane UI (failed restore)

**Data model change — `src/lib/panes/instances.ts`:**

Add one optional field to `PaneInstance`:

```ts
restoreError?: string;  // populated when shell spawn failed during restore
```

When set, the pane is in "dead" state. When undefined, the pane is live (current behavior). `restoreError` is runtime-only — it is **not** on `PaneDescriptor` and is never persisted. Next app launch will attempt a fresh restore from the last known good descriptor.

**Rendering — `src/lib/components/PaneShell.svelte`:**

Add a check that preempts the normal type-based render:

```svelte
{#if instance.restoreError}
  <DeadPaneView
    error={instance.restoreError}
    workingDir={instance.workingDir}
    onRetry={() => retryShellPane(instance.id)}
    onClose={() => closePane(sessionId, instance.id)}
  />
{:else}
  <!-- existing switch on instance.type -->
{/if}
```

**New component — `src/lib/components/DeadPaneView.svelte`:**

Simple static layout, no xterm, no PTY:

- Heading: "Shell failed to restore"
- Working directory (muted label + path)
- Error message (muted label + message)
- Retry button
- Close pane button

Tailwind styling matching existing Roux panels. No new design language.

**Retry logic — new export in `src/lib/sessions/reconnect.ts`:**

```ts
export async function retryShellPane(paneId: string): Promise<void>;
```

Behavior:

1. Read the instance. Bail if not a shell or if `restoreError` is unset.
2. Generate fresh `ptyId`.
3. Call `spawnShell(ptyId, instance.workingDir)`.
4. On success: `updateInstance(paneId, { ptyId, restoreError: undefined })`, then `initTerminal(paneId)` and `attachPtyListeners(paneId)`.
5. On failure: `updateInstance(paneId, { restoreError: newErrorMessage })`. DeadPaneView stays visible with updated error.

**Close behavior:** the Close button calls existing `closePane(sessionId, paneId)` — same path as Cmd+W. No PTY to kill, no special handling.

**Layout implications:** dead panes still occupy leaves in the layout tree. A user with two failed shells sees two dead-pane placeholders in their normal slots. This is intentional — the layout is preserved, the user can retry, and they can close dead panes if they give up. Auto-collapsing would silently reshape the user's workspace.

## Lifecycle Hooks

**Quit flush:** `App.svelte` `close-requested` and `quit-requested` handlers (lines 185–187) add `await flushPaneState()` as the first line. Errors logged and ignored — a failed save must not block quit.

**Session deletion:** the Rust code path that removes a session calls `delete_pane_state(session_id)`. Best-effort; logged on failure.

**Startup localStorage cleanup:** one-shot `localStorage.removeItem` for `roux:pane-layouts-v2` and `roux:pane-descriptors` at the top of `App.svelte` `onMount`. Idempotent; harmless after first launch.

## Testing Plan

### Rust unit tests — `src-tauri/src/pane_state.rs`

- Round-trip: `save_pane_state` → `load_pane_state` returns identical payload.
- Missing file: `load_pane_state` for unknown session → `None`.
- Wrong version: file with `version: 999` → `load_pane_state` returns `None` **and logs the mismatch**.
- Corrupt JSON: `load_pane_state` returns `None` **and logs the parse error**.
- Atomic write: bad tmp file leaves target untouched.
- Delete: `delete_pane_state` on missing file → no error.
- Directory autocreation: first save on a fresh config dir creates `pane_state/` directory.

### Frontend unit tests — `src/lib/panes/__tests__/persistence.test.ts`

- `initPersistence` subscribes to `sessionLayouts` and debounces writes.
- Debounce: 3 rapid changes within 1500ms → one `savePaneState` call.
- `flushPaneState`: pending timer → immediate write → timer cleared.
- `loadPaneState` returns null → no throw.
- Mock Tauri command wrappers; no real disk I/O.

### Frontend reconnect tests — `src/lib/sessions/__tests__/reconnect.test.ts`

- No persisted state → main-pane-only fallback path.
- Persisted state with only main leaf → fast-path, no rehydration work.
- Persisted state with 2 shells → both spawned, both instances created, layout applied.
- Shell spawn fails → instance created with `restoreError` set, layout still applied.
- Command pane in persisted tree → stripped before rehydration.
- Double-click reconnect → second call bails on the `reconnecting` flag.
- **Mid-session disconnect** → current layout already has splits → rehydration skipped, main-pane-only reconnect runs.
- **Corrupt persisted state** (leaf references missing descriptor, duplicate ids, missing main claude, invalid descriptor type) → integrity preflight fails → logged, main-pane-only fallback runs, disk state is not applied.
- **Ordering** — assert `initTerminal` is called before `attachPtyListeners` for each rehydrated pane.
- Mock `spawnShell` and `loadPaneState` to control success/failure.

### Frontend instance tests — extend `src/lib/panes/__tests__/instances.test.ts`

- `restoreError` field round-trips through `createPane` and `updateInstance`.

### Component test — `DeadPaneView.svelte`

- Renders error message and working dir.
- Retry button calls retry callback.
- Close button calls close callback.

### Manual verification (Rule 0 — stated in implementation plan, not inferred)

- `task dev`, split into two shells, close Roux, reopen → click reconnect → splits return, shells are live at their original `cwd`.
- Same flow, but delete a worktree between close and reopen → reconnect → dead pane shows the real filesystem error → fix the path externally → click Retry → pane comes to life.
- Same flow with a command pane in the tree → command pane is stripped on reconnect, remaining panes restore cleanly.
- Stacked-tab state round-trips (stack two shells, save, close, reopen, reconnect → tabs still stacked with the same active index).

## Risks

1. **Subscription fires during rehydration.** Applying the restored tree via `sessionLayouts.update(...)` triggers the auto-save subscription, which schedules a redundant save. Harmless but wasteful. Acceptable — debounce absorbs it.
2. **Reconnect race.** Rapid double-click could double-spawn shells. Mitigated by the per-session `reconnecting` flag set at reconnect entry, cleared in `finally`.
3. **Mid-session disconnect scenario.** Claude CLI crashes while the user has splits open → clicking reconnect should respawn Claude but leave the existing pane tree alone. Mitigated by step 2 of the reconnect flow: rehydrate _only_ when the current layout is a single main-only leaf. Any other runtime state is trusted over disk.
4. **Stale/corrupt persisted state.** A partial write from a crashed old version, or a file tampered with externally, could reference missing pane ids or have mismatched descriptors. Mitigated by the integrity preflight in step 5; invalid state is logged and ignored rather than applied.
5. **PaneShell component contract.** Currently assumes `instance.terminal` exists after mount for shell panes. The `restoreError` branch short-circuits before that dereference, but a full audit of `PaneShell.svelte` during implementation is required to confirm nothing downstream derefs `instance.terminal` unconditionally.
6. **Atomic write on macOS.** `rename(2)` over an existing file is atomic on APFS, but only within the same filesystem. The tmp file must be created in the same directory as the target (not in `/tmp`). Standard practice; just flagging.

## Open Questions

None at design time. All architectural decisions made and confirmed.

## Deliberately Deferred

- Scrollback restore (needs terminal buffer capture at save time, a bounded storage budget, and a replay mechanism on restore).
- localStorage migration (nothing worth rescuing).
- Auto-retry of failed shell spawns.
- Session-level restore-failure UI beyond what already exists for Claude PTY errors.

## Files Touched (forecast)

**New:**

- `src-tauri/src/pane_state.rs`
- `src/lib/components/DeadPaneView.svelte`
- `src/lib/panes/__tests__/persistence.test.ts` (may already exist; extend if so)
- Tests as listed above

**Modified:**

- `src-tauri/src/main.rs` — register new Tauri commands
- `src-tauri/src/services/sessions.rs` — call `delete_pane_state` in `kill_session` after `session_handle.remove(id)`
- `src/lib/tauri.ts` — wrappers for the three new Tauri commands
- `src/lib/panes/persistence.ts` — replace localStorage with Tauri-backed save/load
- `src/lib/panes/instances.ts` — add `restoreError` field
- `src/lib/sessions/reconnect.ts` — extend reconnect + new `retryShellPane`
- `src/lib/components/PaneShell.svelte` — render DeadPaneView branch
- `src/App.svelte` — localStorage cleanup, remove the discard block, flush on quit
