import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  killSession: vi.fn().mockResolvedValue(undefined),
  killPty: vi.fn().mockResolvedValue(undefined),
  removeWorktree: vi.fn().mockResolvedValue(undefined),
  deleteSessionPermanently: vi.fn().mockResolvedValue(undefined),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
  listArchivedSessions: vi.fn().mockResolvedValue([]),
  listSessions: vi.fn().mockResolvedValue([]),
  restoreSession: vi.fn().mockResolvedValue(undefined),
  sessionWorktreeExists: vi.fn().mockResolvedValue(true),
}));

import { closeSession } from "../close";
import { sessionState, addSession } from "$lib/stores/sessions";
import { archivedSessionsState } from "$lib/stores/archivedSessions";
import { settings } from "$lib/stores/settings";
import { resetLayouts } from "$lib/panes/layout";
import { resetInstances } from "$lib/panes/instances";
import { resetFocus } from "$lib/panes/focus";
import { initSession } from "$lib/panes/actions";
import {
  killSession,
  removeWorktree,
  deleteSessionPermanently,
} from "$lib/tauri";
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
    archivedSessionsState.set({ sessions: [], loaded: false, worktreeExists: new Map() });
    resetLayouts();
    resetInstances();
    resetFocus();
    settings.set({ ...DEFAULT_SETTINGS });
    vi.mocked(killSession).mockReset().mockResolvedValue(undefined);
    vi.mocked(removeWorktree).mockReset().mockResolvedValue(undefined);
    vi.mocked(deleteSessionPermanently).mockReset().mockResolvedValue(undefined);
    vi.restoreAllMocks();
  });

  it("archives a session and moves it out of the active store; worktree stays", async () => {
    const session = makeSession({ isWorktree: true, worktreePath: "/wt" });
    addSession(session);
    initSession(session.id);
    archivedSessionsState.update((s) => ({ ...s, loaded: true }));

    const result = await closeSession(session);

    expect(result).toBe(true);
    expect(killSession).toHaveBeenCalledWith(session.id);
    expect(deleteSessionPermanently).not.toHaveBeenCalled();
    // Safer close: worktree is NEVER removed from interactive close path
    // (prompt mode used to stack a second confirm on OK-OK — that got
    // users into trouble, so we removed it entirely).
    expect(removeWorktree).not.toHaveBeenCalled();
    expect(get(sessionState).sessions).toHaveLength(0);
    const archived = get(archivedSessionsState).sessions;
    expect(archived).toHaveLength(1);
    expect(archived[0].id).toBe(session.id);
    expect(archived[0].archived).toBe(true);
    expect(archived[0].endedAt).toBeTypeOf("number");
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

  it("legacy setting worktreeCleanupOnClose=always still drops the worktree on archive", async () => {
    settings.update((s) => ({
      ...s,
      worktreeCleanupOnClose: "always",
      cleanupWorktreesOnClose: true,
    }));

    const session = makeSession({ isWorktree: true, worktreePath: "/wt" });
    addSession(session);
    initSession(session.id);

    await closeSession(session);

    expect(killSession).toHaveBeenCalledWith(session.id);
    expect(removeWorktree).toHaveBeenCalledWith("/wt");
  });

  it("legacy setting worktreeCleanupOnClose=prompt does NOT prompt or drop the worktree", async () => {
    // This is the bug fix: old behavior stacked a second confirm on top
    // of the close confirm, which made double-Enter delete worktrees.
    const confirmSpy = vi.spyOn(window, "confirm");
    settings.update((s) => ({
      ...s,
      worktreeCleanupOnClose: "prompt",
      cleanupWorktreesOnClose: false,
    }));

    const session = makeSession({ isWorktree: true, worktreePath: "/wt" });
    addSession(session);
    initSession(session.id);

    await closeSession(session);

    expect(confirmSpy).not.toHaveBeenCalled();
    expect(removeWorktree).not.toHaveBeenCalled();
  });

  it("delete-forever calls deleteSessionPermanently and does NOT touch the worktree", async () => {
    const session = makeSession({ isWorktree: true, worktreePath: "/wt" });
    addSession(session);
    initSession(session.id);

    await closeSession(session, { action: "delete-forever" });

    expect(deleteSessionPermanently).toHaveBeenCalledWith(session.id);
    expect(killSession).not.toHaveBeenCalled();
    // Worktree handling lives in the History pane now — close.ts
    // delete-forever is a programmatic-only path and stays surgical.
    expect(removeWorktree).not.toHaveBeenCalled();
    expect(get(sessionState).sessions).toHaveLength(0);
    expect(get(archivedSessionsState).sessions).toHaveLength(0);
  });
});
