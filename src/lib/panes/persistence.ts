import { get } from "svelte/store";
import type { LayoutNode } from "./layout";
import { sessionLayouts, collectLeafIds } from "./layout";
import type { PaneType } from "./instances";
import type { NotesScope } from "$lib/tauri";
import type { SpawnProfileRef } from "./profiles";
import type { Provider } from "./profiles";
import {
  loadPaneStateRaw,
  savePaneStateRaw,
  saveLivePaneStateRaw,
  deletePaneStateRaw,
} from "$lib/tauri";
import { log } from "$lib/logging";

/**
 * Pane-state schema version. Bump when descriptors or payload shape change
 * in a way that would confuse the loader. Phase 4 moved off the legacy
 * `claude` pane type and added `spawnProfileRef`, so old payloads are
 * rejected on load and the session restores empty — this is acceptable per
 * the spec's no-backcompat scope rule.
 */
export const PANE_STATE_SCHEMA_VERSION = 4;

export interface PaneDescriptor {
  id: string;
  type: PaneType;
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
  spawnProfileRef?: SpawnProfileRef;
  provider?: Provider;
  providerSessionId?: string;
  nonoProfile?: string;
  nonoAllowDirs?: string[];
  notesScope?: NotesScope;
  notesViewMode?: "edit" | "read";
}

export interface PaneStatePayload {
  schemaVersion: typeof PANE_STATE_SCHEMA_VERSION;
  layout: LayoutNode;
  descriptors: PaneDescriptor[];
}

// ── Public async API ──────────────────────────────────────────────────────────

