# Reconnect Claude In Place — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a user clicks "Continue", "Resume", or "New Session" on a restored session, swap only the Claude PTY in place — preserving the entire pane tree, shell terminals, and layout.

**Architecture:** Add a Rust `reconnect_session` command that kills the old PTY and spawns a new Claude process under the same session ID. The frontend `reconnectSession` function calls this instead of destroying and recreating the session. The SessionPicker gets a new "Continue" button that uses `--continue`.

**Tech Stack:** Rust/Tauri 2, Svelte 5, TypeScript, Vitest

**Spec:** `docs/superpowers/specs/2026-04-06-reconnect-claude-in-place-design.md`

---

### Task 1: Add `get` and `update_status` to `SessionStore`

**Files:**
- Modify: `src-tauri/src/session.rs`

- [ ] **Step 1: Add `get` method**

In `src-tauri/src/session.rs`, add this method to the `impl SessionStore` block, after the existing `remove` method (after line 77):

```rust
pub fn get(&self, id: &str) -> Option<Session> {
    self.sessions.lock().unwrap().iter().find(|s| s.id == id).cloned()
}
```

- [ ] **Step 2: Add `update_status` method**

Add this method right after the `get` method:

```rust
pub fn update_status(&self, id: &str, status: &str) {
    let mut sessions = self.sessions.lock().unwrap();
    if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
        s.status = status.to_string();
    }
    self.dirty.store(true, std::sync::atomic::Ordering::Release);
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles with no errors (warnings OK).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/session.rs
git commit -m "feat: add get and update_status to SessionStore"
```

---

### Task 2: Add `reconnect_session` Tauri command

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Add the `reconnect_session` command function**

Add this function in `src-tauri/src/main.rs`, after the existing `create_session` function (after line 233):

```rust
#[tauri::command]
fn reconnect_session(
    id: String,
    extra_flags: Option<Vec<String>>,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let session = state.session_store.get(&id)
        .ok_or_else(|| format!("Session {} not found", id))?;

    let settings = state.settings.lock().unwrap().clone();

    // Kill existing PTY (ignore errors — it may already be dead)
    let _ = state.pty_manager.kill(&id);

    // Merge settings flags with per-call extra flags
    let mut all_flags = settings.additional_flags.clone();
    if let Some(ef) = extra_flags {
        all_flags.extend(ef);
    }

    rlog!("Reconnecting session '{}' (id={}) in '{}'", session.name, id, session.worktree_path);

    // Spawn new Claude PTY under the same session ID
    state.pty_manager.spawn(
        &id,
        &session.worktree_path,
        settings.default_model.as_deref(),
        &all_flags,
        None,
        settings.claude_binary_path.as_deref(),
        app.clone(),
    )?;

    // Update status to idle
    state.session_store.update_status(&id, "idle");

    rlog!("Session '{}' reconnected successfully", id);

    // Return the session with updated status
    let mut updated = session;
    updated.status = "idle".to_string();
    Ok(updated)
}
```

- [ ] **Step 2: Register the command in the invoke handler**

In `src-tauri/src/main.rs`, add `reconnect_session` to the `invoke_handler` list. Find the line `kill_session,` (around line 482) and add after it:

```rust
reconnect_session,
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat: add reconnect_session Tauri command"
```

---

### Task 3: Add `reconnectSession` Tauri binding on the frontend

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add the binding**

In `src/lib/tauri.ts`, add this function after the existing `killSession` function (after line 33):

```typescript
export async function reconnectSessionPty(
  id: string,
  extraFlags?: string[],
): Promise<Session> {
  return invoke("reconnect_session", { id, extraFlags: extraFlags ?? null });
}
```

Note: Named `reconnectSessionPty` to avoid colliding with the existing `reconnectSession` function in `src/lib/sessions/reconnect.ts` which will be updated in the next task.

