import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  reconnectSessionPty: vi.fn(),
  killSession: vi.fn(),
}));

vi.mock("$lib/panes/terminalRegistry", () => ({
  disposeClaudeTerminal: vi.fn(),
}));

import { reconnectSession } from "../reconnect";
import { sessionState, addSession } from "$lib/stores/sessions";
import { initSessionPanes, paneTrees } from "$lib/stores/panes";
import { reconnectSessionPty } from "$lib/tauri";
import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
import type { Session } from "$lib/types";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "sess-1",
    name: "Repo",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "disconnected",
    model: null,
    cost: null,
    permissionInfo: null,
    createdAt: 1,
    ...overrides,
  };
}

describe("reconnectSession", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    paneTrees.set(new Map());
    vi.mocked(reconnectSessionPty).mockReset();
    vi.mocked(disposeClaudeTerminal).mockReset();
  });

  it("preserves the pane tree when reconnecting", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    // Add a shell pane to simulate a split layout
    const trees = get(paneTrees);
    const tree = trees.get(session.id)!;
    expect(tree).toBeDefined();

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session);

    // Pane tree should still exist under the same session ID
    const afterTrees = get(paneTrees);
    expect(afterTrees.has(session.id)).toBe(true);

    // Session should be updated to idle
    const state = get(sessionState);
    expect(state.sessions.find((s) => s.id === session.id)?.status).toBe("idle");
  });

  it("disposes the claude terminal before reconnecting", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session);

    expect(disposeClaudeTerminal).toHaveBeenCalledWith(session.id);
    expect(vi.mocked(disposeClaudeTerminal).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(reconnectSessionPty).mock.invocationCallOrder[0]
    );
  });

  it("passes extra flags through to the Tauri command", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session, ["--resume", "abc123"]);

    expect(reconnectSessionPty).toHaveBeenCalledWith(session.id, ["--resume", "abc123"]);
  });

  it("passes --continue flag", async () => {
    const session = makeSession();
    addSession(session);
    initSessionPanes(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session, ["--continue"]);

    expect(reconnectSessionPty).toHaveBeenCalledWith(session.id, ["--continue"]);
  });
});
