# Reconnect Claude In Place

## Problem

When the app restores sessions after a restart, Claude terminal panes show a SessionPicker ("Resume or start new") because the PTY process is dead. When the user clicks "New Session" or "Resume", the current code destroys the entire Roux session (pane tree, shell PTYs, layout) and creates a new one from scratch with a new ID. This wipes all splits, stacked tabs, and shell terminals.

## Solution

Keep the same Roux session ID and pane tree. Kill only the Claude PTY and spawn a new one under the same session ID. The layout, shell panes, and all other state remain untouched.

Additionally, add a prominent "Continue" option to the SessionPicker that uses Claude CLI's `--continue` flag to resume the most recent conversation without requiring a session ID selection.

## Changes

### Rust: New `reconnect_session` command (`src-tauri/src/main.rs`)

A new Tauri command that reuses an existing session ID:

1. Looks up the existing session in `SessionStore` to get its metadata (working directory, repo root, etc.)
2. Kills the existing PTY for that session ID via `pty_manager.kill()` (no-op if already dead)
3. Spawns a new Claude PTY under the **same** session ID via `pty_manager.spawn()`
4. Updates the session's status to `"idle"` in `SessionStore`
5. Returns the updated `Session`

Parameters:
- `id: String` — the existing session ID
- `extra_flags: Option<Vec<String>>` — optional flags like `["--resume", "<claude-session-id>"]` or `["--continue"]`

No worktree creation logic needed — the session already has its working directory. Settings (model, additional flags, claude binary path, nono profile) are read fresh from current settings at reconnect time.

### Rust: `SessionStore` additions (`src-tauri/src/session.rs`)

- Add `pub fn get(&self, id: &str) -> Option<Session>` to look up a session by ID
- Add `pub fn update_status(&self, id: &str, status: &str)` to update a session's status in place

### Frontend: New `reconnectSession` Tauri binding (`src/lib/tauri.ts`)

```typescript
export async function reconnectSession(
  id: string,
  extraFlags?: string[],
): Promise<Session> {
  return invoke("reconnect_session", { id, extraFlags: extraFlags ?? null });
}
```

### Frontend: Rewrite `handleNew` / `handleResume` in `Terminal.svelte`

Replace the destroy-and-recreate flow. Both handlers now:

1. Dispose the old xterm terminal instance (fresh buffer for the new process)
2. Call `reconnectSession(sessionId, flags)` where flags vary:
   - `handleNew()`: no extra flags
   - `handleResume(claudeSessionId)`: `["--resume", claudeSessionId]`
   - `handleContinue()`: `["--continue"]`
3. Update the session in the Svelte store (status back to `"idle"`)
4. Re-attach PTY output and exit listeners
5. Re-attach the xterm terminal to the DOM

The pane tree is never touched.

### Frontend: Add "Continue" to `SessionPicker.svelte`

Update the `SessionPicker` props to accept a new callback:

```typescript
interface Props {
  cwd: string;
  onContinue: () => void;  // new
  onResume: (claudeSessionId: string) => void;
  onNew: () => void;
}
```

Layout changes to the picker:
- **Continue last session** — prominent button at top (uses `--continue`). Only shown when there is at least one existing Claude session for this cwd.
- **New Session** — secondary button below
- **Previous sessions list** — existing resume list below that

### Frontend: `kill_session` Tauri command change

Currently `kill_session` in `main.rs` both kills the PTY *and* removes the session from the store. The `reconnect_session` command handles its own PTY kill, so `kill_session` behavior doesn't need to change. However, `Terminal.svelte` will no longer call `kill_session` during reconnect — it calls `reconnectSession` which handles everything server-side.

## What stays the same

- Sessions restore as `"disconnected"` on app start (correct — PTY is dead)
- SessionPicker shows on restore so user can choose what to do
- Shell panes get fresh PTYs during `initSessionPanes` on restore (existing behavior)
- Layout persistence in localStorage works because session ID never changes
- `closeAuxiliaryPanes`, `removeSessionPanes`, `removeSession` are no longer called during reconnect

## Flow after fix

1. App starts, restores sessions as disconnected
2. Shell panes get fresh PTYs (existing behavior)
3. Claude panes show SessionPicker
4. User clicks "Continue", "Resume <session>", or "New Session"
5. Frontend calls `reconnectSession(sessionId, flags)`
6. Rust kills old PTY (if any), spawns new Claude PTY under same ID, updates status
7. Frontend disposes old xterm, creates fresh one, attaches to PTY output
8. All splits, shells, tabs remain untouched
