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
  hasSplitPanes,
  containsPaneId,
  resetLayouts,
  type LayoutNode,
} from "../layout";

function getLayout(sessionId: string): LayoutNode {
  return get(sessionLayouts).get(sessionId)!;
}

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
