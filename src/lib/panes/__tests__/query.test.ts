import { describe, it, expect, beforeEach } from "vitest";
import { collectPaneTree } from "../query";
import {
  sessionLayouts,
  initSessionLayout,
  insertLeaf,
  resetLayouts,
  getLayout,
  type LayoutNode,
} from "../layout";
import { createPane, resetInstances } from "../instances";
import { resetFocus } from "../focus";

describe("collectPaneTree", () => {
  beforeEach(() => {
    resetLayouts();
    resetInstances();
    resetFocus();
  });

  it("returns empty descriptors and null layout when session has no layout", () => {
    const snap = collectPaneTree("nope");
    expect(snap.sessionId).toBe("nope");
    expect(snap.layout).toBeNull();
    expect(snap.descriptors).toEqual([]);
  });

  it("serializes a single-leaf layout with the main pane", () => {
    const sid = "s1";
    const paneId = `${sid}-main`;
    createPane({ id: paneId, type: "shell", ptyId: sid });
    initSessionLayout(sid, paneId);

    const snap = collectPaneTree(sid);
    expect(snap.sessionId).toBe(sid);
    expect(snap.layout?.kind).toBe("leaf");
    expect(snap.descriptors).toHaveLength(1);
    expect(snap.descriptors[0]).toMatchObject({
      id: paneId,
      type: "shell",
      ptyId: sid,
    });
  });

  it("walks a nested split tree in depth-first order", () => {
    const sid = "s2";
    createPane({ id: "a", type: "shell", ptyId: "pty-a" });
    createPane({ id: "b", type: "shell", ptyId: "pty-b" });
    createPane({ id: "c", type: "command", ptyId: "pty-c", command: "npm test" });
    initSessionLayout(sid, "a");

    // Split a → horizontal [a, b]
    sessionLayouts.update((m) => {
      const tree = getLayout(sid);
      m.set(sid, insertLeaf(tree, "a", "h", "b"));
      return new Map(m);
    });
    // Split b → vertical [b, c]
    sessionLayouts.update((m) => {
      const tree = getLayout(sid);
      m.set(sid, insertLeaf(tree, "b", "v", "c"));
      return new Map(m);
    });

    const snap = collectPaneTree(sid);
    expect(snap.layout?.kind).toBe("split");
    expect(snap.descriptors.map((d) => d.id)).toEqual(["a", "b", "c"]);
    const cmd = snap.descriptors.find((d) => d.id === "c")!;
    expect(cmd.type).toBe("command");
    expect(cmd.command).toBe("npm test");
  });

  it("marks leaves with missing pane instances as unknown", () => {
    const sid = "s3";
    // Layout references a pane id that never got a PaneInstance
    sessionLayouts.update((m) => {
      m.set(sid, { kind: "leaf", paneId: "orphan" } as LayoutNode);
      return new Map(m);
    });
    const snap = collectPaneTree(sid);
    expect(snap.descriptors).toHaveLength(1);
    expect(snap.descriptors[0]).toMatchObject({
      id: "orphan",
      type: "unknown",
      ptyId: "",
    });
  });

  it("captures registered spawnProfileRef as profileId", () => {
    const sid = "s4";
    createPane({
      id: "p",
      type: "shell",
      ptyId: "pty",
      spawnProfileRef: { kind: "registered", id: "claude" },
    });
    initSessionLayout(sid, "p");
    const snap = collectPaneTree(sid);
    expect(snap.descriptors[0].profileId).toBe("claude");
  });

  it("captures inline spawnProfileRef profile.id as profileId", () => {
    const sid = "s5";
    createPane({
      id: "p",
      type: "shell",
      ptyId: "pty",
      spawnProfileRef: {
        kind: "inline",
        profile: {
          id: "custom-abc",
          name: "Custom",
          source: "builtin",
        },
      },
    });
    initSessionLayout(sid, "p");
    const snap = collectPaneTree(sid);
    expect(snap.descriptors[0].profileId).toBe("custom-abc");
  });

  it("omits profileId when no spawnProfileRef is attached", () => {
    const sid = "s6";
    createPane({ id: "p", type: "shell", ptyId: "pty" });
    initSessionLayout(sid, "p");
    const snap = collectPaneTree(sid);
    expect(snap.descriptors[0].profileId).toBeUndefined();
  });

  it("carries workingDir and name through into descriptors", () => {
    const sid = "s7";
    createPane({
      id: "p",
      type: "shell",
      ptyId: "pty",
      name: "build",
      workingDir: "/tmp/work",
    });
    initSessionLayout(sid, "p");
    const snap = collectPaneTree(sid);
    expect(snap.descriptors[0].name).toBe("build");
    expect(snap.descriptors[0].workingDir).toBe("/tmp/work");
  });
});
