import { writable, get } from "svelte/store";

export type SplitDirection = "horizontal" | "vertical";

export interface Pane {
  id: string;
  type: "claude" | "shell" | "markdown" | "command";
  ptyId: string;
  name?: string;
  docPath?: string;
  command?: string;
  workingDir?: string;
}

export type SplitNode =
  | { kind: "pane"; pane: Pane }
  | { kind: "split"; direction: SplitDirection; children: SplitNode[];
      stacked?: boolean; activeIndex?: number };

export const paneTrees = writable<Map<string, SplitNode>>(new Map());
export const focusedPaneId = writable<string | null>(null);

function findPaneInTree(node: SplitNode, paneId: string): Pane | null {
  if (node.kind === "pane") {
    return node.pane.id === paneId ? node.pane : null;
  }
  for (const child of node.children) {
    const pane = findPaneInTree(child, paneId);
    if (pane) return pane;
  }
  return null;
}

function collectPanes(node: SplitNode, panes: Pane[]) {
  if (node.kind === "pane") {
    panes.push(node.pane);
    return;
  }
  for (const child of node.children) {
    collectPanes(child, panes);
  }
}

function firstPaneId(node: SplitNode): string {
  if (node.kind === "pane") return node.pane.id;
  return firstPaneId(node.children[0]);
}

export function initSessionPanes(sessionId: string) {
  paneTrees.update((trees) => {
    if (!trees.has(sessionId)) {
      trees.set(sessionId, {
        kind: "pane",
        pane: { id: sessionId + "-main", type: "claude", ptyId: sessionId },
      });
    }
    return new Map(trees);
  });
}

export function addSplit(sessionId: string, direction: SplitDirection, newPane: Pane) {
  paneTrees.update((trees) => {
    const current = trees.get(sessionId);
    if (!current) return trees;
    const focused = get(focusedPaneId);
    trees.set(sessionId, splitAtPane(current, focused, direction, newPane));
    return new Map(trees);
  });
  focusedPaneId.set(newPane.id);
}

function splitAtPane(node: SplitNode, targetId: string | null, direction: SplitDirection, newPane: Pane): SplitNode {
  if (node.kind === "pane") {
    if (!targetId || node.pane.id === targetId) {
      return {
        kind: "split",
        direction,
        children: [node, { kind: "pane", pane: newPane }],
      };
    }
    return node;
  }
  return {
    ...node,
    children: node.children.map((child) => splitAtPane(child, targetId, direction, newPane)),
  };
}

export function removePane(sessionId: string, paneId: string) {
  const currentFocus = get(focusedPaneId);
  let nextFocus = currentFocus;

  paneTrees.update((trees) => {
    const current = trees.get(sessionId);
    if (!current) return trees;
    const result = removePaneFromTree(current, paneId);
    if (result) {
      trees.set(sessionId, result);
      if (currentFocus === paneId) {
        nextFocus = firstPaneId(result);
      }
    } else {
      trees.delete(sessionId);
      if (currentFocus === paneId) {
        nextFocus = null;
      }
    }
    return new Map(trees);
  });

  if (currentFocus === paneId) {
    focusedPaneId.set(nextFocus);
  }
}

function removePaneFromTree(node: SplitNode, paneId: string): SplitNode | null {
  if (node.kind === "pane") {
    return node.pane.id === paneId ? null : node;
  }
  const remaining = node.children
    .map((c) => removePaneFromTree(c, paneId))
    .filter((c): c is SplitNode => c !== null);
  if (remaining.length === 0) return null;
  if (remaining.length === 1) {
    // Auto-unstack: collapsing to 1 child means the split dissolves
    return remaining[0];
  }
  // Clamp activeIndex if this is a stacked split
  const activeIndex = node.stacked
    ? Math.min(node.activeIndex ?? 0, remaining.length - 1)
    : node.activeIndex;
  return { ...node, children: remaining, activeIndex };
}

export function removeSessionPanes(sessionId: string) {
  const focused = get(focusedPaneId);
  const sessionPaneIds = new Set(listPanes(sessionId).map((pane) => pane.id));

  paneTrees.update((trees) => {
    trees.delete(sessionId);
    return new Map(trees);
  });

  if (focused && sessionPaneIds.has(focused)) {
    focusedPaneId.set(null);
  }
}

