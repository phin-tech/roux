import { writable, get } from "svelte/store";
import { setLogicalFocus, focusedPaneId } from "./focus";
import { getInstance } from "./instances";

// ── Types ────────────────────────────────────────────────────────────────────

export type SplitDirection = "h" | "v";

export type LayoutNode =
  | { kind: "leaf"; paneId: string }
  | {
      kind: "split";
      direction: SplitDirection;
      children: LayoutNode[];
      sizes?: number[];
      stacked?: boolean;
      activeIndex?: number;
    };

// ── Store ────────────────────────────────────────────────────────────────────

export const sessionLayouts = writable<Map<string, LayoutNode>>(new Map());

// ── Store mutations ──────────────────────────────────────────────────────────

/**
 * Creates a single-leaf layout for sessionId unless one already exists.
 */
export function initSessionLayout(sessionId: string, mainPaneId: string): void {
  sessionLayouts.update((m) => {
    if (m.has(sessionId)) return m;
    const next = new Map(m);
    next.set(sessionId, { kind: "leaf", paneId: mainPaneId });
    return next;
  });
}

// ── Pure tree transforms ─────────────────────────────────────────────────────

/**
 * Inserts a new leaf next to targetId in the given direction.
 * Same-direction splits are flattened into siblings.
 */
export function insertLeaf(
  node: LayoutNode,
  targetId: string,
  direction: SplitDirection,
  newPaneId: string
): LayoutNode {
  if (node.kind === "leaf") {
    if (node.paneId !== targetId) return node;
    // Wrap this leaf and the new leaf in a split node
    return {
      kind: "split",
      direction,
      children: [
        { kind: "leaf", paneId: targetId },
        { kind: "leaf", paneId: newPaneId },
      ],
    };
  }

  // node is a split — recurse into children
  const newChildren = node.children.map((child) =>
    insertLeaf(child, targetId, direction, newPaneId)
  );

  // Check if any child was transformed into a same-direction split that
  // should be flattened into this level
  const flatChildren: LayoutNode[] = [];
  let changed = false;

  for (let i = 0; i < newChildren.length; i++) {
    const orig = node.children[i];
    const next = newChildren[i];

    if (
      next !== orig &&
      next.kind === "split" &&
      next.direction === node.direction
    ) {
      // Flatten: inline the new split's children at this level
      flatChildren.push(...next.children);
      changed = true;
    } else {
      flatChildren.push(next);
      if (next !== orig) changed = true;
    }
  }

  if (!changed) return node;

  return { ...node, children: flatChildren, sizes: undefined };
}

/**
 * Removes the leaf with paneId from the tree.
 * - Returns null if the removed leaf was the only node.
 * - Collapses single-child splits into their sole child.
 * - Clamps activeIndex if stacked.
 * - Re-normalises sizes (drops the removed slot and redistributes proportionally).
 */
export function removeLeaf(
  node: LayoutNode,
  paneId: string
): LayoutNode | null {
  if (node.kind === "leaf") {
    return node.paneId === paneId ? null : node;
  }

  // Find the index of the child that contains paneId (or is paneId)
  const targetChildIndex = node.children.findIndex((c) =>
    containsPaneId(c, paneId)
  );

  if (targetChildIndex === -1) return node; // not in this subtree

  const targetChild = node.children[targetChildIndex];
  const updatedChild = removeLeaf(targetChild, paneId);

  let newChildren: LayoutNode[];
  let removedIndex: number;

  if (updatedChild === null) {
    // The child was fully removed
    newChildren = node.children.filter((_, i) => i !== targetChildIndex);
    removedIndex = targetChildIndex;
  } else {
    newChildren = node.children.map((c, i) =>
      i === targetChildIndex ? updatedChild : c
    );
    removedIndex = -1; // no slot removed at this level
  }

  // Collapse single-child split
  if (newChildren.length === 1) {
    return newChildren[0];
  }

  // Adjust sizes
  let newSizes = node.sizes;
  if (node.sizes && removedIndex !== -1) {
    const removed = node.sizes[removedIndex];
    const remaining = node.sizes.filter((_, i) => i !== removedIndex);
    const total = remaining.reduce((a, b) => a + b, 0);
    if (total > 0) {
      // Redistribute the removed slot proportionally
      newSizes = remaining.map((s) => s + (s / total) * removed);
    } else {
      // Fallback: equal distribution
      newSizes = remaining.map(() => 1 / remaining.length);
    }
  }

  // Clamp activeIndex
  let newActiveIndex = node.activeIndex;
  if (node.stacked && node.activeIndex !== undefined && removedIndex !== -1) {
    newActiveIndex = Math.min(node.activeIndex, newChildren.length - 1);
  }

  return {
    ...node,
    children: newChildren,
    sizes: newSizes,
    activeIndex: newActiveIndex,
  };
}

