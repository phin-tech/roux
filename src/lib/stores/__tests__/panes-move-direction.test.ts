import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  movePaneInDirection,
  type SplitNode,
} from "../panes";

function treeShape(node: SplitNode): any {
  if (node.kind === "pane") return node.pane.id;
  return { dir: node.direction, children: node.children.map(treeShape) };
}

function getTree(sessionId: string) {
  return get(paneTrees).get(sessionId)!;
}

/** Set up a tree directly for precise control over structure. */
function setTree(sessionId: string, tree: SplitNode) {
  paneTrees.update((trees) => {
    trees.set(sessionId, tree);
    return new Map(trees);
  });
}

function pane(id: string): SplitNode {
  return { kind: "pane", pane: { id, type: "shell", ptyId: `pty-${id}` } };
}

function hsplit(...children: SplitNode[]): SplitNode {
  return { kind: "split", direction: "horizontal", children };
}

function vsplit(...children: SplitNode[]): SplitNode {
  return { kind: "split", direction: "vertical", children };
}

describe("movePaneInDirection", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  // ── Case 1: Swap ──────────────────────────────────────────

  describe("swap (direct child, target is a pane)", () => {
    it("swaps two adjacent panes moving right", () => {
      // H:[A, B, C] → focus B, move right → H:[A, C, B]
      setTree("s1", hsplit(pane("A"), pane("B"), pane("C")));
      focusedPaneId.set("B");

      movePaneInDirection("s1", "right");

      expect(treeShape(getTree("s1"))).toEqual({
        dir: "horizontal",
        children: ["A", "C", "B"],
      });
      expect(get(focusedPaneId)).toBe("B");
    });

    it("swaps two adjacent panes moving left", () => {
      // H:[A, B, C] → focus B, move left → H:[B, A, C]
      setTree("s1", hsplit(pane("A"), pane("B"), pane("C")));
      focusedPaneId.set("B");

      movePaneInDirection("s1", "left");

      expect(treeShape(getTree("s1"))).toEqual({
        dir: "horizontal",
        children: ["B", "A", "C"],
      });
    });

    it("swaps panes vertically", () => {
      // V:[A, B] → focus A, move down → V:[B, A]
      setTree("s1", vsplit(pane("A"), pane("B")));
      focusedPaneId.set("A");

      movePaneInDirection("s1", "down");

      expect(treeShape(getTree("s1"))).toEqual({
        dir: "vertical",
        children: ["B", "A"],
      });
    });
  });

  // ── Case 2: Enter ─────────────────────────────────────────

  describe("enter (direct child, target is a split)", () => {
    it("enters an adjacent split moving right", () => {
      // H:[Cmd, V:[A, B]] → focus Cmd, move right → H:[V:[Cmd, A, B]]
      // After auto-collapse: V:[Cmd, A, B]
      setTree("s1", hsplit(pane("Cmd"), vsplit(pane("A"), pane("B"))));
      focusedPaneId.set("Cmd");

      movePaneInDirection("s1", "right");

      // After removing Cmd from H, H has only V:[A,B] left, so it collapses.
      // Then Cmd is inserted into V at index 0.
      // But the collapse happens first during removePaneFromTree,
      // so we need to insert into the surviving V split.
      const shape = treeShape(getTree("s1"));
      expect(shape).toEqual({
        dir: "vertical",
        children: ["Cmd", "A", "B"],
      });
    });

    it("enters an adjacent split moving left (inserts at end)", () => {
      // H:[V:[A, B], Cmd] → focus Cmd, move left → V:[A, B, Cmd]
      setTree("s1", hsplit(vsplit(pane("A"), pane("B")), pane("Cmd")));
      focusedPaneId.set("Cmd");

      movePaneInDirection("s1", "left");

      const shape = treeShape(getTree("s1"));
      expect(shape).toEqual({
        dir: "vertical",
        children: ["A", "B", "Cmd"],
      });
    });

    it("enters a split in a three-column layout", () => {
      // H:[Claude, Cmd, V:[A, B]] → focus Cmd, move right → H:[Claude, V:[Cmd, A, B]]
      setTree("s1", hsplit(pane("Claude"), pane("Cmd"), vsplit(pane("A"), pane("B"))));
      focusedPaneId.set("Cmd");

      movePaneInDirection("s1", "right");

      const shape = treeShape(getTree("s1"));
      expect(shape).toEqual({
        dir: "horizontal",
        children: ["Claude", { dir: "vertical", children: ["Cmd", "A", "B"] }],
      });
    });
  });

  // ── Case 3: Extract ───────────────────────────────────────

  describe("extract (nested child, moves out to ancestor)", () => {
    it("extracts a pane from a nested split moving left", () => {
      // H:[Claude, V:[A, B, C]] → focus A, move left → H:[Claude, A, V:[B, C]]
      setTree("s1", hsplit(pane("Claude"), vsplit(pane("A"), pane("B"), pane("C"))));
      focusedPaneId.set("A");

      movePaneInDirection("s1", "left");

      const shape = treeShape(getTree("s1"));
      expect(shape).toEqual({
        dir: "horizontal",
        children: ["Claude", "A", { dir: "vertical", children: ["B", "C"] }],
      });
    });

    it("extracts a pane from a nested split moving right", () => {
      // H:[V:[A, B], Claude] → focus B, move right → H:[V:[A], B, Claude]
      // V:[A] auto-collapses to A → H:[A, B, Claude]
      setTree("s1", hsplit(vsplit(pane("A"), pane("B")), pane("Claude")));
      focusedPaneId.set("B");

      movePaneInDirection("s1", "right");

      const shape = treeShape(getTree("s1"));
      expect(shape).toEqual({
        dir: "horizontal",
        children: ["A", "B", "Claude"],
      });
    });
  });

  // ── No-ops ────────────────────────────────────────────────

  describe("no-ops", () => {
    it("does nothing at the edge of the tree", () => {
      setTree("s1", hsplit(pane("A"), pane("B")));
      focusedPaneId.set("B");

      movePaneInDirection("s1", "right");

      expect(treeShape(getTree("s1"))).toEqual({
        dir: "horizontal",
        children: ["A", "B"],
      });
    });

    it("does nothing for a single pane", () => {
      setTree("s1", pane("A"));
      focusedPaneId.set("A");

      movePaneInDirection("s1", "right");

      expect(treeShape(getTree("s1"))).toEqual("A");
    });

    it("does nothing for wrong axis with no matching ancestor", () => {
      // H:[A, B] → focus A, move up → no vertical ancestor, no-op
      setTree("s1", hsplit(pane("A"), pane("B")));
      focusedPaneId.set("A");

      movePaneInDirection("s1", "up");

      expect(treeShape(getTree("s1"))).toEqual({
        dir: "horizontal",
        children: ["A", "B"],
      });
    });
  });

  // ── Focus preservation ────────────────────────────────────

  it("preserves focus on the moved pane", () => {
    setTree("s1", hsplit(pane("A"), pane("B"), pane("C")));
    focusedPaneId.set("B");

    movePaneInDirection("s1", "right");

    expect(get(focusedPaneId)).toBe("B");
  });
});