const VALID_NOTES_SCOPES = new Set(["session", "repo", "project", "global"]);
const VALID_VIEW_MODES = new Set(["edit", "read"]);
// Note: ptyId may be empty for non-shell panes (notes/markdown), so we
// don't enforce a min-length on it. Type narrowing is what matters.
const VALID_PANE_TYPES = new Set<PaneType>(["shell", "markdown", "command", "notes"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isLayoutNode(value: unknown): value is LayoutNode {
  if (!isRecord(value)) return false;
  if (value.kind === "leaf") {
    return typeof value.paneId === "string" && value.paneId.length > 0;
  }
  if (value.kind !== "split") return false;
  if (value.direction !== "h" && value.direction !== "v") return false;
  if (!Array.isArray(value.children) || value.children.length === 0) return false;
  if (!value.children.every(isLayoutNode)) return false;
  if (
    value.sizes !== undefined &&
    value.sizes !== null &&
    (
      !Array.isArray(value.sizes) ||
      value.sizes.length !== value.children.length ||
      value.sizes.some((size) => typeof size !== "number" || !Number.isFinite(size) || size < 0)
    )
  ) {
    return false;
  }
  if (value.stacked !== undefined && typeof value.stacked !== "boolean") return false;
  if (
    value.activeIndex !== undefined &&
    (
      typeof value.activeIndex !== "number" ||
      !Number.isInteger(value.activeIndex) ||
      value.activeIndex < 0 ||
      value.activeIndex >= value.children.length
    )
  ) {
    return false;
  }
  return true;
}

function isDescriptor(value: unknown): value is PaneDescriptor {
  if (!isRecord(value)) return false;
  return (
    typeof value.id === "string" &&
    value.id.length > 0 &&
    typeof value.type === "string" &&
    VALID_PANE_TYPES.has(value.type as PaneType) &&
    typeof value.ptyId === "string"
  );
}

function validateDescriptor(d: PaneDescriptor): PaneDescriptor {
  const result = { ...d };
  // Validate notesScope - default to "session" if invalid
  if (result.notesScope !== undefined && !VALID_NOTES_SCOPES.has(result.notesScope)) {
    log(`validateDescriptor: invalid notesScope "${result.notesScope}", defaulting to "session"`);
    result.notesScope = "session";
  }
  // Validate notesViewMode - default to "edit" if invalid
  if (result.notesViewMode !== undefined && !VALID_VIEW_MODES.has(result.notesViewMode)) {
    log(`validateDescriptor: invalid notesViewMode "${result.notesViewMode}", defaulting to "edit"`);
    result.notesViewMode = "edit";
  }
  return result;
}

export async function loadPaneState(
  sessionId: string,
): Promise<PaneStatePayload | null> {
  try {
    const raw = await loadPaneStateRaw(sessionId);
    if (raw == null) return null;
    if (!isRecord(raw)) {
      log(`loadPaneState(${sessionId}): invalid payload shape — dropping persisted state`);
      return null;
    }
    const payload = raw as { schemaVersion?: number } & Partial<PaneStatePayload>;
    if (payload.schemaVersion !== PANE_STATE_SCHEMA_VERSION) {
      log(
        `loadPaneState(${sessionId}): schema mismatch (got ${
          payload.schemaVersion ?? "missing"
        }, expected ${PANE_STATE_SCHEMA_VERSION}) — dropping persisted state`,
      );
      return null;
    }
    if (!isLayoutNode(payload.layout)) {
      log(`loadPaneState(${sessionId}): invalid layout tree — dropping persisted state`);
      return null;
    }
    if (!Array.isArray(payload.descriptors) || !payload.descriptors.every(isDescriptor)) {
      log(`loadPaneState(${sessionId}): invalid pane descriptors — dropping persisted state`);
      return null;
    }
    // Validate descriptors to ensure notes fields have valid values
    return {
      schemaVersion: PANE_STATE_SCHEMA_VERSION,
      layout: payload.layout,
      descriptors: payload.descriptors.map(validateDescriptor),
    };
  } catch (e) {
    log(`loadPaneState(${sessionId}): failed — ${e}`);
    return null;
  }
}

export async function savePaneState(
  sessionId: string,
  payload: PaneStatePayload,
): Promise<void> {
  await savePaneStateRaw(sessionId, payload);
}

export async function deletePaneState(sessionId: string): Promise<void> {
  await deletePaneStateRaw(sessionId);
}

// ── Restore helpers ───────────────────────────────────────────────────────────

/**
 * Recursively removes leaves whose IDs match command-type descriptors.
 * Collapses single-child splits. Returns null if the entire tree is removed.
 */
export function stripCommandPanes(
  tree: LayoutNode,
  descriptors: PaneDescriptor[]
): { tree: LayoutNode | null; descriptors: PaneDescriptor[] } {
  const commandIds = new Set(
    descriptors.filter((d) => d.type === "command").map((d) => d.id),
  );
  const strippedTree = stripCommandsFromNode(tree, commandIds);
  const strippedDescriptors = descriptors.filter((d) => d.type !== "command");
  return { tree: strippedTree, descriptors: strippedDescriptors };
}

function stripCommandsFromNode(
  node: LayoutNode,
  commandIds: Set<string>
): LayoutNode | null {
  if (node.kind === "leaf") {
    return commandIds.has(node.paneId) ? null : node;
  }

  const newChildren: LayoutNode[] = [];
  for (const child of node.children) {
    const result = stripCommandsFromNode(child, commandIds);
    if (result !== null) {
      newChildren.push(result);
    }
  }

  if (newChildren.length === 0) return null;
  if (newChildren.length === 1) return newChildren[0];
  return { ...node, children: newChildren };
}

// ── Debounced auto-save ───────────────────────────────────────────────────────

const DEBOUNCE_MS = 1500;

let saveTimer: ReturnType<typeof setTimeout> | null = null;
// Track which sessions have pending unsaved changes
let dirtySessions: Set<string> = new Set();

export function scheduleSave(sessionIds: Iterable<string>): void {
  for (const sessionId of sessionIds) {
    dirtySessions.add(sessionId);
  }

  if (dirtySessions.size === 0) return;

  if (saveTimer !== null) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    void writeAllDirty();
  }, DEBOUNCE_MS);
}

