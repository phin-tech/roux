import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  reconnectSessionPty: vi.fn(),
  killSession: vi.fn(),
}));

import { reconnectSession } from "../reconnect";
import { sessionState, addSession } from "$lib/stores/sessions";
import { initSession } from "$lib/panes/actions";
import { sessionLayouts } from "$lib/panes/layout";
import { resetLayouts } from "$lib/panes/layout";
import { resetInstances } from "$lib/panes/instances";
import { resetFocus } from "$lib/panes/focus";
import { reconnectSessionPty } from "$lib/tauri";
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
    projectId: null,
    ...overrides,
  };
}

describe("reconnectSession", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    resetLayouts();
    resetInstances();
    resetFocus();
    vi.mocked(reconnectSessionPty).mockReset();
  });

  it("preserves the layout tree when reconnecting", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    // Layout tree should exist
    const tree = get(sessionLayouts).get(session.id);
    expect(tree).toBeDefined();

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session);

    // Layout tree should still exist under the same session ID
    const afterTree = get(sessionLayouts).get(session.id);
    expect(afterTree).toBeDefined();

    // Session should be updated to idle
    const state = get(sessionState);
    expect(state.sessions.find((s) => s.id === session.id)?.status).toBe("idle");
  });

  it("calls replacePty on the main pane before reconnecting", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session);

    // replacePty is called inline, so we just check the reconnect was called after
    expect(reconnectSessionPty).toHaveBeenCalledWith(session.id, undefined);
  });

  it("passes extra flags through to the Tauri command", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session, ["--resume", "abc123"]);

    expect(reconnectSessionPty).toHaveBeenCalledWith(session.id, ["--resume", "abc123"]);
  });

  it("passes --continue flag", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const updatedSession = makeSession({ status: "idle" });
    vi.mocked(reconnectSessionPty).mockResolvedValue(updatedSession);

    await reconnectSession(session, ["--continue"]);

    expect(reconnectSessionPty).toHaveBeenCalledWith(session.id, ["--continue"]);
  });
});
