import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  killSession: vi.fn().mockResolvedValue(undefined),
  killPty: vi.fn().mockResolvedValue(undefined),
  removeWorktree: vi.fn().mockResolvedValue(undefined),
}));

import { closeSession } from "../close";
import { sessionState, addSession } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";
import { resetLayouts } from "$lib/panes/layout";
import { resetInstances } from "$lib/panes/instances";
import { resetFocus } from "$lib/panes/focus";
import { initSession } from "$lib/panes/actions";
import { killSession, removeWorktree } from "$lib/tauri";
import type { Session } from "$lib/types";
import { DEFAULT_SETTINGS } from "$lib/types";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "sess-1",
    name: "Test",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: null,
    isGitRepo: true,
    ...overrides,
  };
}

describe("closeSession", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    resetLayouts();
    resetInstances();
    resetFocus();
    settings.set({ ...DEFAULT_SETTINGS });
    vi.mocked(killSession).mockReset().mockResolvedValue(undefined);
    vi.mocked(removeWorktree).mockReset().mockResolvedValue(undefined);
    vi.restoreAllMocks();
  });

  it("closes a session and removes it from the store", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const result = await closeSession(session);

    expect(result).toBe(true);
    expect(killSession).toHaveBeenCalledWith(session.id);
    expect(get(sessionState).sessions).toHaveLength(0);
  });

  it("prompts for confirmation when session is thinking and confirmOnClose is true", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    settings.update((s) => ({ ...s, confirmOnClose: true }));

    const session = makeSession({ status: "thinking" });
    addSession(session);
    initSession(session.id);

    const result = await closeSession(session);

    expect(confirmSpy).toHaveBeenCalled();
    expect(result).toBe(false);
    expect(get(sessionState).sessions).toHaveLength(1);
  });

  it("skips confirmation when session is thinking but confirmOnClose is false", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    settings.update((s) => ({ ...s, confirmOnClose: false }));

    const session = makeSession({ status: "thinking" });
    addSession(session);
    initSession(session.id);

    const result = await closeSession(session);

    expect(confirmSpy).not.toHaveBeenCalled();
    expect(result).toBe(true);
    expect(get(sessionState).sessions).toHaveLength(0);
  });

  it("skips confirmation when force is true even if session is thinking", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    settings.update((s) => ({ ...s, confirmOnClose: true }));

    const session = makeSession({ status: "thinking" });
    addSession(session);
    initSession(session.id);

    const result = await closeSession(session, { force: true });

    expect(confirmSpy).not.toHaveBeenCalled();
    expect(result).toBe(true);
    expect(get(sessionState).sessions).toHaveLength(0);
  });

  it("prompts about worktree removal when mode is prompt", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    settings.update((s) => ({ ...s, worktreeCleanupOnClose: "prompt", cleanupWorktreesOnClose: false }));

    const session = makeSession({ isWorktree: true, worktreePath: "/repo-wt" });
    addSession(session);
    initSession(session.id);

    await closeSession(session);

    expect(confirmSpy).toHaveBeenCalledWith("Also remove the worktree at /repo-wt?");
    expect(removeWorktree).toHaveBeenCalledWith("/repo-wt");
  });

  it("auto-removes worktree when mode is always", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    settings.update((s) => ({ ...s, worktreeCleanupOnClose: "always", cleanupWorktreesOnClose: true }));

    const session = makeSession({ isWorktree: true, worktreePath: "/repo-wt" });
    addSession(session);
    initSession(session.id);

    await closeSession(session);

    // Should not prompt, just remove
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(removeWorktree).toHaveBeenCalledWith("/repo-wt");
  });

  it("skips worktree prompt when force is true", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    settings.update((s) => ({ ...s, worktreeCleanupOnClose: "prompt", cleanupWorktreesOnClose: false }));

    const session = makeSession({ isWorktree: true, worktreePath: "/repo-wt" });
    addSession(session);
    initSession(session.id);

    await closeSession(session, { force: true });

    expect(confirmSpy).not.toHaveBeenCalled();
    expect(removeWorktree).not.toHaveBeenCalled();
  });

  it("does not attempt worktree removal for non-worktree sessions", async () => {
    const session = makeSession({ isWorktree: false });
    addSession(session);
    initSession(session.id);

    await closeSession(session);

    expect(removeWorktree).not.toHaveBeenCalled();
  });
});