// ── Helper functions ─────────────────────────────────────────────────────────

/** Returns the paneId of the leftmost (first) leaf. */
export function firstLeafId(node: LayoutNode): string {
  if (node.kind === "leaf") return node.paneId;
  return firstLeafId(node.children[0]);
}

/** Returns the paneId of the rightmost (last) leaf. */
export function lastLeafId(node: LayoutNode): string {
  if (node.kind === "leaf") return node.paneId;
  return lastLeafId(node.children[node.children.length - 1]);
}

/** Collects all leaf paneIds in the tree. */
export function collectLeafIds(node: LayoutNode): string[] {
  if (node.kind === "leaf") return [node.paneId];
  return node.children.flatMap(collectLeafIds);
}

/**
 * Returns the sessionId whose layout currently contains paneId, or null
 * if no session owns it. Walks all layouts; per-session traversal
 * short-circuits at the first match via `containsPaneId`.
 */
export function findSessionForPane(paneId: string): string | null {
  for (const [sessionId, layout] of get(sessionLayouts)) {
    if (containsPaneId(layout, paneId)) return sessionId;
  }
  return null;
}

/**
 * Collects leaf paneIds in DFS order, visiting only the currently-visible
 * branch of stacked splits. Hidden stack tabs are skipped because the
 * session-switch / pane-jump overlay can only draw a badge on a rendered
 * pane element, and a focused pane inside a hidden stack tab would not be
 * reachable without extra stack-activation bookkeeping.
 */
export function collectVisibleLeafIds(node: LayoutNode): string[] {
  if (node.kind === "leaf") return [node.paneId];
  if (node.stacked) {
    const activeIdx = node.activeIndex ?? 0;
    const active = node.children[activeIdx];
    return active ? collectVisibleLeafIds(active) : [];
  }
  return node.children.flatMap(collectVisibleLeafIds);
}

/** Returns true if the given node contains paneId anywhere in its subtree. */
export function containsPaneId(node: LayoutNode, paneId: string): boolean {
  if (node.kind === "leaf") return node.paneId === paneId;
  return node.children.some((c) => containsPaneId(c, paneId));
}

/** Returns true if the session has more than one pane (i.e. a split exists). */
export function hasSplitPanes(sessionId: string): boolean {
  const layout = get(sessionLayouts).get(sessionId);
  if (!layout) return false;
  return layout.kind === "split";
}

/** Resets all layouts — for use in tests only. */
export function resetLayouts(): void {
  sessionLayouts.set(new Map());
}

// ── Navigation / Movement / Resize types ────────────────────────────────────

export type Direction = "left" | "right" | "up" | "down";
export type DropSide = "left" | "right" | "top" | "bottom";

type PathEntry = { parent: LayoutNode & { kind: "split" }; childIndex: number };

const directionAxis: Record<Direction, SplitDirection> = {
  left: "h", right: "h",
  up: "v", down: "v",
};
const directionStep: Record<Direction, number> = {
  left: -1, right: 1, up: -1, down: 1,
};

const dropSideToDirection: Record<DropSide, SplitDirection> = {
  left: "h",
  right: "h",
  top: "v",
  bottom: "v",
};