async function writeAllDirty(): Promise<void> {
  const layouts = get(sessionLayouts);
  const toWrite = new Set(dirtySessions);
  dirtySessions.clear();

  for (const sessionId of toWrite) {
    const tree = layouts.get(sessionId);
    if (!tree) continue;
    try {
      await saveLivePaneStateRaw(
        sessionId,
        PANE_STATE_SCHEMA_VERSION,
        tree,
        collectLeafIds(tree),
      );
    } catch (e) {
      log(`auto-save failed for session ${sessionId}: ${e}`);
    }
  }
}

function changedLayoutSessionIds(
  previous: Map<string, LayoutNode>,
  current: Map<string, LayoutNode>,
): string[] {
  const changed: string[] = [];
  for (const [sessionId, tree] of current) {
    if (previous.get(sessionId) !== tree) {
      changed.push(sessionId);
    }
  }
  return changed;
}

/**
 * Cancels any pending debounce timer and writes session pane state to disk.
 *
 * Without `sessionId`, force-marks every session in `sessionLayouts` dirty
 * and writes all of them — used by quit handlers where every session's
 * latest state needs to land before the process exits.
 *
 * With `sessionId`, only that session is force-marked and written. This is
 * what `closeSession` uses: on launch, multiple disconnected sessions get
 * a transient primary-only layout in `sessionLayouts` until the user
 * clicks Continue. A blanket flush during one close would overwrite the
 * other sessions' rich persisted layouts with that primary-only stub,
 * losing their split panes for the next restore.
 *
 * Force-marking is necessary because shell cwd changes via `cd` don't
 * touch `sessionLayouts`, so `dirtySessions` may be empty even when
 * on-disk state is stale.
 */
export async function flushPaneState(sessionId?: string): Promise<void> {
  const layouts = get(sessionLayouts);

  if (sessionId !== undefined) {
    // Single-session flush. Don't cancel the global debounce timer or
    // touch other sessions' dirty bits — those still belong to the
    // ongoing debounce window. Just write this one and exit.
    const tree = layouts.get(sessionId);
    if (!tree) return;
    dirtySessions.delete(sessionId);
    try {
      await saveLivePaneStateRaw(
        sessionId,
        PANE_STATE_SCHEMA_VERSION,
        tree,
        collectLeafIds(tree),
      );
    } catch (e) {
      log(`flushPaneState(${sessionId}): failed — ${e}`);
    }
    return;
  }

  // Whole-app flush (quit handler): force-mark every session and write
  // them all. This is also what cancels the pending debounce timer.
  if (saveTimer !== null) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  for (const id of layouts.keys()) {
    dirtySessions.add(id);
  }
  if (dirtySessions.size > 0) {
    await writeAllDirty();
  }
}

let unsubscribe: (() => void) | null = null;

/**
 * Subscribe to sessionLayouts changes and auto-save.
 * Call once during app initialization.
 */
export function initPersistence(): void {
  if (unsubscribe) return;
  // Skip the first callback — Svelte stores fire immediately on subscribe
  // with the current value, which is the initial state, not a mutation.
  // Without this guard, startup would schedule a save of whatever layout
  // was already in the store (e.g. main-only leaves from session restore),
  // clobbering the persisted full layout on disk.
  let isFirstCallback = true;
  let previousLayouts = new Map<string, LayoutNode>();
  unsubscribe = sessionLayouts.subscribe((layouts) => {
    if (isFirstCallback) {
      isFirstCallback = false;
      previousLayouts = layouts;
      return;
    }
    const changed = changedLayoutSessionIds(previousLayouts, layouts);
    previousLayouts = layouts;
    scheduleSave(changed);
  });
}

/**
 * Stop the auto-save subscription — for tests only.
 */
export function stopPersistence(): void {
  if (unsubscribe) {
    unsubscribe();
    unsubscribe = null;
  }
  if (saveTimer !== null) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  dirtySessions.clear();
}
