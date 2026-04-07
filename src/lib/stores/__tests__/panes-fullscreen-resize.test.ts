import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  fullscreenPaneId,
  toggleFullscreen,
  resizePane,
  removePane,
  containsPaneId,
  type SplitNode,
} from "../panes";

function pane(id: string): SplitNode {
  return { kind: "pane", pane: { id, type: "shell", ptyId: `pty-${id}` } };
}

function hsplit(...children: SplitNode[]): SplitNode {
  return { kind: "split", direction: "horizontal", children };
}

function vsplit(...children: SplitNode[]): SplitNode {
  return { kind: "split", direction: "vertical", children };
}

function setTree(sessionId: string, tree: SplitNode) {
  paneTrees.update((trees) => {
    trees.set(sessionId, tree);
    return new Map(trees);
  });
}

function getTree(sessionId: string) {
  return get(paneTrees).get(sessionId)!;
}

describe("containsPaneId", () => {
  it("finds a pane in a flat split", () => {
    const tree = hsplit(pane("A"), pane("B"));
    expect(containsPaneId(tree, "A")).toBe(true);
    expect(containsPaneId(tree, "C")).toBe(false);
  });

  it("finds a pane in nested splits", () => {
    const tree = hsplit(pane("A"), vsplit(pane("B"), pane("C")));
    expect(containsPaneId(tree, "C")).toBe(true);
  });
});

describe("toggleFullscreen", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    fullscreenPaneId.set(null);
  });

  it("sets fullscreenPaneId to the focused pane", () => {
    setTree("s1", hsplit(pane("A"), pane("B")));
    focusedPaneId.set("A");

    toggleFullscreen();

    expect(get(fullscreenPaneId)).toBe("A");
  });

  it("toggles off when called again", () => {
    setTree("s1", hsplit(pane("A"), pane("B")));
    focusedPaneId.set("A");

    toggleFullscreen();
    toggleFullscreen();

    expect(get(fullscreenPaneId)).toBeNull();
  });

  it("does nothing with no focused pane", () => {
    toggleFullscreen();
    expect(get(fullscreenPaneId)).toBeNull();
  });
});

describe("resizePane", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  it("grows the focused pane to the right", () => {
    setTree("s1", hsplit(pane("A"), pane("B")));
    focusedPaneId.set("A");

    resizePane("s1", "right", 0.05);

    const tree = getTree("s1") as SplitNode & { kind: "split" };
    expect(tree.sizes).toBeDefined();
    expect(tree.sizes![0]).toBeGreaterThan(tree.sizes![1]);
  });

  it("grows the focused pane downward", () => {
    setTree("s1", vsplit(pane("A"), pane("B")));
    focusedPaneId.set("A");

    resizePane("s1", "down", 0.05);

    const tree = getTree("s1") as SplitNode & { kind: "split" };
    expect(tree.sizes![0]).toBeGreaterThan(tree.sizes![1]);
  });

  it("shrinks the focused pane when resizing toward it", () => {
    setTree("s1", hsplit(pane("A"), pane("B")));
    focusedPaneId.set("B");

    // Resize B to the left = grow B, shrink A
    resizePane("s1", "left", 0.05);

    const tree = getTree("s1") as SplitNode & { kind: "split" };
    expect(tree.sizes![1]).toBeGreaterThan(tree.sizes![0]);
  });

  it("does not go below minimum size", () => {
    setTree("s1", hsplit(pane("A"), pane("B")));
    focusedPaneId.set("A");

    // Resize many times to try to make B tiny
    for (let i = 0; i < 50; i++) {
      resizePane("s1", "right", 0.05);
    }

    const tree = getTree("s1") as SplitNode & { kind: "split" };
    expect(tree.sizes![1]).toBeGreaterThan(0.01);
  });

  it("no-ops when at the edge with no neighbor", () => {
    setTree("s1", hsplit(pane("A"), pane("B")));
    focusedPaneId.set("B");

    resizePane("s1", "right", 0.05);

    const tree = getTree("s1") as SplitNode & { kind: "split" };
    expect(tree.sizes).toBeUndefined();
  });

  it("sizes are preserved and renormalized when a pane is removed", () => {
    setTree("s1", {
      kind: "split",
      direction: "horizontal",
      children: [pane("A"), pane("B"), pane("C")],
      sizes: [0.5, 0.25, 0.25],
    });

    removePane("s1", "B");

    const tree = getTree("s1") as SplitNode & { kind: "split" };
    expect(tree.sizes).toBeDefined();
    expect(tree.sizes!.length).toBe(2);
    // A was 0.5, C was 0.25 → renormalized: A=0.667, C=0.333
    expect(tree.sizes![0]).toBeCloseTo(0.667, 2);
    expect(tree.sizes![1]).toBeCloseTo(0.333, 2);
  });
});