const MIN_PANE_FRACTION = 0.05;

// ── Internal tree helpers ────────────────────────────────────────────────────

/** Builds the path of { parent, childIndex } entries from root to target leaf. */
function buildPath(node: LayoutNode, targetId: string, path: PathEntry[]): boolean {
  if (node.kind === "leaf") return node.paneId === targetId;
  for (let i = 0; i < node.children.length; i++) {
    path.push({ parent: node as LayoutNode & { kind: "split" }, childIndex: i });
    if (buildPath(node.children[i], targetId, path)) return true;
    path.pop();
  }
  return false;
}

/** Immutable update: replace a split node found by reference with a modified copy. */
function updateSplitByRef(
  node: LayoutNode,
  target: LayoutNode & { kind: "split" },
  patch: Partial<LayoutNode & { kind: "split" }>
): LayoutNode {
  if (node === target) {
    return { ...node, ...patch } as LayoutNode;
  }
  if (node.kind === "leaf") return node;
  const newChildren = node.children.map((c) => updateSplitByRef(c, target, patch));
  if (newChildren.every((c, i) => c === node.children[i])) return node;
  return { ...node, children: newChildren };
}

/** Replace a specific split node in the tree with a replacement node. */
function replaceSplitInTree(
  root: LayoutNode,
  target: LayoutNode & { kind: "split" },
  replacement: LayoutNode
): LayoutNode {
  if (root === target) return replacement;
  if (root.kind === "leaf") return root;
  const mapped = root.children.map((c) => replaceSplitInTree(c, target, replacement));
  if (mapped.every((c, i) => c === root.children[i])) return root;
  return { ...root, children: mapped };
}

/** Remove a leaf from a subtree (mirrors removeLeaf but avoids size/activeIndex adjustments). */
function removeLeafFromSubtree(node: LayoutNode, paneId: string): LayoutNode | null {
  if (node.kind === "leaf") {
    return node.paneId === paneId ? null : node;
  }
  const mapped = node.children.map((c) => removeLeafFromSubtree(c, paneId));
  const remaining = mapped.filter((c): c is LayoutNode => c !== null);
  if (remaining.length === 0) return null;
  if (remaining.length === 1) return remaining[0];
  return { ...node, children: remaining };
}

/** Insert a new leaf next to target, creating a split of the given direction. */
function insertLeafAtTarget(
  node: LayoutNode,
  targetId: string,
  direction: SplitDirection,
  newLeaf: LayoutNode,
  insertBefore: boolean
): LayoutNode {
  if (node.kind === "leaf") {
    if (node.paneId === targetId) {
      const children = insertBefore ? [newLeaf, node] : [node, newLeaf];
      return { kind: "split", direction, children };
    }
    return node;
  }
  return {
    ...node,
    children: node.children.map((child) =>
      insertLeafAtTarget(child, targetId, direction, newLeaf, insertBefore)
    ),
  };
}

// ── Stacked panes helpers ────────────────────────────────────────────────────

/** Build the path of child indices from root to the split containing targetId. */
function buildSplitPath(node: LayoutNode, targetId: string, path: number[]): boolean {
  if (node.kind === "leaf") return node.paneId === targetId;
  for (let i = 0; i < node.children.length; i++) {
    path.push(i);
    if (buildSplitPath(node.children[i], targetId, path)) return true;
    path.pop();
  }
  return false;
}

/** Resolve the split node at the given depth along path. */
function splitAtPath(
  root: LayoutNode,
  path: number[],
  depth: number
): LayoutNode & { kind: "split" } {
  let node = root;
  for (let i = 0; i < depth; i++) {
    if (node.kind !== "split") throw new Error("Expected split");
    node = node.children[path[i]];
  }
  if (node.kind !== "split") throw new Error("Expected split at path");
  return node;
}

