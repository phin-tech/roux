import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  sessionLayouts,
  initSessionLayout,
  insertLeaf,
  removeLeaf,
  firstLeafId,
  lastLeafId,
  collectLeafIds,
  collectVisibleLeafIds,
  hasSplitPanes,
  containsPaneId,
  resetLayouts,
  getLayout,
  navigatePane,
  toggleStack,
  getStackLabel,
  movePane,
  resizePane,
  resizeSplitDivider,
  type LayoutNode,
} from "../layout";
import { focusedPaneId, setLogicalFocus, resetFocus } from "../focus";
import { createPane, resetInstances } from "../instances";

function treeShape(node: LayoutNode): any {
  if (node.kind === "leaf") return node.paneId;
  return { dir: node.direction, children: node.children.map(treeShape) };
}

describe("layout tree", () => {
  beforeEach(() => {
    resetLayouts();
  });

  describe("initSessionLayout", () => {
    it("creates a single leaf for a new session", () => {
      initSessionLayout("s1", "s1-main");
      const tree = getLayout("s1");
      expect(tree.kind).toBe("leaf");
      if (tree.kind === "leaf") expect(tree.paneId).toBe("s1-main");
    });

    it("does not reinitialize if already exists", () => {
      initSessionLayout("s1", "s1-main");
      const tree1 = getLayout("s1");
      initSessionLayout("s1", "s1-other");
      const tree2 = getLayout("s1");
      expect(tree1).toEqual(tree2);
    });
  });

  describe("insertLeaf", () => {
    it("splits a leaf into two children", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toEqual({
        dir: "h",
        children: ["s1-main", "shell-1"],
      });
    });

    it("flattens same-direction splits into siblings", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "h", "shell-2");
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toEqual({
        dir: "h",
        children: ["s1-main", "shell-1", "shell-2"],
      });
    });

    it("nests different-direction splits", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "v", "shell-2");
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toEqual({
        dir: "h",
        children: [
          "s1-main",
          { dir: "v", children: ["shell-1", "shell-2"] },
        ],
      });
    });
  });

  describe("removeLeaf", () => {
    it("collapses a split back to a single leaf", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1");
        t = removeLeaf(t, "shell-1")!;
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toBe("s1-main");
    });

    it("returns null when removing the only leaf", () => {
      const result = removeLeaf({ kind: "leaf", paneId: "p1" }, "p1");
      expect(result).toBeNull();
    });

    it("preserves other children when removing from a 3-child split", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "h", "shell-2");
        t = removeLeaf(t, "shell-1")!;
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toEqual({
        dir: "h",
        children: ["s1-main", "shell-2"],
      });
    });

    it("clamps activeIndex on stacked splits", () => {
      const tree: LayoutNode = {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "p1" },
          { kind: "leaf", paneId: "p2" },
          { kind: "leaf", paneId: "p3" },
        ],
        stacked: true,
        activeIndex: 2,
      };
      const result = removeLeaf(tree, "p3")!;
      if (result.kind === "split") {
        expect(result.activeIndex).toBe(1);
      }
    });

    it("adjusts sizes array when removing a child", () => {
      const tree: LayoutNode = {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "p1" },
          { kind: "leaf", paneId: "p2" },
          { kind: "leaf", paneId: "p3" },
        ],
        sizes: [0.25, 0.5, 0.25],
      };
      const result = removeLeaf(tree, "p2")!;
      if (result.kind === "split") {
        expect(result.sizes).toBeDefined();
        const total = result.sizes!.reduce((a, b) => a + b, 0);
        expect(total).toBeCloseTo(1);
      }
    });
  });

  describe("helpers", () => {
    it("firstLeafId returns the leftmost leaf", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      expect(firstLeafId(getLayout("s1"))).toBe("s1-main");
    });

    it("lastLeafId returns the rightmost leaf", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      expect(lastLeafId(getLayout("s1"))).toBe("shell-1");
    });

    it("collectVisibleLeafIds returns DFS order for a plain split tree", () => {
      const tree: LayoutNode = {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "a" },
          {
            kind: "split",
            direction: "v",
            children: [
              { kind: "leaf", paneId: "b" },
              { kind: "leaf", paneId: "c" },
            ],
          },
          { kind: "leaf", paneId: "d" },
        ],
      };
      expect(collectVisibleLeafIds(tree)).toEqual(["a", "b", "c", "d"]);
    });

    it("collectVisibleLeafIds skips hidden children of stacked splits", () => {
      const tree: LayoutNode = {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "a" },
          {
            kind: "split",
            direction: "h",
            stacked: true,
            activeIndex: 1,
            children: [
              { kind: "leaf", paneId: "hidden-1" },
              { kind: "leaf", paneId: "visible" },
              { kind: "leaf", paneId: "hidden-2" },
            ],
          },
          { kind: "leaf", paneId: "d" },
        ],
      };
      expect(collectVisibleLeafIds(tree)).toEqual(["a", "visible", "d"]);
    });

    it("collectVisibleLeafIds defaults stacked activeIndex to 0", () => {
      const tree: LayoutNode = {
        kind: "split",
        direction: "h",
        stacked: true,
        children: [
          { kind: "leaf", paneId: "first" },
          { kind: "leaf", paneId: "second" },
        ],
      };
      expect(collectVisibleLeafIds(tree)).toEqual(["first"]);
    });

    it("collectLeafIds returns all leaf IDs", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "v", "shell-2");
        m.set("s1", t);
        return new Map(m);
      });
      expect(collectLeafIds(getLayout("s1")).sort()).toEqual(
        ["s1-main", "shell-1", "shell-2"].sort()
      );
    });

    it("hasSplitPanes returns false for single leaf", () => {
      initSessionLayout("s1", "s1-main");
      expect(hasSplitPanes("s1")).toBe(false);
    });

    it("hasSplitPanes returns true for split", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      expect(hasSplitPanes("s1")).toBe(true);
    });

    it("containsPaneId searches recursively", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      const tree = getLayout("s1");
      expect(containsPaneId(tree, "shell-1")).toBe(true);
      expect(containsPaneId(tree, "nope")).toBe(false);
    });
  });
});

