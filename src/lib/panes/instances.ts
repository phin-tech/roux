import { writable, get } from "svelte/store";
import type { SpawnProfileRef } from "./profiles";
import type { Provider } from "./profiles";
import { clearPtyOutputBuffer } from "./ptyOutputBus";
import {
  clearPaneOutputChannel,
  disposePaneTerminalRuntime,
} from "./terminalRuntime";
import {
  upsertPaneRecord,
  removePaneRecord,
  type PaneRecordPayload,
  type NotesScope,
} from "$lib/tauri";

export type PaneType = "shell" | "markdown" | "command" | "notes";

export type CommandStatus = "idle" | "running" | "success" | "error";

export type TerminalState =
  | { kind: "attached"; ptyId: string }
  | { kind: "empty" }
  | { kind: "dead"; ptyId: string; exitCode: number | null };

export interface PaneInstance {
  id: string;
  type: PaneType;
  ptyId: string;

  // Explicit terminal state — when present, takes precedence over ptyId for
  // determining whether a PTY is attached. Absent on panes created before
  // Phase 3 migration; those fall back to ptyId.
  terminalState?: TerminalState;

  // Role for the primary session shell pane (ptyId === sessionId).
  role?: "session-primary";

  // Cleanup hooks
  unlisteners: Array<() => void>;

  // Optional metadata
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;

  /**
   * Optional pointer to the spawn profile this pane was launched from.
   * `registered` refs are re-resolved from the profile registry at render /
   * re-run time; `inline` refs carry the entire profile so ad-hoc "Custom…"
   * panes survive restore without depending on settings state. Absent for
   * plain shell panes launched without a profile.
   */
  spawnProfileRef?: SpawnProfileRef;
  provider?: Provider;
  providerSessionId?: string;

  // Set when a shell pane failed to spawn during session restore.
  // Causes PaneShell to render the DeadPaneView instead of xterm.
  // Not persisted — only lives in runtime state.
  restoreError?: string;

  // Command-pane runtime state
  commandStatus?: CommandStatus;
  commandExitCode?: number | null;
  commandStartedAt?: number | null;
  elapsedTimer?: ReturnType<typeof setInterval> | null;

  // Notes-pane state
  notesScope?: NotesScope;
  notesViewMode?: "edit" | "read";

  // Session this pane belongs to — used by the backend's list_by_session
  // query so --pane-type resolution works for panes with random UUIDs.
  sessionId?: string;
}

export interface CreatePaneOpts {
  id?: string;
  type: PaneType;
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
  spawnProfileRef?: SpawnProfileRef;
  provider?: Provider;
  providerSessionId?: string;
  notesScope?: NotesScope;
  notesViewMode?: "edit" | "read";
  sessionId?: string;
}

// ── Store ──────────────────────────────────────────────────

export const paneInstances = writable<Map<string, PaneInstance>>(new Map());

// ── Helpers ────────────────────────────────────────────────