/** Returns true if the session has any split panes (more than just the main claude pane) */
export function hasSplitPanes(sessionId: string): boolean {
  const trees = get(paneTrees);
  const tree = trees.get(sessionId);
  if (!tree) return false;
  return tree.kind === "split";
}

export function getPane(sessionId: string, paneId: string): Pane | null {
  const tree = get(paneTrees).get(sessionId);
  if (!tree) return null;
  return findPaneInTree(tree, paneId);
}

export function listPanes(sessionId: string): Pane[] {
  const tree = get(paneTrees).get(sessionId);
  if (!tree) return [];
  const panes: Pane[] = [];
  collectPanes(tree, panes);
  return panes;
}

function setPaneNameInTree(node: SplitNode, paneId: string, name: string | undefined): SplitNode {
  if (node.kind === "pane") {
    if (node.pane.id === paneId) {
      return { kind: "pane", pane: { ...node.pane, name } };
    }
    return node;
  }
  return {
    ...node,
    children: node.children.map((child) => setPaneNameInTree(child, paneId, name)),
  };
}

export function renamePane(sessionId: string, paneId: string, name: string) {
  paneTrees.update((trees) => {
    const tree = trees.get(sessionId);
    if (!tree) return trees;
    trees.set(sessionId, setPaneNameInTree(tree, paneId, name || undefined));
    return new Map(trees);
  });
}

/** Generate a display label for a SplitNode (used for collapsed stack tabs). */
export function getStackLabel(node: SplitNode): string {
  if (node.kind === "pane") {
    return node.pane.name ?? node.pane.type;
  }
  const panes: Pane[] = [];
  collectPanes(node, panes);
  return panes.map((p) => p.name ?? p.type).join(" | ");
}

// ── Stacked panes ──────────────────────────────────────────

/** Build the path of child indices from root to the split containing targetId. */
function buildSplitPath(
  node: SplitNode,
  targetId: string,
  path: number[]
): boolean {
  if (node.kind === "pane") return node.pane.id === targetId;
  for (let i = 0; i < node.children.length; i++) {
    path.push(i);
    if (buildSplitPath(node.children[i], targetId, path)) return true;
    path.pop();
  }
  return false;
}

/** Resolve a split node at the given path of child indices. */
function splitAtPath(root: SplitNode, path: number[], depth: number): SplitNode & { kind: "split" } {
  let node = root;
  for (let i = 0; i < depth; i++) {
    if (node.kind !== "split") throw new Error("Expected split");
    node = node.children[path[i]];
  }
  if (node.kind !== "split") throw new Error("Expected split at path");
  return node;
}

/** Collect the depth of each ancestor split from the path (depths 0..path.length-1 are splits). */
function ancestorSplitDepths(root: SplitNode, path: number[]): number[] {
  const depths: number[] = [];
  let node = root;
  for (let i = 0; i < path.length; i++) {
    if (node.kind === "split") {
      depths.push(i);
      node = node.children[path[i]];
    }
  }
  return depths;
}

/** Find which child index of a split contains the given pane ID. */
function childIndexContaining(split: SplitNode & { kind: "split" }, paneId: string): number {
  for (let i = 0; i < split.children.length; i++) {
    if (findPaneInTree(split.children[i], paneId)) return i;
  }
  return 0;
}

/** Set stacked/activeIndex on the split node at the given depth in the path. */
function setStackedAtDepth(
  node: SplitNode,
  path: number[],
  targetDepth: number,
  currentDepth: number,
  stacked: boolean,
  activeIndex: number | undefined
): SplitNode {
  if (node.kind === "pane") return node;
  if (currentDepth === targetDepth) {
    return { ...node, stacked: stacked || undefined, activeIndex: stacked ? (activeIndex ?? 0) : undefined };
  }
  const childIdx = path[currentDepth];
  const newChildren = node.children.map((c, i) =>
    i === childIdx ? setStackedAtDepth(c, path, targetDepth, currentDepth + 1, stacked, activeIndex) : c
  );
  if (newChildren.every((c, i) => c === node.children[i])) return node;
  return { ...node, children: newChildren };
}

