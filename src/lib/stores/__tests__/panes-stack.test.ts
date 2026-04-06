import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  initSessionPanes,
  addSplit,
  removePane,
  toggleStack,
  setActiveStackIndex,
  getStackLabel,
  navigatePane,
  type SplitNode,
  type Pane,
} from "../panes";

function getTree(sessionId: string): SplitNode {
  return get(paneTrees).get(sessionId)!;
}

function asSplit(node: SplitNode) {
  if (node.kind !== "split") throw new Error("Expected split node");
  return node;
}

describe("stacked panes", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  describe("toggleStack", () => {
    it("stacks the immediate parent split of the focused pane", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");

      toggleStack("s1");

      const tree = asSplit(getTree("s1"));
      expect(tree.stacked).toBe(true);
      expect(tree.activeIndex).toBe(0);
    });

    it("cycling: second press stacks the next ancestor split", () => {
      // Build: root horizontal split -> [claude, vertical split -> [shell-1, shell-2]]
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });

      // Focus shell-1, toggle once -> stacks the vertical split (shell-1's parent)
      focusedPaneId.set("shell-1");
      toggleStack("s1");

      const innerSplit = asSplit(asSplit(getTree("s1")).children[1]);
      expect(innerSplit.stacked).toBe(true);

      // Toggle again -> unstacks inner, stacks the root horizontal split
      toggleStack("s1");

      const root = asSplit(getTree("s1"));
      expect(root.stacked).toBe(true);
      // Inner should be unstacked
      const inner2 = asSplit(root.children[1]);
      expect(inner2.stacked).toBeFalsy();
    });

    it("cycling: third press unstacks everything", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });
      focusedPaneId.set("shell-1");

      toggleStack("s1"); // stack inner
      toggleStack("s1"); // stack outer
      toggleStack("s1"); // unstack all

      const root = asSplit(getTree("s1"));
      expect(root.stacked).toBeFalsy();
    });

    it("does nothing when focused pane has no parent split", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");

      toggleStack("s1"); // root is a single pane, no split to stack

      const tree = getTree("s1");
      expect(tree.kind).toBe("pane");
    });

    it("sets activeIndex to the child containing the focused pane", () => {
      // Build: root split(h) [pane(s1-main), split(h) [pane(shell-1), pane(shell-2)]]
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      // focus is now on shell-1; second split nests under shell-1's parent
      addSplit("s1", "horizontal", { id: "shell-2", type: "shell", ptyId: "pty-2" });
      focusedPaneId.set("shell-1");

      toggleStack("s1");

      // shell-1's immediate parent is the inner split [shell-1, shell-2]
      const root = asSplit(getTree("s1"));
      const innerSplit = asSplit(root.children[1]);
      expect(innerSplit.stacked).toBe(true);
      // shell-1 is child index 0 in the inner split
      expect(innerSplit.activeIndex).toBe(0);
    });
  });

  describe("setActiveStackIndex", () => {
    it("sets the active index on the nearest stacked ancestor", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      setActiveStackIndex("s1", 1);

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1);
    });

    it("clamps index to valid range", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      setActiveStackIndex("s1", 99);

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1); // clamped to last valid index
    });
  });

  describe("getStackLabel", () => {
    it("returns pane name for a leaf node", () => {
      const node: SplitNode = { kind: "pane", pane: { id: "p1", type: "shell", ptyId: "", name: "my shell" } };
      expect(getStackLabel(node)).toBe("my shell");
    });

    it("returns pane type label when no name", () => {
      const node: SplitNode = { kind: "pane", pane: { id: "p1", type: "shell", ptyId: "" } };
      expect(getStackLabel(node)).toBe("shell");
    });

    it("joins leaf names with | for splits", () => {
      const node: SplitNode = {
        kind: "split",
        direction: "horizontal",
        children: [
          { kind: "pane", pane: { id: "p1", type: "shell", ptyId: "", name: "P1" } },
          { kind: "pane", pane: { id: "p2", type: "claude", ptyId: "" } },
        ],
      };
      expect(getStackLabel(node)).toBe("P1 | claude");
    });
  });

  describe("navigation in stacks", () => {
    it("alt+j (down) cycles to next tab in a stacked split", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      addSplit("s1", "horizontal", { id: "shell-2", type: "shell", ptyId: "pty-2" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      // activeIndex is 0 (s1-main). Navigate down should go to index 1
      navigatePane("s1", "down");

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1);
      expect(get(focusedPaneId)).toBe("shell-1");
    });

    it("alt+k (up) cycles to previous tab in a stacked split", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      setActiveStackIndex("s1", 1);
      focusedPaneId.set("shell-1");

      navigatePane("s1", "up");

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(0);
      expect(get(focusedPaneId)).toBe("s1-main");
    });

    it("does not cycle past the last tab", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      toggleStack("s1");

      // Already at last tab (index 1), navigate down should do nothing
      navigatePane("s1", "down");

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1);
    });

    it("alt+h/alt+l navigate out of the stack spatially", () => {
      // Build: root horizontal -> [claude pane, stacked vertical -> [shell-1, shell-2]]
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });

      // Stack the vertical split (shell-1's parent)
      focusedPaneId.set("shell-1");
      toggleStack("s1");

      // Navigate left should go to the claude pane (spatial, not tab)
      navigatePane("s1", "left");
      expect(get(focusedPaneId)).toBe("s1-main");
    });
  });

  describe("auto-unstack on remove", () => {
    it("auto-unstacks when stack drops to 1 child", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      expect(asSplit(getTree("s1")).stacked).toBe(true);

      removePane("s1", "shell-1");

      const tree = getTree("s1");
      expect(tree.kind).toBe("pane");
    });

    it("clamps activeIndex when removing a child from a stacked split", () => {
      // Build: root split(h) [pane(s1-main), split(h) [pane(shell-1), pane(shell-2)]]
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      addSplit("s1", "horizontal", { id: "shell-2", type: "shell", ptyId: "pty-2" });

      // Stack the root split with activeIndex pointing to child 1 (inner split)
      focusedPaneId.set("shell-1");
      toggleStack("s1"); // stacks inner split [shell-1, shell-2]
      toggleStack("s1"); // stacks root split [s1-main, inner-split]

      const rootBefore = asSplit(getTree("s1"));
      expect(rootBefore.stacked).toBe(true);
      // activeIndex should be 1 (the inner split containing shell-1)
      expect(rootBefore.activeIndex).toBe(1);

      // Remove shell-2 — inner split collapses to shell-1, root still has 2 children
      removePane("s1", "shell-2");

      const tree = asSplit(getTree("s1"));
      expect(tree.stacked).toBe(true);
      expect(tree.activeIndex).toBe(1);
      expect(tree.children).toHaveLength(2);
    });
  });
});