/** Safely resolve a split node at the given path; returns null if path is invalid or target is not a split. */
function splitAtExactPath(
  root: LayoutNode,
  path: readonly number[]
): (LayoutNode & { kind: "split" }) | null {
  let node = root;
  for (const index of path) {
    if (node.kind !== "split") return null;
    if (!Number.isInteger(index) || index < 0 || index >= node.children.length) return null;
    node = node.children[index];
  }
  return node.kind === "split" ? node : null;
}

/** Immutably replace a split node at the given path with a replacement split, returning the updated root. */
function replaceSplitAtPath(
  root: LayoutNode,
  path: readonly number[],
  replacement: LayoutNode & { kind: "split" },
  depth = 0
): LayoutNode {
  if (depth === path.length) return replacement;
  if (root.kind === "leaf") return root;

  const childIdx = path[depth];
  const newChildren = root.children.map((child, i) =>
    i === childIdx ? replaceSplitAtPath(child, path, replacement, depth + 1) : child
  );
  if (newChildren.every((c, i) => c === root.children[i])) return root;
  return { ...root, children: newChildren };
}

/** Return normalized split sizes (sum to 1); returns equal distribution if sizes are missing or invalid. */
function normalizedSplitSizes(split: LayoutNode & { kind: "split" }): number[] {
  const count = split.children.length;
  const sizes = split.sizes;
  if (
    !sizes ||
    sizes.length !== count ||
    sizes.some((size) => !Number.isFinite(size) || size < 0)
  ) {
    return Array(count).fill(1 / count);
  }

  const total = sizes.reduce((sum, size) => sum + size, 0);
  if (total <= 0) return Array(count).fill(1 / count);
  return sizes.map((size) => size / total);
}

/** Collect the depths of ancestor split nodes along the path. */
function ancestorSplitDepths(root: LayoutNode, path: number[]): number[] {
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
function childIndexContaining(split: LayoutNode & { kind: "split" }, paneId: string): number {
  for (let i = 0; i < split.children.length; i++) {
    if (containsPaneId(split.children[i], paneId)) return i;
  }
  return 0;
}

/** Set stacked/activeIndex on the split at the given depth in the path. */
function setStackedAtDepth(
  node: LayoutNode,
  path: number[],
  targetDepth: number,
  currentDepth: number,
  stacked: boolean,
  activeIndex: number | undefined
): LayoutNode {
  if (node.kind === "leaf") return node;
  if (currentDepth === targetDepth) {
    return {
      ...node,
      stacked: stacked || undefined,
      activeIndex: stacked ? (activeIndex ?? 0) : undefined,
    };
  }
  const childIdx = path[currentDepth];
  const newChildren = node.children.map((c, i) =>
    i === childIdx
      ? setStackedAtDepth(c, path, targetDepth, currentDepth + 1, stacked, activeIndex)
      : c
  );
  if (newChildren.every((c, i) => c === node.children[i])) return node;
  return { ...node, children: newChildren };
}

/** Toggle stacking on the tree, cycling through ancestor splits. */
function toggleStackInTree(root: LayoutNode, focusedId: string): LayoutNode {
  const path: number[] = [];
  if (!buildSplitPath(root, focusedId, path)) return root;

  const splitDepths = ancestorSplitDepths(root, path);
  if (splitDepths.length === 0) return root;

  let lowestStackedDepthIdx = -1;
  for (let i = splitDepths.length - 1; i >= 0; i--) {
    const split = splitAtPath(root, path, splitDepths[i]);
    if (split.stacked) {
      lowestStackedDepthIdx = i;
      break;
    }
  }

  if (lowestStackedDepthIdx === -1) {
    const depth = splitDepths[splitDepths.length - 1];
    const split = splitAtPath(root, path, depth);
    return setStackedAtDepth(root, path, depth, 0, true, childIndexContaining(split, focusedId));
  } else {
    return setStackedAtDepth(
      root,
      path,
      splitDepths[lowestStackedDepthIdx],
      0,
      false,
      undefined
    );
  }
}

// ── Public navigation / move / resize / stack API ───────────────────────────

/** Get the layout tree for a session. */
export function getLayout(sessionId: string): LayoutNode {
  return get(sessionLayouts).get(sessionId)!;
}

/** Generate a display label for a LayoutNode (used for collapsed stack tabs). */
export function getStackLabel(node: LayoutNode): string {
  if (node.kind === "leaf") {
    const inst = getInstance(node.paneId);
    return inst?.name ?? inst?.type ?? node.paneId;
  }
  return node.children.map(getStackLabel).join(" | ");
}

/** Navigate focus in a direction within the session's layout tree. */
export function navigatePane(sessionId: string, direction: Direction): void {
  const tree = get(sessionLayouts).get(sessionId);
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
    if (axis !== "v") continue;

    const nextIndex = (parent.activeIndex ?? 0) + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) return;

    sessionLayouts.update((m) => {
      const root = m.get(sessionId);
      if (!root) return m;
      const next = new Map(m);
      next.set(sessionId, updateSplitByRef(root, parent, { activeIndex: nextIndex }));
      return next;
    });
    const target = parent.children[nextIndex];
    setLogicalFocus(firstLeafId(target));
    return;
  }

  // Normal spatial navigation
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (parent.direction !== axis) continue;
    const nextIndex = childIndex + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) continue;
    const target = parent.children[nextIndex];
    const newFocus = step > 0 ? firstLeafId(target) : lastLeafId(target);
    setLogicalFocus(newFocus);
    return;
  }
}