/** Apply multiple stacked changes to the tree. */
function applyStackChanges(
  root: SplitNode,
  path: number[],
  changes: { depth: number; stacked: boolean; activeIndex: number | undefined }[]
): SplitNode {
  let result = root;
  for (const change of changes) {
    result = setStackedAtDepth(result, path, change.depth, 0, change.stacked, change.activeIndex);
  }
  return result;
}

/** Toggle stacking on the tree, cycling through ancestor splits. */
function toggleStackInTree(root: SplitNode, focusedId: string): SplitNode {
  const path: number[] = [];
  if (!buildSplitPath(root, focusedId, path)) return root;

  // Collect ancestor split depths
  const splitDepths = ancestorSplitDepths(root, path);
  if (splitDepths.length === 0) return root; // focused pane is the root, no split to stack

  // Find the lowest (deepest) stacked ancestor
  let lowestStackedDepthIdx = -1;
  for (let i = splitDepths.length - 1; i >= 0; i--) {
    const split = splitAtPath(root, path, splitDepths[i]);
    if (split.stacked) {
      lowestStackedDepthIdx = i;
      break;
    }
  }

  if (lowestStackedDepthIdx === -1) {
    // Nothing stacked yet — stack the immediate parent (last split depth)
    const depth = splitDepths[splitDepths.length - 1];
    const split = splitAtPath(root, path, depth);
    return setStackedAtDepth(root, path, depth, 0, true, childIndexContaining(split, focusedId));
  } else if (lowestStackedDepthIdx > 0) {
    // Something is stacked but there's a higher ancestor — unstack current, stack higher
    const unstackDepth = splitDepths[lowestStackedDepthIdx];
    const stackDepth = splitDepths[lowestStackedDepthIdx - 1];
    const higherSplit = splitAtPath(root, path, stackDepth);
    return applyStackChanges(root, path, [
      { depth: unstackDepth, stacked: false, activeIndex: undefined },
      { depth: stackDepth, stacked: true, activeIndex: childIndexContaining(higherSplit, focusedId) },
    ]);
  } else {
    // Already stacked at root level — unstack everything
    return setStackedAtDepth(root, path, splitDepths[0], 0, false, undefined);
  }
}

export function toggleStack(sessionId: string) {
  const focused = get(focusedPaneId);
  if (!focused) return;

  paneTrees.update((trees) => {
    const tree = trees.get(sessionId);
    if (!tree) return trees;
    trees.set(sessionId, toggleStackInTree(tree, focused));
    return new Map(trees);
  });
}

export function setActiveStackIndex(sessionId: string, index: number) {
  const focused = get(focusedPaneId);
  if (!focused) return;

  let newFocusTarget: SplitNode | null = null;

  paneTrees.update((trees) => {
    const tree = trees.get(sessionId);
    if (!tree) return trees;

    const path: number[] = [];
    if (!buildSplitPath(tree, focused, path)) return trees;

    // Find nearest stacked ancestor
    const splitDepths = ancestorSplitDepths(tree, path);
    let stackedDepth = -1;
    for (let i = splitDepths.length - 1; i >= 0; i--) {
      const split = splitAtPath(tree, path, splitDepths[i]);
      if (split.stacked) {
        stackedDepth = splitDepths[i];
        break;
      }
    }
    if (stackedDepth === -1) return trees;

    const split = splitAtPath(tree, path, stackedDepth);
    const clamped = Math.max(0, Math.min(index, split.children.length - 1));
    newFocusTarget = split.children[clamped];
    trees.set(sessionId, setStackedAtDepth(tree, path, stackedDepth, 0, true, clamped));
    return new Map(trees);
  });

  if (newFocusTarget) {
    focusedPaneId.set(firstPaneId(newFocusTarget));
  }
}

// ── Pane drag-and-drop ─────────────────────────────────────

export type DropSide = "left" | "right" | "top" | "bottom";

const dropSideToDirection: Record<DropSide, SplitDirection> = {
  left: "horizontal",
  right: "horizontal",
  top: "vertical",
  bottom: "vertical",
};

