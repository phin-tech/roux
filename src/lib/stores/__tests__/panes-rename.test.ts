import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  initSessionPanes,
  addSplit,
  renamePane,
  getPane,
} from "../panes";

const shell = (id: string) => ({ id, type: "shell" as const, ptyId: `pty-${id}` });

const markdown = (id: string, docPath: string) => ({
  id,
  type: "markdown" as const,
  ptyId: "",
  docPath,
});

describe("renamePane", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  // ── Basic naming ──────────────────────────────────────────

  it("names the main claude pane", () => {
    initSessionPanes("s1");
    renamePane("s1", "s1-main", "backend work");

    expect(getPane("s1", "s1-main")?.name).toBe("backend work");
  });

  it("names a shell split pane", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", shell("sh1"));

    renamePane("s1", "sh1", "logs");

    expect(getPane("s1", "sh1")?.name).toBe("logs");
  });

  it("names a deeply nested pane (3 levels)", () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");
    addSplit("s1", "horizontal", shell("sh1"));
    focusedPaneId.set("sh1");
    addSplit("s1", "vertical", shell("sh2"));

    renamePane("s1", "sh2", "deep pane");

    expect(getPane("s1", "sh2")?.name).toBe("deep pane");
    expect(getPane("s1", "s1-main")?.name).toBeUndefined();
    expect(getPane("s1", "sh1")?.name).toBeUndefined();
  });

  // ── Clearing ──────────────────────────────────────────────

  it("clears a name when set to empty string", () => {
    initSessionPanes("s1");
    renamePane("s1", "s1-main", "my pane");
    renamePane("s1", "s1-main", "");

    expect(getPane("s1", "s1-main")?.name).toBeUndefined();
  });

  // ── Isolation ─────────────────────────────────────────────

  it("only affects the target pane, not siblings", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", shell("sh1"));

    renamePane("s1", "s1-main", "claude");

    expect(getPane("s1", "s1-main")?.name).toBe("claude");
    expect(getPane("s1", "sh1")?.name).toBeUndefined();
  });

  it("only affects the target session, not others", () => {
    initSessionPanes("s1");
    initSessionPanes("s2");

    renamePane("s1", "s1-main", "first");

    expect(getPane("s1", "s1-main")?.name).toBe("first");
    expect(getPane("s2", "s2-main")?.name).toBeUndefined();
  });

  // ── Edge cases ────────────────────────────────────────────

  it("no-ops for a nonexistent session", () => {
    initSessionPanes("s1");
    renamePane("ghost", "s1-main", "nope");

    expect(getPane("s1", "s1-main")?.name).toBeUndefined();
  });

  it("no-ops for a nonexistent pane", () => {
    initSessionPanes("s1");
    renamePane("s1", "ghost", "nope");

    expect(getPane("s1", "s1-main")?.name).toBeUndefined();
  });

  // ── Preserves data ────────────────────────────────────────

  it("keeps existing pane properties intact", () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", markdown("doc1", "/tmp/notes.md"));

    renamePane("s1", "doc1", "my notes");

    const pane = getPane("s1", "doc1");
    expect(pane?.name).toBe("my notes");
    expect(pane?.type).toBe("markdown");
    expect(pane?.docPath).toBe("/tmp/notes.md");
  });

  // ── Svelte reactivity ────────────────────────────────────

  it("produces a new tree reference so Svelte re-renders", () => {
    initSessionPanes("s1");
    const before = get(paneTrees).get("s1");

    renamePane("s1", "s1-main", "renamed");

    const after = get(paneTrees).get("s1");
    expect(after).not.toBe(before);
  });
});