function generateId(): string {
  return `pane-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function toPaneRecord(instance: PaneInstance): PaneRecordPayload {
  return {
    id: instance.id,
    type: instance.type,
    ptyId: instance.ptyId,
    name: instance.name,
    workingDir: instance.workingDir,
    command: instance.command,
    docPath: instance.docPath,
    spawnProfileRef: instance.spawnProfileRef,
    provider: instance.provider,
    providerSessionId: instance.providerSessionId,
    notesScope: instance.notesScope,
    notesViewMode: instance.notesViewMode,
    sessionId: instance.sessionId,
  };
}

function syncPaneRecord(instance: PaneInstance): void {
  void upsertPaneRecord(toPaneRecord(instance)).catch(() => {});
}

// Field-level diff against PaneInstance — replaces a prior double
// JSON.stringify, which was O(record-size) on every pane mutation
// (terminal output, status changes) and showed up as a beach-ball under
// fast PTY traffic.
function paneRecordChanged(before: PaneInstance, after: PaneInstance): boolean {
  return (
    before.id !== after.id ||
    before.type !== after.type ||
    before.ptyId !== after.ptyId ||
    before.name !== after.name ||
    before.workingDir !== after.workingDir ||
    before.command !== after.command ||
    before.docPath !== after.docPath ||
    before.spawnProfileRef !== after.spawnProfileRef ||
    before.provider !== after.provider ||
    before.providerSessionId !== after.providerSessionId ||
    before.notesScope !== after.notesScope ||
    before.notesViewMode !== after.notesViewMode ||
    before.sessionId !== after.sessionId
  );
}

/**
 * Return the PTY ID that is currently attached to this pane, or null if the
 * pane is empty or dead. Uses `terminalState` when present; falls back to the
 * legacy `ptyId` field for panes created before the Phase 3 migration.
 */
export function getAttachedPtyId(pane: PaneInstance): string | null {
  if (pane.terminalState) {
    return pane.terminalState.kind === "attached"
      ? pane.terminalState.ptyId
      : null;
  }
  return pane.ptyId || null;
}

// ── Public API ─────────────────────────────────────────────

/**
 * Create a new pane instance and add it to the store.
 * Terminal creation is deferred — terminal is kept null until
 * attachToContainer is called.
 * Returns the pane id.
 */
export function createPane(opts: CreatePaneOpts): string {
  const id = opts.id ?? generateId();
  const instance: PaneInstance = {
    id,
    type: opts.type,
    ptyId: opts.ptyId,
    unlisteners: [],
    name: opts.name,
    workingDir: opts.workingDir,
    command: opts.command,
    docPath: opts.docPath,
    spawnProfileRef: opts.spawnProfileRef,
    provider: opts.provider,
    providerSessionId: opts.providerSessionId,
    commandStatus: "idle",
    commandExitCode: null,
    commandStartedAt: null,
    elapsedTimer: null,
    notesScope: opts.notesScope,
    notesViewMode: opts.notesViewMode,
    sessionId: opts.sessionId,
  };
  paneInstances.update((map) => {
    const next = new Map(map);
    next.set(id, instance);
    return next;
  });
  syncPaneRecord(instance);
  return id;
}

/**
 * Extra cleanup hooks run after a pane instance is removed. Actions.ts
 * registers `disposeAgentState` here at startup so that every disposal
 * path — `closePane`, `closeSessionPanes`, splitPane rollback, and any
 * future caller — clears runtime agent state for the pane without
 * instances.ts needing to import agentState.ts (which would create an
 * instances → agentState → layout → instances cycle).
 */
const postDisposeHooks: Array<(paneId: string) => void> = [];

export function registerDisposeHook(hook: (paneId: string) => void): void {
  if (!postDisposeHooks.includes(hook)) postDisposeHooks.push(hook);
}

/** Test-only: drop registered dispose hooks so Vitest runs stay isolated. */
export function resetDisposeHooks(): void {
  postDisposeHooks.length = 0;
}

/**
 * Dispose a pane instance — idempotent.
 * Cleans up unlisteners, clears elapsed timer, disposes terminal runtime, kills PTY
 * for shell/command types, removes from the store, and runs any registered
 * post-dispose hooks (e.g. agent-state cleanup).
 */
export function disposePane(
  id: string,
  killPty?: (ptyId: string) => Promise<void>,
): void {
  const map = get(paneInstances);
  const inst = map.get(id);
  if (!inst) return;

  // Kill the underlying PTY for any pane that hosts one. Markdown panes
  // carry an empty ptyId so the killer is skipped implicitly.
  // Note: agents now live inside the shell PTY, so killing the pane kills
  // the agent — there is no separate session-level PTY to preserve.
  if (
    killPty &&
    inst.ptyId &&
    (inst.type === "shell" || inst.type === "command")
  ) {
    killPty(inst.ptyId).catch(() => {
      /* best-effort */
    });
  }

  // Run all cleanup listeners
  for (const unlisten of inst.unlisteners.splice(0)) {
    try {
      unlisten();
    } catch {
      /* best-effort */
    }
  }

  // Clear command elapsed timer
  if (inst.elapsedTimer != null) {
    clearInterval(inst.elapsedTimer);
    inst.elapsedTimer = null;
  }

  disposePaneTerminalRuntime(id);
  void removePaneRecord(id).catch(() => {});

  paneInstances.update((m) => {
    const next = new Map(m);
    next.delete(id);
    return next;
  });

  for (const hook of postDisposeHooks) {
    try {
      hook(id);
    } catch {
      /* best-effort */
    }
  }
}

/**
 * Tear down old PTY listeners on a pane and update its ptyId.
 * The caller is responsible for attaching new listeners.
 */
export function replacePty(paneId: string, newPtyId: string): void {
  paneInstances.update((map) => {
    const inst = map.get(paneId);
    if (!inst) return map;

    // Clean up old listeners
    for (const unlisten of inst.unlisteners.splice(0)) {
      try {
        unlisten();
      } catch {
        /* best-effort */
      }
    }
    clearPaneOutputChannel(paneId);

    // Drop the PTY output replay buffer so a subsequent readiness-wait
    // on the fresh PTY doesn't see stale bytes from the prior process
    // (reconnect/rerun may reuse the same id).
    clearPtyOutputBuffer(inst.ptyId);
    if (inst.ptyId !== newPtyId) clearPtyOutputBuffer(newPtyId);

    const next = new Map(map);
    const updated = { ...inst, ptyId: newPtyId, unlisteners: [] };
    next.set(paneId, updated);
    syncPaneRecord(updated);
    return next;
  });
}

/**
 * Partial update for metadata / status fields.
 */
export function updateInstance(
  paneId: string,
  fields: Partial<Omit<PaneInstance, "id">>,
): void {
  paneInstances.update((map) => {
    const inst = map.get(paneId);
    if (!inst) return map;
    const next = new Map(map);
    const updated = { ...inst, ...fields };
    next.set(paneId, updated);
    if (paneRecordChanged(inst, updated)) {
      syncPaneRecord(updated);
    }
    return next;
  });
}

/**
 * Read a single instance from the store synchronously.
 */
export function getInstance(paneId: string): PaneInstance | undefined {
  return get(paneInstances).get(paneId);
}

/**
 * Find the pane that currently has a PTY attached. Returns null if no pane
 * has this PTY attached. Uses `terminalState` when present; falls back to
 * `ptyId` for legacy panes.
 */
export function findPaneByPtyId(ptyId: string): PaneInstance | null {
  const map = get(paneInstances);
  for (const pane of map.values()) {
    if (getAttachedPtyId(pane) === ptyId) {
      return pane;
    }
  }
  return null;
}

/**
 * Reset the store to an empty map — only intended for tests.
 */
export function resetInstances(): void {
  // Dispose all existing instances cleanly
  const map = get(paneInstances);
  for (const id of map.keys()) {
    disposePane(id);
  }
  paneInstances.set(new Map());
}
