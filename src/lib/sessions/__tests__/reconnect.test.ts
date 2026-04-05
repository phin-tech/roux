import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  createSession: vi.fn(),
  killSession: vi.fn(),
}));

import { reconnectSession } from "../reconnect";
import { sessionState, addSession } from "$lib/stores/sessions";
import { initSessionPanes, paneTrees } from "$lib/stores/panes";
import { createSession, killSession } from "$lib/tauri";
import type { Session } from "$lib/types";

describe("reconnectSession", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    paneTrees.set(new Map());
    vi.mocked(killSession).mockReset();
    vi.mocked(createSession).mockReset();
  });

  it("removes the old disconnected session before creating the replacement", async () => {
    const oldSession: Session = {
      id: "old",
      name: "Repo",
      repoRoot: "/repo",
      worktreePath: "/repo-worktree",
      branch: "feature/x",
      isWorktree: true,
      status: "disconnected",
      model: null,
      cost: null,
      permissionInfo: null,
      createdAt: 1,
    };
    const newSession: Session = {
      ...oldSession,
      id: "new",
      status: "idle",
      createdAt: 2,
    };

    addSession(oldSession);
    initSessionPanes(oldSession.id);

    vi.mocked(killSession).mockResolvedValue();
    vi.mocked(createSession).mockResolvedValue(newSession);

    await reconnectSession(oldSession);

    expect(killSession).toHaveBeenCalledWith("old");
    expect(createSession).toHaveBeenCalledWith(
      "/repo",
      "Repo",
      "/repo-worktree",
      null
    );
    expect(vi.mocked(killSession).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(createSession).mock.invocationCallOrder[0]
    );

    const state = get(sessionState);
    expect(state.sessions.map((session) => session.id)).toEqual(["new"]);
    expect(state.activeSessionId).toBe("new");
    expect(get(paneTrees).has("old")).toBe(false);
    expect(get(paneTrees).has("new")).toBe(true);
  });
});
