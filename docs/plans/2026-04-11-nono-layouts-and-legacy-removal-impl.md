# Nono in Layouts + Remove Legacy Claude Path — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify all session creation/reconnect onto the shell-spawn path, add nono sandbox support to `spawn_shell` so it works for any profile and layout, and delete the legacy Claude-specific spawn path.

**Architecture:** Every pane spawns a shell (optionally nono-wrapped), then `runProfileInPane` types commands into it. Nono config comes from layout KDL leaves, `SpawnProfile` fields, or the dialog dropdown — resolved in that priority order. Continue/Resume/New for Claude reconnects works by appending flags to the startup command typed into the shell, not by passing them to the PTY spawn. Persistence gains nono fields so wrapping survives restart.

**Tech Stack:** Rust (portable-pty, specta, serde, kdl), Svelte 5, Tauri 2, existing xterm/PTY stack.

**Spec:** `docs/plans/2026-04-11-nono-layouts-and-legacy-removal.md`

---

## Phase 1: Backend — NonoConfig + nono on spawn_shell

Add `NonoConfig` struct and optional nono parameter to `PtyManager::spawn_shell`. Purely additive.

**Files:**
- Modify: `src-tauri/src/pty.rs`
- Modify: `src-tauri/src/services/sessions.rs` (fix callers to pass `None`)
- Modify: `src-tauri/src/commands/sessions.rs` (fix callers to pass `None`)

**Steps:** Add NonoConfig with path normalization (~/ expansion, relative resolution), add nono param to spawn_shell, fix all callers to pass None, add unit tests for path resolution. See full spec for NonoConfig struct and spawn_shell implementation.

---

## Phase 2: Backend — Nono on create_session_shell + reconnect_session_shell + spawn_shell command

Add optional nono params to the Tauri commands. Additive — existing frontend callers pass undefined.

**Files:**
- Modify: `src-tauri/src/services/sessions.rs`
- Modify: `src-tauri/src/commands/sessions.rs`

**Steps:** Add nono_profile + nono_allow_dirs to create_session_shell, reconnect_session_shell, and spawn_shell Tauri commands. Thread through to spawn_shell backend.

---

## Phase 3: Nono on SpawnProfile + regenerate bindings

**Files:**
- Modify: `crates/roux-core/src/models/profile.rs`
- Regenerate: `src/lib/bindings.ts`
- Modify: `src/lib/tauri.ts`

**Steps:** Add nono_profile and nono_allow_dirs to SpawnProfile. Regenerate bindings. Update frontend spawnShell/createSessionShell/reconnectSessionShell wrappers with optional nono params.

---

## Phase 4: Parser — nono attributes on layout KDL leaves

**Files:**
- Modify: `crates/roux-core/src/models/layout.rs`

**Steps:** Add nono_profile and nono_allow_dirs to LayoutPaneNode::Leaf. Parse `nono` attribute and `nono_flags { allow_dir "..." }` child block. Implement body disambiguation (registered profile + nono_flags is OK, registered + inline fields is rejected). TDD with ~5 new tests.

---

## Phase 5: Persistence — nono on PaneDescriptor

**Files:**
- Modify: `src/lib/panes/persistence.ts`
- Modify: `src/lib/panes/instances.ts`

**Steps:** Add nonoProfile and nonoAllowDirs to PaneDescriptor and PaneInstance/CreatePaneOpts. Bump schema version 3→4.

---

## Phase 6: Walker — thread nono through layout runner

**Files:**
- Modify: `src/lib/panes/layoutRunner.ts`
- Modify: `src/lib/panes/__tests__/layoutRunner.test.ts`

**Steps:** Update LeafInfo with nono fields. Implement nono resolution (layout leaf > profile > none). Pass nono to spawnShell calls. Store nono on pane instances. Add 4 tests (nono from leaf, from profile, leaf overrides profile, allow_dirs merge).

---

## Phase 7: Migrate all createSession callers

Replace every `createSession` call with `createSessionShell` + `initSessionWithProfile` + `runProfileInPane`.

**Files:**
- Modify: `src/lib/components/NewSessionDialog.svelte`
- Modify: `src/lib/components/SessionTabs.svelte`
- Modify: `src/lib/commands/sessions.ts`
- Modify: `src/App.svelte` (socket handler)
- Modify: `src-tauri/src/socket.rs`

**Steps:**
1. NewSessionDialog: delete useLegacyClaudePath branch, all sessions use createSessionShell. Delete skip-permissions checkbox. Ungate nono dropdown to `nonoInstalled && !selectedLayout`. Thread nono from dialog.
2. SessionTabs: switch worktree creation to createSessionShell + profile replay.
3. commands/sessions.ts: same migration for session.new-worktree command.
4. socket.rs: switch to svc::create_session_shell.
5. App.svelte socket handler: add runProfileInPane after terminal init.

---

## Phase 8: Migrate all reconnectSession callers

Replace every `reconnectSession` call with `reconnectSessionShell` + flag appending.

**Files:**
- Modify: `src/lib/sessions/reconnect.ts`
- Modify: `src/lib/components/PaneShell.svelte`
- Modify: `src/lib/commands/sessions.ts`

**Steps:**
1. reconnect.ts: update reconnectSessionShell to accept optional extraStartupFlags, append them to startup command before runProfileInPane. Pass nono from persisted pane state.
2. PaneShell.svelte: change Claude reconnect handlers to use reconnectSessionShell with extra flags. Keep SessionPicker UI.
3. commands/sessions.ts: switch session.reconnect to reconnectSessionShell.
4. reconnect.ts full restore: switch reconnectPrimaryPaneOnly to use shell path + profile replay.
5. rehydratePane: pass nono from persisted descriptor to spawnShell.

---

## Phase 9: Thread nono through remaining spawnShell callers

**Files:**
- Modify: `src/lib/commands/panes.ts`
- Modify: `src/lib/sessions/reconnect.ts` (retryShellPane)

**Steps:** Split-pane commands read nono from SpawnProfile and pass to spawnShell. retryShellPane reads nono from PaneInstance.

---

## Phase 10: Delete legacy backend

**Files:**
- Modify: `src-tauri/src/pty.rs` — delete PtyManager::spawn() + resolve_claude_command()
- Modify: `src-tauri/src/services/sessions.rs` — delete svc::create_session + svc::reconnect_session
- Modify: `src-tauri/src/commands/sessions.rs` — delete Tauri commands
- Modify: `src-tauri/src/main.rs` — remove from collect_commands! + generate_handler!
- Regenerate: `src/lib/bindings.ts`
- Modify: `src/lib/tauri.ts` — delete dead wrappers

---

## Phase 11: Docs + polish

**Files:**
- Modify: `docs/features/layouts.md`

**Steps:** Add nono section to layout docs. Remove "Claude panes inside layouts are not nono-wrapped" from limitations. Note that --dangerously-skip-permissions is a profile concern. Run full test suite.
