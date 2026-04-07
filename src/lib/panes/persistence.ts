import type { LayoutNode } from "./layout";

export interface PaneDescriptor {
  id: string;
  type: "claude" | "shell" | "command" | "markdown";
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
}

const LAYOUT_KEY = "roux:pane-layouts-v2";
const DESCRIPTOR_KEY = "roux:pane-descriptors";

// ── Layout persistence ────────────────────────────────────────────────────────

export function saveLayout(sessionId: string, tree: LayoutNode): void {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    const all: Record<string, LayoutNode> = raw ? JSON.parse(raw) : {};
    all[sessionId] = tree;
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(all));
  } catch {
    // silently ignore storage errors
  }
}

export function loadLayout(sessionId: string): LayoutNode | null {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (!raw) return null;
    const all: Record<string, LayoutNode> = JSON.parse(raw);
    return all[sessionId] ?? null;
  } catch {
    return null;
  }
}

export function clearLayout(sessionId: string): void {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (!raw) return;
    const all: Record<string, LayoutNode> = JSON.parse(raw);
    delete all[sessionId];
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(all));
  } catch {
    // silently ignore storage errors
  }
}

// ── Descriptor persistence ────────────────────────────────────────────────────

export function savePaneDescriptors(sessionId: string, descriptors: PaneDescriptor[]): void {
  try {
    const raw = localStorage.getItem(DESCRIPTOR_KEY);
    const all: Record<string, PaneDescriptor[]> = raw ? JSON.parse(raw) : {};
    all[sessionId] = descriptors;
    localStorage.setItem(DESCRIPTOR_KEY, JSON.stringify(all));
  } catch {
    // silently ignore storage errors
  }
}

export function loadPaneDescriptors(sessionId: string): PaneDescriptor[] | null {
  try {
    const raw = localStorage.getItem(DESCRIPTOR_KEY);
    if (!raw) return null;
    const all: Record<string, PaneDescriptor[]> = JSON.parse(raw);
    return all[sessionId] ?? null;
  } catch {
    return null;
  }
}

export function clearPaneDescriptors(sessionId: string): void {
  try {
    const raw = localStorage.getItem(DESCRIPTOR_KEY);
    if (!raw) return;
    const all: Record<string, PaneDescriptor[]> = JSON.parse(raw);
    delete all[sessionId];
    localStorage.setItem(DESCRIPTOR_KEY, JSON.stringify(all));
  } catch {
    // silently ignore storage errors
  }
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

  // Recurse into children, filtering out nulls
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

let saveTimer: ReturnType<typeof setTimeout> | null = null;

export function scheduleSave(
  layouts: Map<string, LayoutNode>,
  getDescriptors: (sessionId: string) => PaneDescriptor[]
): void {
  if (saveTimer !== null) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    for (const [sessionId, tree] of layouts) {
      saveLayout(sessionId, tree);
      savePaneDescriptors(sessionId, getDescriptors(sessionId));
    }
  }, 300);
}
