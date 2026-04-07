import { writable, get } from "svelte/store";

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