/** Toggle stacking on the parent split of the focused pane. */
export function toggleStack(sessionId: string): void {
  const focused = get(focusedPaneId);
  if (!focused) return;

  sessionLayouts.update((m) => {
    const tree = m.get(sessionId);
    if (!tree) return m;
    const next = new Map(m);
    next.set(sessionId, toggleStackInTree(tree, focused));
    return next;
  });

  setLogicalFocus(focused);
}

/** Whether the focused pane has a parent split whose stacked state can be toggled. */
export function canToggleStack(sessionId: string): boolean {
  const focused = get(focusedPaneId);
  if (!focused) return false;
  const tree = get(sessionLayouts).get(sessionId);
  if (!tree) return false;

  const path: number[] = [];
  if (!buildSplitPath(tree, focused, path)) return false;
  return ancestorSplitDepths(tree, path).length > 0;
}

/** Switch the active stack tab within a stacked split. */
export function setActiveStackIndex(
  sessionId: string,
  index: number,
  stackPath?: readonly number[]
): void {
  const layouts = get(sessionLayouts);
  const tree = layouts.get(sessionId);
  if (!tree) return;

  let nextTree: LayoutNode | null = null;
  let newFocusTarget: LayoutNode | null = null;

  if (stackPath !== undefined) {
    const split = splitAtExactPath(tree, stackPath);
    if (!split || !split.stacked) return;

    const clamped = Math.max(0, Math.min(index, split.children.length - 1));
    newFocusTarget = split.children[clamped];
    if ((split.activeIndex ?? 0) !== clamped) {
      nextTree = replaceSplitAtPath(tree, stackPath, { ...split, stacked: true, activeIndex: clamped });
    }
  } else {
    const focused = get(focusedPaneId);
    if (!focused) return;

    const path: number[] = [];
    if (!buildSplitPath(tree, focused, path)) return;

    const splitDepths = ancestorSplitDepths(tree, path);
    let stackedDepth = -1;
    for (let i = splitDepths.length - 1; i >= 0; i--) {
      const split = splitAtPath(tree, path, splitDepths[i]);
      if (split.stacked) {
        stackedDepth = splitDepths[i];
        break;
      }
    }
    if (stackedDepth === -1) return;

    const split = splitAtPath(tree, path, stackedDepth);
    const clamped = Math.max(0, Math.min(index, split.children.length - 1));
    newFocusTarget = split.children[clamped];
    if ((split.activeIndex ?? 0) !== clamped) {
      nextTree = setStackedAtDepth(tree, path, stackedDepth, 0, true, clamped);
    }
  }

  if (nextTree) {
    const next = new Map(layouts);
    next.set(sessionId, nextTree);
    sessionLayouts.set(next);
  }

  if (newFocusTarget) {
    const targetPaneId = firstLeafId(newFocusTarget);
    if (targetPaneId !== get(focusedPaneId)) {
      setLogicalFocus(targetPaneId);
    }
  }
}