- [ ] **Step 2: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat: add reconnectSessionPty Tauri binding"
```

---

### Task 4: Add `transferSessionPanes` to panes store

**Files:**
- Modify: `src/lib/stores/panes.ts`
- Test: `src/lib/stores/__tests__/panes.test.ts`

We need a function that moves a pane tree from one session ID to another and updates the Claude pane's `ptyId` to match the new session ID. This is needed because the `reconnect_session` Rust command keeps the same session ID, but the `disposeClaudeTerminal` + `ensureClaudeTerminal` cycle in the terminal registry is keyed by session ID. Actually — since we're keeping the same session ID, we don't need a transfer. The pane tree stays under the same key.

However, looking at the existing `reconnectSession` in `reconnect.ts`, it's also called from the command palette's "Reconnect Session" command. We need to make sure the command palette path also works.

Since the session ID stays the same, the pane tree doesn't need to move. Skip this task — no `transferSessionPanes` needed.

---

### Task 4: Rewrite `reconnectSession` to reconnect in place

**Files:**
- Modify: `src/lib/sessions/reconnect.ts`
- Modify: `src/lib/sessions/__tests__/reconnect.test.ts`

- [ ] **Step 1: Write the updated test**

Replace the contents of `src/lib/sessions/__tests__/reconnect.test.ts`:

```typescript
import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  reconnectSessionPty: vi.fn(),
  killSession: vi.fn(),
}));

vi.mock("$lib/panes/terminalRegistry", () => ({
  disposeClaudeTerminal: vi.fn(),
}));

import { reconnectSession } from "../reconnect";
import { sessionState, addSession } from "$lib/stores/sessions";
import { initSessionPanes, paneTrees } from "$lib/stores/panes";
import { reconnectSessionPty } from "$lib/tauri";
import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
import type { Session } from "$lib/types";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "sess-1",
    name: "Repo",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "disconnected",
    model: null,
    cost: null,
    permissionInfo: null,
    createdAt: 1,
    ...overrides,
  };
}

describe("reconnectSession", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    paneTrees.set(new Map());
    vi.mocked(reconnectSessionPty).mockReset();
    vi.mocked(disposeClaudeTerminal).mockReset();
  });

  it("preserves the pane tree when reconnecting", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    // Add a shell pane to simulate a split layout
    const trees = get(paneTrees);
    const tree = trees.get(session.id)!;
    expect(tree).toBeDefined();

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session);

    // Pane tree should still exist under the same session ID
    const afterTrees = get(paneTrees);
    expect(afterTrees.has(session.id)).toBe(true);

    // Session should be updated to idle
    const state = get(sessionState);
    expect(state.sessions.find((s) => s.id === session.id)?.status).toBe("idle");
  });

  it("disposes the claude terminal before reconnecting", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session);

    expect(disposeClaudeTerminal).toHaveBeenCalledWith(session.id);
    expect(vi.mocked(disposeClaudeTerminal).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(reconnectSessionPty).mock.invocationCallOrder[0]
    );
  });

  it("passes extra flags through to the Tauri command", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session, ["--resume", "abc123"]);

    expect(reconnectSessionPty).toHaveBeenCalledWith(session.id, ["--resume", "abc123"]);
  });

  it("passes --continue flag", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session, ["--continue"]);

    expect(reconnectSessionPty).toHaveBeenCalledWith(session.id, ["--continue"]);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && npx vitest run src/lib/sessions/__tests__/reconnect.test.ts`
Expected: FAIL — the current `reconnectSession` signature doesn't accept extra flags and still does destroy-recreate.

- [ ] **Step 3: Rewrite `reconnectSession`**

Replace the contents of `src/lib/sessions/reconnect.ts`:

```typescript
import type { Session } from "$lib/types";
import { updateSessionStatus } from "$lib/stores/sessions";
import { reconnectSessionPty } from "$lib/tauri";
import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
import { log, logError } from "$lib/logging";