/** Move a pane from its current position to a new position next to a target pane. */
export function movePane(sessionId: string, paneId: string, targetPaneId: string, side: DropSide) {
  if (paneId === targetPaneId) return;

  paneTrees.update((trees) => {
    let tree = trees.get(sessionId);
    if (!tree) return trees;

    // 1. Extract the pane data before removing
    const pane = findPaneInTree(tree, paneId);
    if (!pane) return trees;
    const paneCopy = { ...pane };

    // 2. Remove from current position
    const afterRemove = removePaneFromTree(tree, paneId);
    if (!afterRemove) return trees; // was the only pane

    // 3. Verify target still exists after removal
    if (!findPaneInTree(afterRemove, targetPaneId)) return trees;

    // 4. Insert at new position next to the target
    const direction = dropSideToDirection[side];
    const insertBefore = side === "left" || side === "top";
    const newNode: SplitNode = { kind: "pane", pane: paneCopy };

    const afterInsert = insertPaneAtTarget(afterRemove, targetPaneId, direction, newNode, insertBefore);
    trees.set(sessionId, afterInsert);
    return new Map(trees);
  });

  focusedPaneId.set(paneId);
}

function insertPaneAtTarget(
  node: SplitNode,
  targetId: string,
  direction: SplitDirection,
  newNode: SplitNode,
  insertBefore: boolean,
): SplitNode {
  if (node.kind === "pane") {
    if (node.pane.id === targetId) {
      const children = insertBefore ? [newNode, node] : [node, newNode];
      return { kind: "split", direction, children };
    }
    return node;
  }
  return {
    ...node,
    children: node.children.map((child) =>
      insertPaneAtTarget(child, targetId, direction, newNode, insertBefore),
    ),
  };
}

export type Direction = "left" | "right" | "up" | "down";

type PathEntry = { parent: SplitNode & { kind: "split" }; childIndex: number };

function buildPath(node: SplitNode, targetId: string, path: PathEntry[]): boolean {
  if (node.kind === "pane") return node.pane.id === targetId;
  for (let i = 0; i < node.children.length; i++) {
    path.push({ parent: node, childIndex: i });
    if (buildPath(node.children[i], targetId, path)) return true;
    path.pop();
  }
  return false;
}

function lastPaneId(node: SplitNode): string {
  if (node.kind === "pane") return node.pane.id;
  return lastPaneId(node.children[node.children.length - 1]);
}

const directionAxis: Record<Direction, SplitDirection> = {
  left: "horizontal", right: "horizontal",
  up: "vertical", down: "vertical",
};
const directionStep: Record<Direction, number> = {
  left: -1, right: 1, up: -1, down: 1,
};

/** Immutable update: replace a split node found by reference with a modified copy. */
function updateSplitByRef(node: SplitNode, target: SplitNode & { kind: "split" }, patch: Partial<SplitNode & { kind: "split" }>): SplitNode {
  if (node === target) {
    return { ...node, ...patch } as SplitNode;
  }
  if (node.kind === "pane") return node;
  const newChildren = node.children.map((c) => updateSplitByRef(c, target, patch));
  if (newChildren.every((c, i) => c === node.children[i])) return node;
  return { ...node, children: newChildren };
}

export function navigatePane(sessionId: string, direction: Direction) {
  const tree = get(paneTrees).get(sessionId);
  if (!tree) return;
  const focused = get(focusedPaneId);
  if (!focused) return;

  const path: PathEntry[] = [];
  if (!buildPath(tree, focused, path)) return;

  const axis = directionAxis[direction];
  const step = directionStep[direction];

  // Check for stacked ancestor — up/down navigates tabs within stacks
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent } = path[i];
    if (!parent.stacked) continue;

    // Only up/down cycles stack tabs
    if (axis !== "vertical") continue;

    const nextIndex = (parent.activeIndex ?? 0) + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) return;

    // Update activeIndex and focus the first pane in the new active child
    paneTrees.update((trees) => {
      const root = trees.get(sessionId);
      if (!root) return trees;
      trees.set(sessionId, updateSplitByRef(root, parent, { activeIndex: nextIndex }));
      return new Map(trees);
    });
    const target = parent.children[nextIndex];
    focusedPaneId.set(firstPaneId(target));
    return;
  }

  // No stacked ancestor matched — fall through to normal spatial navigation
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (parent.direction !== axis) continue;
    const nextIndex = childIndex + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) continue;
    const target = parent.children[nextIndex];
    const newFocus = step > 0 ? firstPaneId(target) : lastPaneId(target);
    focusedPaneId.set(newFocus);
    return;
  }
}