/** Move the focused pane in a direction (swap / enter / extract). */
export function movePaneInDirection(sessionId: string, direction: Direction): void {
  const tree = get(sessionLayouts).get(sessionId);
  if (!tree) return;
  const focused = get(focusedPaneId);
  if (!focused) return;

  const result = movePaneInTree(tree, focused, direction);
  if (!result) return;

  sessionLayouts.update((m) => {
    const next = new Map(m);
    next.set(sessionId, result);
    return next;
  });
  setLogicalFocus(focused);
}

/** Move a pane within the tree (swap, enter a split, or extract from nesting); returns null if not possible. */
function movePaneInTree(
  root: LayoutNode,
  paneId: string,
  direction: Direction
): LayoutNode | null {
  const path: PathEntry[] = [];
  if (!buildPath(root, paneId, path)) return null;

  const axis = directionAxis[direction];
  const step = directionStep[direction];

  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (parent.direction !== axis) continue;

    const targetIndex = childIndex + step;
    if (targetIndex < 0 || targetIndex >= parent.children.length) {
      continue; // at edge — walk up to extract
    }

    const isDirectChild = i === path.length - 1;
    const targetSibling = parent.children[targetIndex];

    if (isDirectChild && targetSibling.kind === "leaf") {
      // SWAP
      const newChildren = [...parent.children];
      newChildren[childIndex] = parent.children[targetIndex];
      newChildren[targetIndex] = parent.children[childIndex];
      return replaceSplitInTree(root, parent, { ...parent, children: newChildren });
    }

    if (isDirectChild && targetSibling.kind === "split") {
      // ENTER
      const focusedNode = parent.children[childIndex];
      const insertIdx = step > 0 ? 0 : targetSibling.children.length;
      const newTargetChildren = [...targetSibling.children];
      newTargetChildren.splice(insertIdx, 0, focusedNode);
      const updatedTarget: LayoutNode = { ...targetSibling, children: newTargetChildren };

      const newChildren = parent.children
        .map((c, j) => {
          if (j === childIndex) return null;
          if (j === targetIndex) return updatedTarget;
          return c;
        })
        .filter((c): c is LayoutNode => c !== null);

      if (newChildren.length === 1) {
        return replaceSplitInTree(root, parent, newChildren[0]);
      }
      return replaceSplitInTree(root, parent, { ...parent, children: newChildren });
    }

    if (!isDirectChild) {
      // EXTRACT
      const paneLeaf: LayoutNode = { kind: "leaf", paneId };

      const subtree = parent.children[childIndex];
      const subtreeAfterRemove = removeLeafFromSubtree(subtree, paneId);

      const newChildren: LayoutNode[] = [];
      let subtreePos = -1;
      for (let j = 0; j < parent.children.length; j++) {
        if (j === childIndex) {
          subtreePos = newChildren.length;
          if (subtreeAfterRemove) newChildren.push(subtreeAfterRemove);
        } else {
          newChildren.push(parent.children[j]);
        }
      }

      const insertPos =
        step > 0
          ? subtreeAfterRemove ? subtreePos + 1 : subtreePos
          : subtreePos;
      newChildren.splice(insertPos, 0, paneLeaf);

      return replaceSplitInTree(root, parent, { ...parent, children: newChildren });
    }
  }

  return null;
}

