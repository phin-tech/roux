import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  initSessionPanes,
  addSplit,
  toggleStack,
  setActiveStackIndex,
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
});
