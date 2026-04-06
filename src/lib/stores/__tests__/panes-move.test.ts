import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  initSessionPanes,
  addSplit,
  movePane,
  getPane,
  listPanes,
  type Pane,
  type SplitNode,
} from "../panes";

const shell = (id: string): Pane => ({ id, type: "shell", ptyId: `pty-${id}` });

function treeShape(node: SplitNode): any {
  if (node.kind === "pane") return node.pane.id;
  return { dir: node.direction, children: node.children.map(treeShape) };
}

function getTree(sessionId: string) {
  return get(paneTrees).get(sessionId)!;
}

describe("movePane", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  // ── Basic moves ───────────────────────────────────────────

  it("moves a pane to the right of another", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");
    addSplit("s1", "horizontal", shell("sh1"));

    // sh1 is right of main — move it to the left instead
    movePane("s1", "sh1", "s1-main", "left");

    const shape = treeShape(getTree("s1"));
    expect(shape).toEqual({
      dir: "horizontal",
      children: ["sh1", "s1-main"],
    });
  });

  it("moves a pane below another", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");
    addSplit("s1", "horizontal", shell("sh1"));

    movePane("s1", "sh1", "s1-main", "bottom");

    const shape = treeShape(getTree("s1"));
    expect(shape).toEqual({
      dir: "vertical",
      children: ["s1-main", "sh1"],
    });
  });

  it("moves a pane above another", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");
    addSplit("s1", "vertical", shell("sh1"));

    movePane("s1", "sh1", "s1-main", "top");

    const shape = treeShape(getTree("s1"));
    expect(shape).toEqual({
      dir: "vertical",
      children: ["sh1", "s1-main"],
    });
  });

  // ── Edge cases ────────────────────────────────────────────

  it("no-ops when dropping a pane onto itself", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", shell("sh1"));
    const before = treeShape(getTree("s1"));

    movePane("s1", "sh1", "sh1", "left");

    expect(treeShape(getTree("s1"))).toEqual(before);
  });

  it("no-ops for a nonexistent session", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", shell("sh1"));
    const before = treeShape(getTree("s1"));

    movePane("ghost", "sh1", "s1-main", "left");

    expect(treeShape(getTree("s1"))).toEqual(before);
  });

  it("no-ops for a nonexistent pane", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", shell("sh1"));
    const before = treeShape(getTree("s1"));

    movePane("s1", "ghost", "s1-main", "left");

    expect(treeShape(getTree("s1"))).toEqual(before);
  });

  // ── Preserves data ────────────────────────────────────────

  it("preserves pane properties through the move", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", {
      id: "doc1",
      type: "markdown",
      ptyId: "",
      name: "my notes",
      docPath: "/tmp/notes.md",
    });

    movePane("s1", "doc1", "s1-main", "bottom");

    const pane = getPane("s1", "doc1");
    expect(pane?.name).toBe("my notes");
    expect(pane?.type).toBe("markdown");
    expect(pane?.docPath).toBe("/tmp/notes.md");
  });

  it("sets focus to the moved pane", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", shell("sh1"));
    focusedPaneId.set("s1-main");

    movePane("s1", "sh1", "s1-main", "left");

    expect(get(focusedPaneId)).toBe("sh1");
  });

  // ── Three-pane layouts ────────────────────────────────────

  it("moves across a three-pane layout", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");
    addSplit("s1", "horizontal", shell("sh1"));
    focusedPaneId.set("sh1");
    addSplit("s1", "horizontal", shell("sh2"));

    // Move sh2 to the left of main
    movePane("s1", "sh2", "s1-main", "left");

    const paneIds = listPanes("s1").map((p) => p.id);
    expect(paneIds).toContain("s1-main");
    expect(paneIds).toContain("sh1");
    expect(paneIds).toContain("sh2");
    expect(paneIds).toHaveLength(3);
  });

  it("retains all panes after multiple moves", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");
    addSplit("s1", "horizontal", shell("sh1"));
    focusedPaneId.set("sh1");
    addSplit("s1", "vertical", shell("sh2"));

    movePane("s1", "sh2", "s1-main", "right");
    movePane("s1", "sh1", "sh2", "bottom");

    const paneIds = listPanes("s1").map((p) => p.id).sort();
    expect(paneIds).toEqual(["s1-main", "sh1", "sh2"]);
  });
});