/** Move a pane (drag-and-drop) to a drop side of a target pane. */
export function movePane(
  sessionId: string,
  paneId: string,
  targetPaneId: string,
  side: DropSide
): void {
  if (paneId === targetPaneId) return;

  sessionLayouts.update((m) => {
    let tree = m.get(sessionId);
    if (!tree) return m;

    // 1. Remove pane from current position
    const afterRemove = removeLeafFromSubtree(tree, paneId);
    if (!afterRemove) return m;

    // 2. Verify target still exists
    if (!containsPaneId(afterRemove, targetPaneId)) return m;

    // 3. Insert at new position
    const direction = dropSideToDirection[side];
    const insertBefore = side === "left" || side === "top";
    const newLeaf: LayoutNode = { kind: "leaf", paneId };

    const afterInsert = insertLeafAtTarget(afterRemove, targetPaneId, direction, newLeaf, insertBefore);
    const next = new Map(m);
    next.set(sessionId, afterInsert);
    return next;
  });

  setLogicalFocus(paneId);
}

/** Resize the focused pane in the given direction by a step fraction. */
export function resizePane(sessionId: string, direction: Direction, step: number): void {
  const tree = get(sessionLayouts).get(sessionId);
  if (!tree) return;
  const focused = get(focusedPaneId);
  if (!focused) return;

  const path: PathEntry[] = [];
  if (!buildPath(tree, focused, path)) return;

  const axis = directionAxis[direction];
  const grow = directionStep[direction] * step;

  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (parent.direction !== axis) continue;

    const neighborIndex = grow > 0 ? childIndex + 1 : childIndex - 1;
    if (neighborIndex < 0 || neighborIndex >= parent.children.length) continue;

    const count = parent.children.length;
    const currentSizes = parent.sizes ?? Array(count).fill(1 / count);
    const newSizes = [...currentSizes];

    const delta = Math.abs(step);

    newSizes[childIndex] = Math.min(
      1 - MIN_PANE_FRACTION * (count - 1),
      newSizes[childIndex] + delta
    );
    newSizes[neighborIndex] = Math.max(MIN_PANE_FRACTION, newSizes[neighborIndex] - delta);

    const total = newSizes.reduce((a, b) => a + b, 0);
    for (let j = 0; j < newSizes.length; j++) newSizes[j] /= total;

    sessionLayouts.update((m) => {
      const root = m.get(sessionId);
      if (!root) return m;
      const next = new Map(m);
      next.set(sessionId, updateSplitByRef(root, parent, { sizes: newSizes }));
      return next;
    });
    return;
  }
}

/** Resize the two children adjacent to a rendered split divider. */
export function resizeSplitDivider(
  sessionId: string,
  splitPath: readonly number[],
  dividerIndex: number,
  deltaPx: number,
  containerPx: number
): void {
  if (
    !Number.isInteger(dividerIndex) ||
    !Number.isFinite(deltaPx) ||
    !Number.isFinite(containerPx) ||
    containerPx <= 0
  ) {
    return;
  }

  sessionLayouts.update((m) => {
    const root = m.get(sessionId);
    if (!root) return m;

    const split = splitAtExactPath(root, splitPath);
    if (!split || split.stacked) return m;

    const count = split.children.length;
    if (count < 2 || dividerIndex < 0 || dividerIndex >= count - 1) return m;

    const leftIndex = dividerIndex;
    const rightIndex = dividerIndex + 1;
    const sizes = normalizedSplitSizes(split);
    const pairTotal = sizes[leftIndex] + sizes[rightIndex];
    const minSize = Math.min(MIN_PANE_FRACTION, pairTotal / 2);
    const delta = deltaPx / containerPx;
    const nextLeft = Math.max(
      minSize,
      Math.min(pairTotal - minSize, sizes[leftIndex] + delta)
    );

    const newSizes = [...sizes];
    newSizes[leftIndex] = nextLeft;
    newSizes[rightIndex] = pairTotal - nextLeft;

    const total = newSizes.reduce((sum, size) => sum + size, 0);
    const normalized = total > 0 ? newSizes.map((size) => size / total) : newSizes;

    const next = new Map(m);
    next.set(sessionId, replaceSplitAtPath(root, splitPath, { ...split, sizes: normalized }));
    return next;
  });
}