describe("navigatePane", () => {
  beforeEach(() => {
    resetLayouts();
    resetFocus();
    resetInstances();
  });

  it("navigates right in a horizontal split", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p1");
    navigatePane("s1", "right");
    expect(get(focusedPaneId)).toBe("p2");
  });

  it("navigates left", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p2");
    navigatePane("s1", "left");
    expect(get(focusedPaneId)).toBe("p1");
  });

  it("does nothing at edge", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p2");
    navigatePane("s1", "right");
    expect(get(focusedPaneId)).toBe("p2");
  });

  it("navigates down in a vertical split", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "v", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p1");
    navigatePane("s1", "down");
    expect(get(focusedPaneId)).toBe("p2");
  });
});

describe("toggleStack", () => {
  beforeEach(() => {
    resetLayouts();
    resetFocus();
    resetInstances();
  });

  it("stacks the parent split of the focused pane", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p1");
    toggleStack("s1");
    const tree = getLayout("s1");
    if (tree.kind === "split") {
      expect(tree.stacked).toBe(true);
      expect(tree.activeIndex).toBe(0);
    }
  });

  it("unstacks when already stacked", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p1");
    toggleStack("s1");
    toggleStack("s1");
    const tree = getLayout("s1");
    if (tree.kind === "split") {
      expect(tree.stacked).toBeFalsy();
    }
  });
});

describe("resizePane", () => {
  beforeEach(() => {
    resetLayouts();
    resetFocus();
  });

  it("grows the focused pane to the right", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p1");
    resizePane("s1", "right", 0.05);
    const tree = getLayout("s1");
    if (tree.kind === "split") {
      expect(tree.sizes).toBeDefined();
      expect(tree.sizes![0]).toBeGreaterThan(0.5);
    }
  });
});

