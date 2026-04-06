import { writable, get } from "svelte/store";

export type SplitDirection = "horizontal" | "vertical";

export interface Pane {
  id: string;
  type: "claude" | "shell" | "markdown" | "command";
  ptyId: string;
  docPath?: string;
  command?: string;
  workingDir?: string;
}

export type SplitNode =
  | { kind: "pane"; pane: Pane }
  | { kind: "split"; direction: SplitDirection; children: SplitNode[] };

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
  if (remaining.length === 1) return remaining[0];
  return { ...node, children: remaining };
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

export function navigatePane(sessionId: string, direction: Direction) {
  const tree = get(paneTrees).get(sessionId);
  if (!tree) return;
  const focused = get(focusedPaneId);
  if (!focused) return;

  const path: PathEntry[] = [];
  if (!buildPath(tree, focused, path)) return;

  const axis = directionAxis[direction];
  const step = directionStep[direction];

  // Walk up the path to find a split on the matching axis where we can move
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (parent.direction !== axis) continue;
    const nextIndex = childIndex + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) continue;
    // Found a valid move — descend into the target child, picking the near edge
    const target = parent.children[nextIndex];
    const newFocus = step > 0 ? firstPaneId(target) : lastPaneId(target);
    focusedPaneId.set(newFocus);
    return;
  }
}
