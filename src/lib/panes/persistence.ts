import { get } from "svelte/store";
import type { LayoutNode } from "./layout";
import { sessionLayouts, collectLeafIds } from "./layout";
import { paneInstances, type PaneInstance } from "./instances";
import { loadPaneStateRaw, savePaneStateRaw, deletePaneStateRaw } from "$lib/tauri";
import { log } from "$lib/logging";

export interface PaneDescriptor {
  id: string;
  type: "claude" | "shell" | "command" | "markdown";
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
}

export interface PaneStatePayload {
  layout: LayoutNode;
  descriptors: PaneDescriptor[];
}

// ── Public async API ──────────────────────────────────────────────────────────

export async function loadPaneState(
  sessionId: string,
): Promise<PaneStatePayload | null> {
  try {
    const raw = await loadPaneStateRaw(sessionId);
    if (raw == null) return null;
    return raw as PaneStatePayload;
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
    descriptors.filter((d) => d.type === "command").map((d) => d.id)
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

function descriptorsForSession(sessionId: string): PaneDescriptor[] {
  const instances = get(paneInstances);
  const layouts = get(sessionLayouts);
  const tree = layouts.get(sessionId);
  if (!tree) return [];

  const paneIds = collectLeafIds(tree);

  return paneIds
    .map((id) => instances.get(id))
    .filter((inst): inst is PaneInstance => inst != null)
    .map((inst) => ({
      id: inst.id,
      type: inst.type,
      ptyId: inst.ptyId,
      name: inst.name,
      workingDir: inst.workingDir,
      command: inst.command,
      docPath: inst.docPath,
    }));
}

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
    const descriptors = descriptorsForSession(sessionId);
    try {
      await savePaneState(sessionId, { layout: tree, descriptors });
    } catch (e) {
      log(`auto-save failed for session ${sessionId}: ${e}`);
    }
  }
}

/**
 * Cancels any pending debounce timer and writes all dirty sessions immediately.
 * Call from quit/close handlers to avoid losing the last layout change.
 */
export async function flushPaneState(): Promise<void> {
  if (saveTimer !== null) {
    clearTimeout(saveTimer);
    saveTimer = null;
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
