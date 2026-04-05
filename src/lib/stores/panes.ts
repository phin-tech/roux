import { writable, get } from "svelte/store";

export type SplitDirection = "horizontal" | "vertical";

export interface Pane {
  id: string;
  type: "claude" | "shell" | "doc";
  ptyId: string;
  docPath?: string;
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
