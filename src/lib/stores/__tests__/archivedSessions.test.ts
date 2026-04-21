import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  listArchivedSessions: vi.fn(),
  listSessions: vi.fn().mockResolvedValue([]),
  restoreSession: vi.fn().mockResolvedValue(undefined),
  deleteSessionPermanently: vi.fn().mockResolvedValue(undefined),
  sessionWorktreeExists: vi.fn(),
  removeWorktree: vi.fn().mockResolvedValue(undefined),
}));

import {
  archivedSessionsState,
  loadArchivedSessions,
  restoreArchivedSession,
  removeArchivedSessionForever,
  addArchivedSessionFromEvent,
  cleanArchivedWorktree,
} from "../archivedSessions";
import { sessionState } from "$lib/stores/sessions";
import {
  listArchivedSessions,
  listSessions,
  restoreSession,
  deleteSessionPermanently,
  sessionWorktreeExists,
  removeWorktree,
} from "$lib/tauri";
import type { Session } from "$lib/types";

function makeArchived(id: string, overrides: Partial<Session> = {}): Session {
  return {
    id,
    name: id,
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "disconnected",
    model: null,
    cost: null,
    createdAt: 0,
    archived: true,
    endedAt: 1000,
    ...overrides,
  };
}

describe("archivedSessions store", () => {
  beforeEach(() => {
    archivedSessionsState.set({ sessions: [], loaded: false, worktreeExists: new Map() });
    sessionState.set({ sessions: [], activeSessionId: null });
    vi.mocked(listArchivedSessions).mockReset();
    vi.mocked(listSessions).mockReset().mockResolvedValue([]);
    vi.mocked(restoreSession).mockReset().mockResolvedValue(undefined);
    vi.mocked(deleteSessionPermanently).mockReset().mockResolvedValue(undefined);
    vi.mocked(sessionWorktreeExists).mockReset().mockResolvedValue(true);
    vi.mocked(removeWorktree).mockReset().mockResolvedValue(undefined);
  });

  it("loadArchivedSessions hydrates the list and per-session worktree existence", async () => {
    const sessions = [makeArchived("a"), makeArchived("b", { worktreePath: "/gone" })];
    vi.mocked(listArchivedSessions).mockResolvedValueOnce(sessions);
    vi.mocked(sessionWorktreeExists).mockImplementation(async (id) => id !== "b");

    await loadArchivedSessions();

    const state = get(archivedSessionsState);
    expect(state.loaded).toBe(true);
    expect(state.sessions.map((s) => s.id)).toEqual(["a", "b"]);
    expect(state.worktreeExists.get("a")).toBe(true);
    expect(state.worktreeExists.get("b")).toBe(false);
  });

  it("restoreArchivedSession removes the row + its worktreeExists entry, hydrates the active store", async () => {
    archivedSessionsState.set({
      sessions: [makeArchived("a"), makeArchived("b")],
      loaded: true,
      worktreeExists: new Map([["a", true], ["b", false]]),
    });
    const restoredActive: Session = { ...makeArchived("a"), archived: false, endedAt: null };
    vi.mocked(listSessions).mockResolvedValueOnce([restoredActive]);

    await restoreArchivedSession("a");

    expect(restoreSession).toHaveBeenCalledWith("a");
    expect(listSessions).toHaveBeenCalled();
    const state = get(archivedSessionsState);
    expect(state.sessions.map((s) => s.id)).toEqual(["b"]);
    // Don't leave a stale worktreeExists key for the restored id.
    expect(state.worktreeExists.has("a")).toBe(false);
    expect(state.worktreeExists.get("b")).toBe(false);
    expect(get(sessionState).sessions.map((s) => s.id)).toEqual(["a"]);
  });

  it("addArchivedSessionFromEvent honors explicit worktreeExists=false", () => {
    archivedSessionsState.set({
      sessions: [],
      loaded: true,
      worktreeExists: new Map(),
    });

    addArchivedSessionFromEvent(makeArchived("wt-gone"), false);

    const state = get(archivedSessionsState);
    expect(state.sessions.map((s) => s.id)).toEqual(["wt-gone"]);
    expect(state.worktreeExists.get("wt-gone")).toBe(false);
  });

  it("removeArchivedSessionForever removes the row and cleans its worktree entry", async () => {
    archivedSessionsState.set({
      sessions: [makeArchived("a"), makeArchived("b")],
      loaded: true,
      worktreeExists: new Map([["a", true], ["b", false]]),
    });

    await removeArchivedSessionForever("b");

    expect(deleteSessionPermanently).toHaveBeenCalledWith("b");
    const state = get(archivedSessionsState);
    expect(state.sessions.map((s) => s.id)).toEqual(["a"]);
    expect(state.worktreeExists.has("b")).toBe(false);
  });

  it("cleanArchivedWorktree removes the worktree and flips worktreeExists to false", async () => {
    archivedSessionsState.set({
      sessions: [makeArchived("a", { isWorktree: true, worktreePath: "/wt/a" })],
      loaded: true,
      worktreeExists: new Map([["a", true]]),
    });

    await cleanArchivedWorktree("a", "/wt/a");

    expect(removeWorktree).toHaveBeenCalledWith("/wt/a");
    const state = get(archivedSessionsState);
    expect(state.sessions.map((s) => s.id)).toEqual(["a"]);
    expect(state.worktreeExists.get("a")).toBe(false);
  });

  it("addArchivedSessionFromEvent prepends when the pane has loaded", () => {
    archivedSessionsState.set({
      sessions: [makeArchived("existing")],
      loaded: true,
      worktreeExists: new Map([["existing", true]]),
    });

    addArchivedSessionFromEvent(makeArchived("new"));

    expect(get(archivedSessionsState).sessions.map((s) => s.id)).toEqual(["new", "existing"]);
    expect(get(archivedSessionsState).worktreeExists.get("new")).toBe(true);
  });

  it("addArchivedSessionFromEvent skips when the pane has not loaded", () => {
    addArchivedSessionFromEvent(makeArchived("new"));
    expect(get(archivedSessionsState).sessions).toHaveLength(0);
  });

  it("addArchivedSessionFromEvent is idempotent", () => {
    archivedSessionsState.set({
      sessions: [makeArchived("a")],
      loaded: true,
      worktreeExists: new Map([["a", true]]),
    });

    addArchivedSessionFromEvent(makeArchived("a"));
    expect(get(archivedSessionsState).sessions).toHaveLength(1);
  });
});