describe("resizeSplitDivider", () => {
  beforeEach(() => {
    resetLayouts();
    resetFocus();
  });

  it("resizes adjacent children in a two-pane split", () => {
    sessionLayouts.set(new Map([
      ["s1", {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "p1" },
          { kind: "leaf", paneId: "p2" },
        ],
      }],
    ]));

    resizeSplitDivider("s1", [], 0, 100, 1000);

    const tree = getLayout("s1");
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.sizes).toEqual([0.6, 0.4]);
    }
  });

  it("only changes the adjacent pair in a multi-child split", () => {
    sessionLayouts.set(new Map([
      ["s1", {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "p1" },
          { kind: "leaf", paneId: "p2" },
          { kind: "leaf", paneId: "p3" },
        ],
        sizes: [0.2, 0.5, 0.3],
      }],
    ]));

    resizeSplitDivider("s1", [], 1, -100, 1000);

    const tree = getLayout("s1");
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.sizes).toEqual([0.2, 0.4, 0.4]);
    }
  });

  it("clamps adjacent children to the minimum size", () => {
    sessionLayouts.set(new Map([
      ["s1", {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "p1" },
          { kind: "leaf", paneId: "p2" },
        ],
        sizes: [0.5, 0.5],
      }],
    ]));

    resizeSplitDivider("s1", [], 0, 900, 1000);

    const tree = getLayout("s1");
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.sizes![0]).toBeCloseTo(0.95);
      expect(tree.sizes![1]).toBeCloseTo(0.05);
    }
  });

  it("resizes a nested split by path without changing the root split", () => {
    sessionLayouts.set(new Map([
      ["s1", {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "p1" },
          {
            kind: "split",
            direction: "v",
            children: [
              { kind: "leaf", paneId: "p2" },
              { kind: "leaf", paneId: "p3" },
            ],
          },
        ],
        sizes: [0.3, 0.7],
      }],
    ]));

    resizeSplitDivider("s1", [1], 0, 50, 500);

    const tree = getLayout("s1");
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.sizes).toEqual([0.3, 0.7]);
      const nested = tree.children[1];
      expect(nested.kind).toBe("split");
      if (nested.kind === "split") {
        expect(nested.sizes).toEqual([0.6, 0.4]);
      }
    }
  });

  it("does nothing for invalid divider inputs", () => {
    const tree: LayoutNode = {
      kind: "split",
      direction: "h",
      children: [
        { kind: "leaf", paneId: "p1" },
        { kind: "leaf", paneId: "p2" },
      ],
      sizes: [0.25, 0.75],
    };
    sessionLayouts.set(new Map([["s1", tree]]));

    resizeSplitDivider("s1", [], 1, 100, 1000);
    resizeSplitDivider("s1", [9], 0, 100, 1000);
    resizeSplitDivider("s1", [], 0, 100, 0);

    expect(getLayout("s1")).toEqual(tree);
  });
});

describe("getStackLabel", () => {
  beforeEach(() => {
    resetLayouts();
    resetFocus();
    resetInstances();
  });

  it("returns pane name for leaf", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1", name: "my-shell" });
    const node: LayoutNode = { kind: "leaf", paneId: "p1" };
    expect(getStackLabel(node)).toBe("my-shell");
  });

  it("falls back to type for unnamed leaf", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    const node: LayoutNode = { kind: "leaf", paneId: "p1" };
    expect(getStackLabel(node)).toBe("shell");
  });
});

describe("movePane (drag-and-drop)", () => {
  beforeEach(() => {
    resetLayouts();
    resetFocus();
  });

  it("moves a pane to the right of another", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      let t = getLayout("s1");
      t = insertLeaf(t, "p1", "h", "p2");
      t = insertLeaf(t, "p2", "h", "p3");
      m.set("s1", t);
      return new Map(m);
    });
    movePane("s1", "p3", "p1", "right");
    const ids = collectLeafIds(getLayout("s1"));
    expect(ids).toContain("p1");
    expect(ids).toContain("p2");
    expect(ids).toContain("p3");
  });
});
