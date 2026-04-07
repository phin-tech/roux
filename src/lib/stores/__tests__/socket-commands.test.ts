import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  initSessionPanes,
  addSplit,
  type Pane,
} from "../panes";
import {
  sessionState,
  addSession,
  setActiveSession,
} from "../sessions";
import type { RouxCommand } from "$lib/tauri";

/**
 * Tests for the store operations triggered by roux-cli socket commands.
 * These test the same code paths that App.svelte's onRouxCommand handler calls.
 */

function makeSession(id: string, name = "test") {
  return {
    id,
    name,
    repoRoot: "/tmp/repo",
    worktreePath: "/tmp/repo",
    branch: "main",
    isWorktree: false,
    status: "idle" as const,
    model: null,
    cost: null,
    createdAt: Date.now(),
    projectId: null,
  };
}

describe("socket command: split", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds a horizontal shell split to session", () => {
    const session = makeSession("s1");
    addSession(session);
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    // Simulate what the roux-command handler does for split
    const paneId = "new-shell-1";
    const ptyId = "pty-shell-1";
    addSplit("s1", "horizontal", { id: paneId, type: "shell", ptyId });

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("horizontal");
      expect(tree.children).toHaveLength(2);
      expect(tree.children[1]).toMatchObject({
        kind: "pane",
        pane: { id: "new-shell-1", type: "shell", ptyId: "pty-shell-1" },
      });
    }
  });

  it("adds a vertical shell split to session", () => {
    const session = makeSession("s1");
    addSession(session);
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    addSplit("s1", "vertical", { id: "v-1", type: "shell", ptyId: "pty-v-1" });

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("vertical");
    }
  });
});

describe("socket command: session-created", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds new session and initializes panes", () => {
    const session = makeSession("new-1", "My Session");
    addSession(session);
    initSessionPanes("new-1");

    const state = get(sessionState);
    expect(state.sessions).toHaveLength(1);
    expect(state.sessions[0].id).toBe("new-1");
    expect(state.activeSessionId).toBe("new-1");

    const tree = get(paneTrees).get("new-1");
    expect(tree).toBeDefined();
    expect(tree!.kind).toBe("pane");
  });
});

describe("socket command: shell-opened", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds shell pane to existing session", () => {
    addSession(makeSession("s1"));
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    // Simulate shell-opened: PTY already spawned by backend, just add pane
    addSplit("s1", "horizontal", {
      id: "shell-pane-1",
      type: "shell",
      ptyId: "shell-pty-1",
    });

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.children).toHaveLength(2);
    }
  });
});

describe("socket command: command-opened", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds command pane with metadata", () => {
    addSession(makeSession("s1"));
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    const pane: Pane = {
      id: "cmd-pane-1",
      type: "command",
      ptyId: "cmd-pty-1",
      command: "npm test",
      workingDir: "/tmp/repo",
    };
    addSplit("s1", "horizontal", pane);

    const tree = get(paneTrees).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      const cmdNode = tree.children[1];
      expect(cmdNode.kind).toBe("pane");
      if (cmdNode.kind === "pane") {
        expect(cmdNode.pane.type).toBe("command");
        expect(cmdNode.pane.command).toBe("npm test");
        expect(cmdNode.pane.workingDir).toBe("/tmp/repo");
      }
    }
  });
});

describe("socket command: focus", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("switches active session", () => {
    addSession(makeSession("s1", "First"));
    addSession(makeSession("s2", "Second"));

    expect(get(sessionState).activeSessionId).toBe("s2");

    setActiveSession("s1");
    expect(get(sessionState).activeSessionId).toBe("s1");
  });

  it("sets focused pane", () => {
    addSession(makeSession("s1"));
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });

    focusedPaneId.set("shell-1");
    expect(get(focusedPaneId)).toBe("shell-1");
  });
});

describe("RouxCommand type", () => {
  it("handles minimal command payload", () => {
    const cmd: RouxCommand = { action: "split" };
    expect(cmd.action).toBe("split");
    expect(cmd.sessionId).toBeUndefined();
  });

  it("handles full command payload", () => {
    const cmd: RouxCommand = {
      action: "split",
      sessionId: "s1",
      paneId: "p1",
      direction: "horizontal",
    };
    expect(cmd.action).toBe("split");
    expect(cmd.sessionId).toBe("s1");
    expect(cmd.direction).toBe("horizontal");
  });

  it("handles command-opened payload", () => {
    const cmd: RouxCommand = {
      action: "command-opened",
      sessionId: "s1",
      paneId: "cmd-1",
      ptyId: "pty-1",
      command: "npm test",
      workingDir: "/tmp/repo",
    };
    expect(cmd.command).toBe("npm test");
    expect(cmd.workingDir).toBe("/tmp/repo");
  });
});