export async function reconnectSession(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  log(`Reconnecting session ${session.id} (${session.name})${extraFlags ? ` with flags: ${extraFlags.join(" ")}` : ""}`);

  // Dispose the old xterm terminal so a fresh one is created on re-attach
  await disposeClaudeTerminal(session.id);

  // Call the Rust command that kills old PTY + spawns new one under same ID
  const updated = await reconnectSessionPty(session.id, extraFlags);

  // Update session status in the Svelte store
  updateSessionStatus(session.id, updated.status as Session["status"]);

  log(`Session ${session.id} reconnected`);
  return updated;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && npx vitest run src/lib/sessions/__tests__/reconnect.test.ts`
Expected: All 4 tests PASS.

- [ ] **Step 5: Run the full test suite to check for regressions**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && npm run test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib/sessions/reconnect.ts src/lib/sessions/__tests__/reconnect.test.ts
git commit -m "feat: rewrite reconnectSession to preserve pane tree"
```

---

### Task 5: Rewrite Terminal.svelte handlers

**Files:**
- Modify: `src/lib/components/Terminal.svelte`

- [ ] **Step 1: Replace handleResume and handleNew with reconnect-based handlers**

In `src/lib/components/Terminal.svelte`, replace the `handleResume` function (lines 37-61) and `handleNew` function (lines 63-86) with:

```typescript
  async function handleContinue() {
    if (!session) return;
    log(`Continuing last session for ${sessionId}`);
    await reconnect(["--continue"]);
  }

  async function handleResume(claudeSessionId: string) {
    if (!session) return;
    log(`Resuming claude session ${claudeSessionId} for ${sessionId}`);
    await reconnect(["--resume", claudeSessionId]);
  }

  async function handleNew() {
    if (!session) return;
    log(`Starting new claude session for ${sessionId}`);
    await reconnect();
  }

  async function reconnect(extraFlags?: string[]) {
    if (!session) return;
    try {
      await reconnectSession(session, extraFlags);
      // Re-attach listeners for the new PTY
      await attachListeners();
    } catch (e) {
      logError("Failed to reconnect session", e);
    }
  }
```

- [ ] **Step 2: Update imports**

Replace the imports at the top of the `<script>` block. Remove `createSession` from the tauri import and add `reconnectSession`:

Change line 8 from:
```typescript
  import { onPtyOutput, onSessionExit, writeToSession, resizeSession, createSession } from "$lib/tauri";
```
to:
```typescript
  import { onPtyOutput, onSessionExit, writeToSession, resizeSession } from "$lib/tauri";
```

Change line 9 from:
```typescript
  import { sessionState, setSessionDisconnected, addSession, removeSession } from "$lib/stores/sessions";
```
to:
```typescript
  import { sessionState, setSessionDisconnected } from "$lib/stores/sessions";
```

Remove line 12 (the `initSessionPanes, removeSessionPanes` import):
```typescript
  import { initSessionPanes, removeSessionPanes } from "$lib/stores/panes";
```

Remove line 13 (the `closeAuxiliaryPanes` import):
```typescript
  import { closeAuxiliaryPanes } from "$lib/panes/actions";
```

Add this import after the existing imports:
```typescript
  import { reconnectSession } from "$lib/sessions/reconnect";
```

Also remove the `killSession` import on line 14 since it's no longer used directly:
```typescript
  import { killSession } from "$lib/tauri";
```

- [ ] **Step 3: Pass `onContinue` to SessionPicker**

Update the SessionPicker usage in the template (around line 225-232). Change:

```svelte
    <SessionPicker
      cwd={session.worktreePath}
      onResume={handleResume}
      onNew={handleNew}
    />
```

to:

```svelte
    <SessionPicker
      cwd={session.worktreePath}
      onContinue={handleContinue}
      onResume={handleResume}
      onNew={handleNew}
    />
```

- [ ] **Step 4: Verify TypeScript checks pass**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && npm run check`
Expected: No type errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/Terminal.svelte
git commit -m "feat: Terminal handlers use reconnectSession instead of destroy-recreate"
```

---

### Task 6: Add "Continue" button to SessionPicker

**Files:**
- Modify: `src/lib/components/SessionPicker.svelte`

- [ ] **Step 1: Update the Props interface**

In `src/lib/components/SessionPicker.svelte`, change the Props interface (lines 6-10) to:

```typescript
  interface Props {
    cwd: string;
    onContinue: () => void;
    onResume: (claudeSessionId: string) => void;
    onNew: () => void;
  }
```

- [ ] **Step 2: Update the destructured props**

Change line 12 from:
```typescript
  let { cwd, onResume, onNew }: Props = $props();
```
to:
```typescript
  let { cwd, onContinue, onResume, onNew }: Props = $props();
```

- [ ] **Step 3: Add the Continue button and reorganize the layout**

Replace the template section (lines 42-91) with:

```svelte
<div class="flex h-full w-full flex-col items-center justify-center p-8">
  <div class="w-full max-w-md space-y-4">
    <div class="text-center space-y-1">
      <div class="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl border border-border-subtle bg-bg-surface/80 text-accent shadow-[0_12px_32px_rgba(2,6,23,0.4)]">
        <span class="text-xl">&#9095;</span>
      </div>
      <p class="pt-3 text-sm font-semibold tracking-tight text-text-primary">Resume or start new</p>
      <p class="text-xs text-text-secondary">{cwd.split("/").pop()}</p>
    </div>

    {#if loading}
      <p class="text-center text-xs text-text-muted">Loading sessions...</p>
    {:else}
      {#if sessions.length > 0}
        <button
          class="flex w-full items-center justify-center gap-2 rounded-xl border border-accent-dim/20 bg-accent-dim/15 py-2.5 text-sm text-accent cursor-pointer transition-all hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
          onclick={onContinue}
        >
          Continue last session
        </button>
      {/if}

      <button
        class="flex w-full items-center justify-center gap-2 rounded-xl border border-border-subtle bg-bg-surface/70 py-2.5 text-sm text-text-primary cursor-pointer transition-all hover:border-border hover:bg-bg-surface focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={onNew}
      >
        <span class="text-base">+</span> New Session
      </button>

      {#if sessions.length > 0}
        {#if sessions.length > 5}
          <input
            class="w-full rounded-lg border border-border-subtle bg-bg-surface/80 px-2.5 py-1.5 text-[11px] text-text-primary placeholder:text-text-muted outline-none focus:border-border"
            placeholder="Filter sessions..."
            bind:value={filter}
          />
        {/if}

        <div class="app-scrollbar max-h-64 space-y-1.5 overflow-y-auto">
          {#each filtered as cs (cs.sessionId)}
            <button
              class="group flex w-full items-start gap-3 rounded-xl border border-border-subtle bg-bg-surface/70 px-3 py-2.5 text-left cursor-pointer transition-colors hover:border-border hover:bg-bg-surface focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              onclick={() => onResume(cs.sessionId)}
            >
              <div class="min-w-0 flex-1">
                <div class="truncate text-[12px] font-medium text-text-primary">
                  {cs.summary || "Empty session"}
                </div>
                <div class="mt-0.5 flex items-center gap-2">
                  <span class="font-mono text-[10px] text-text-secondary">{cs.sessionId.slice(0, 8)}</span>
                  <span class="text-[10px] text-text-muted">{timeAgo(cs.modifiedAt)}</span>
                </div>
              </div>
              <span class="shrink-0 pt-1 text-[10px] text-text-secondary opacity-0 transition-opacity group-hover:opacity-100">&#8594;</span>
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>
```

Key changes:
- "Continue last session" is the top prominent button (accent styled), only shown when there are existing sessions
- "New Session" is secondary (neutral styled), always visible
- Session list appears below both buttons

- [ ] **Step 4: Verify TypeScript checks pass**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && npm run check`
Expected: No type errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/SessionPicker.svelte
git commit -m "feat: add Continue button to SessionPicker"
```

---

### Task 7: Update command palette reconnect command

**Files:**
- Modify: `src/lib/commands/index.ts`

- [ ] **Step 1: Update the reconnect command to pass no extra flags**

The `session.reconnect` command in `src/lib/commands/index.ts` (around line 300-308) currently calls `reconnectSession(session)` which now has the correct signature — it passes no extra flags and reconnects in place. No code change needed here.

However, we should remove the now-unused imports. The `reconnectSession` rewrite removed the dependency on `createSession` and `killSession` from the reconnect module, but `index.ts` imports `createSession` from tauri for other uses. Check if all imports are still needed.

Check the file for any unused imports introduced by this change. The `createSession` import from `$lib/tauri` on line 6 is still used by other commands (e.g., session.new). No changes needed.

- [ ] **Step 2: Run the full test suite**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && npm run test`
Expected: All tests pass.

- [ ] **Step 3: Run type checking**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && npm run check`
Expected: No errors.

- [ ] **Step 4: Commit (only if changes were needed)**

If any import cleanups were made:
```bash
git add src/lib/commands/index.ts
git commit -m "chore: clean up imports after reconnect rewrite"
```

---

### Task 8: Manual smoke test

- [ ] **Step 1: Build and run the app**

Run: `cd /Users/sphinizy/src/github.com/phin-tech/roux && task dev`

- [ ] **Step 2: Test session restore with layout preservation**

1. Create a session, add a shell split (Cmd+D), arrange layout
2. Quit and reopen the app
3. Verify: Layout restored, Claude pane shows SessionPicker, shell pane is alive
4. Click "Continue last session"
5. Verify: Claude reconnects, layout is preserved, shell still works

- [ ] **Step 3: Test "Resume" with specific session**

1. From a disconnected session, click a specific session in the list
2. Verify: Claude resumes that session, layout preserved

- [ ] **Step 4: Test "New Session"**

1. From a disconnected session, click "New Session"
2. Verify: Fresh Claude starts, layout preserved

- [ ] **Step 5: Test command palette reconnect**

1. From a disconnected session, open command palette (Cmd+K), type "Reconnect"
2. Verify: Session reconnects, layout preserved
