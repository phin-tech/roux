import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  initSessionPanes,
  addSplit,
  removePane,
  removeSessionPanes,
  hasSplitPanes,
  type SplitNode,
  type Pane,
} from "../panes";

describe("panes store", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  it("initializes session with a single claude pane", () => {
    initSessionPanes("session-1");

    const tree = get(paneTrees).get("session-1");
    expect(tree).toBeDefined();
    expect(tree!.kind).toBe("pane");
    if (tree!.kind === "pane") {
      expect(tree!.pane.type).toBe("claude");
      expect(tree!.pane.id).toBe("session-1-main");
      expect(tree!.pane.ptyId).toBe("session-1");
    }
  });

  it("does not reinitialize if already exists", () => {
    initSessionPanes("session-1");
    const tree1 = get(paneTrees).get("session-1");

    initSessionPanes("session-1");
    const tree2 = get(paneTrees).get("session-1");

    expect(tree1).toEqual(tree2);
  });

  it("splits a pane horizontally", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    const newPane: Pane = { id: "shell-1", type: "shell", ptyId: "pty-1" };
    addSplit("s1", "horizontal", newPane);

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("horizontal");
      expect(tree.children).toHaveLength(2);
      expect(tree.children[0].kind).toBe("pane");
      expect(tree.children[1].kind).toBe("pane");
      if (tree.children[1].kind === "pane") {
        expect(tree.children[1].pane.id).toBe("shell-1");
      }
    }
  });

  it("splits a pane vertically", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    addSplit("s1", "vertical", { id: "shell-1", type: "shell", ptyId: "pty-1" });

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("vertical");
    }
  });

  it("sets focused pane after split", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });

    expect(get(focusedPaneId)).toBe("shell-1");
  });

  it("handles nested splits", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");
    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });

    // Focus the new shell pane, then split it vertically
    focusedPaneId.set("shell-1");
    addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      // First child is the original claude pane
      expect(tree.children[0].kind).toBe("pane");
      // Second child is now a nested split (shell-1 + shell-2)
      expect(tree.children[1].kind).toBe("split");
      if (tree.children[1].kind === "split") {
        expect(tree.children[1].direction).toBe("vertical");
        expect(tree.children[1].children).toHaveLength(2);
      }
    }
  });

  it("removes a pane from a split", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });

    removePane("s1", "shell-1");

    const tree = get(paneTrees).get("s1")!;
    // Should collapse back to a single pane
    expect(tree.kind).toBe("pane");
    if (tree.kind === "pane") {
      expect(tree.pane.id).toBe("s1-main");
    }
  });

  it("removes a nested pane correctly", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
    focusedPaneId.set("shell-1");
    addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });

    // Remove shell-1, shell-2 should remain alongside main
    removePane("s1", "shell-1");

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.children).toHaveLength(2);
    }
  });

  it("removes session panes entirely", () => {
    initSessionPanes("s1");
    initSessionPanes("s2");

    removeSessionPanes("s1");

    expect(get(paneTrees).has("s1")).toBe(false);
    expect(get(paneTrees).has("s2")).toBe(true);
  });

  it("hasSplitPanes returns false for single pane", () => {
    initSessionPanes("s1");
    expect(hasSplitPanes("s1")).toBe(false);
  });

  it("hasSplitPanes returns true after splitting", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
    expect(hasSplitPanes("s1")).toBe(true);
  });

  it("hasSplitPanes returns false after removing all splits", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
    removePane("s1", "shell-1");
    expect(hasSplitPanes("s1")).toBe(false);
  });

  it("supports doc pane type", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", {
      id: "doc-1",
      type: "doc",
      ptyId: "",
      docPath: "/tmp/plan.md",
    });

    const tree = get(paneTrees).get("s1")!;
    if (tree.kind === "split") {
      const docPane = tree.children[1];
      if (docPane.kind === "pane") {
        expect(docPane.pane.type).toBe("doc");
        expect(docPane.pane.docPath).toBe("/tmp/plan.md");
      }
    }
  });
});
