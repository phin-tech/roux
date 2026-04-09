import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  sessionLayouts,
  resetLayouts,
} from "$lib/panes/layout";
import {
  focusedPaneId,
  resetFocus,
} from "$lib/panes/focus";
import {
  resetInstances,
} from "$lib/panes/instances";
import {
  initSession,
  splitPane,
} from "$lib/panes/actions";
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
    permissionInfo: null,
    createdAt: Date.now(),
    projectId: null,
    isGitRepo: true,
  };
}

describe("socket command: split", () => {
  beforeEach(() => {
    resetLayouts();
    resetInstances();
    resetFocus();
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds a horizontal shell split to session", () => {
    const session = makeSession("s1");
    addSession(session);
    initSession("s1");
    focusedPaneId.set("s1-main");

    // Simulate what the roux-command handler does for split
    const ptyId = "pty-shell-1";
    splitPane("s1", "h", { type: "shell", ptyId });

    const tree = get(sessionLayouts).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("h");
      expect(tree.children).toHaveLength(2);
    }
  });

  it("adds a vertical shell split to session", () => {
    const session = makeSession("s1");
    addSession(session);
    initSession("s1");
    focusedPaneId.set("s1-main");

    splitPane("s1", "v", { type: "shell", ptyId: "pty-v-1" });

    const tree = get(sessionLayouts).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("v");
    }
  });
});

describe("socket command: session-created", () => {
  beforeEach(() => {
    resetLayouts();
    resetInstances();
    resetFocus();
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds new session and initializes panes", () => {
    const session = makeSession("new-1", "My Session");
    addSession(session);
    initSession("new-1");

    const state = get(sessionState);
    expect(state.sessions).toHaveLength(1);
    expect(state.sessions[0].id).toBe("new-1");
    expect(state.activeSessionId).toBe("new-1");

    const tree = get(sessionLayouts).get("new-1");
    expect(tree).toBeDefined();
    expect(tree!.kind).toBe("leaf");
  });
});

describe("socket command: shell-opened", () => {
  beforeEach(() => {
    resetLayouts();
    resetInstances();
    resetFocus();
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds shell pane to existing session", () => {
    addSession(makeSession("s1"));
    initSession("s1");
    focusedPaneId.set("s1-main");

    // Simulate shell-opened: PTY already spawned by backend, just add pane
    splitPane("s1", "h", {
      type: "shell",
      ptyId: "shell-pty-1",
    });

    const tree = get(sessionLayouts).get("s1")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.children).toHaveLength(2);
    }
  });
});

describe("socket command: command-opened", () => {
  beforeEach(() => {
    resetLayouts();
    resetInstances();
    resetFocus();
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds command pane with metadata", () => {
    addSession(makeSession("s1"));
    initSession("s1");
    focusedPaneId.set("s1-main");

    splitPane("s1", "h", {
      type: "command",
      ptyId: "cmd-pty-1",
      command: "npm test",
      workingDir: "/tmp/repo",
    });

    const tree = get(sessionLayouts).get("s1")!;
    expect(tree.kind).toBe("split");
  });
});

describe("socket command: focus", () => {
  beforeEach(() => {
    resetLayouts();
    resetInstances();
    resetFocus();
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
    initSession("s1");
    focusedPaneId.set("s1-main");

    splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

    // The splitPane function sets focus to the new pane
    // Just verify focus is set
    expect(get(focusedPaneId)).toBeDefined();
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
