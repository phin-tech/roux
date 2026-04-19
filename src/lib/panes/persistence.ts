import { get } from "svelte/store";
import type { LayoutNode } from "./layout";
import { sessionLayouts, collectLeafIds } from "./layout";
import type { PaneType } from "./instances";
import type { NotesScope } from "$lib/tauri";
import type { SpawnProfileRef } from "./profiles";
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
    const payload = raw as { schemaVersion?: number } & PaneStatePayload;
    if (payload.schemaVersion !== PANE_STATE_SCHEMA_VERSION) {
      log(
        `loadPaneState(${sessionId}): schema mismatch (got ${
          payload.schemaVersion ?? "missing"
        }, expected ${PANE_STATE_SCHEMA_VERSION}) — dropping persisted state`,
      );
      return null;
    }
    // Validate descriptors to ensure notes fields have valid values
    payload.descriptors = payload.descriptors.map(validateDescriptor);
    return payload;
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

export function scheduleSave(layouts: Map<string, LayoutNode>): void {
  // Mark all current sessions as dirty
  for (const sessionId of layouts.keys()) {
    dirtySessions.add(sessionId);
  }

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

/**
 * Cancels any pending debounce timer and writes all currently-mounted sessions
 * immediately. Call from quit/close handlers.
 *
 * Always force-marks every session in sessionLayouts dirty before writing:
 * users can mutate shell cwd (via `cd`) without touching sessionLayouts, so
 * dirtySessions may be empty even when the on-disk state is stale. Quit is
 * rare, so the extra write is cheap.
 */
export async function flushPaneState(): Promise<void> {
  if (saveTimer !== null) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  const layouts = get(sessionLayouts);
  for (const sessionId of layouts.keys()) {
    dirtySessions.add(sessionId);
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
  unsubscribe = sessionLayouts.subscribe((layouts) => {
    if (isFirstCallback) {
      isFirstCallback = false;
      return;
    }
    scheduleSave(layouts);
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
