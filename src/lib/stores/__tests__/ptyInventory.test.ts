import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PtyInfo, Session } from "$lib/types";
import { listAllPtys } from "$lib/tauri";
import { sessionState } from "$lib/stores/sessions";
import {
  _resetPtyInventoryForTests,
  initPtyInventoryPolling,
  ptyInventoryBySession,
  refreshPtyInventory,
  summarizePtyInventory,
} from "../ptyInventory";

vi.mock("$lib/tauri", () => ({
  listAllPtys: vi.fn(),
}));

const mockListAllPtys = vi.mocked(listAllPtys);

function makeSession(id: string): Session {
  return {
    id,
    name: id,
    repoRoot: "/repo",
    worktreePath: `/repo/${id}`,
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: null,
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: id,
    archived: false,
    endedAt: null,
  };
}

function makePty(overrides: Partial<PtyInfo> = {}): PtyInfo {
  return {
    id: "pty-1",
    session_id: "s1",
    role: "sessionPrimary",
    status: { type: "RunningAttached", pane_id: "pane-1" },
    name: null,
    working_dir: "/repo",
    profile: "claude",
    unread_output: false,
    bell_pending: false,
    ...overrides,
  };
}

describe("ptyInventory", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockListAllPtys.mockReset();
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetPtyInventoryForTests();
  });

  afterEach(() => {
    _resetPtyInventoryForTests();
    sessionState.set({ sessions: [], activeSessionId: null });
    vi.useRealTimers();
  });

  it("summarizes attached and detached PTYs by active session", () => {
    const summary = summarizePtyInventory(
      [
        makePty({ id: "pty-1", session_id: "s1", status: { type: "RunningAttached", pane_id: "p1" } }),
        makePty({ id: "pty-2", session_id: "s1", status: { type: "RunningDetached", since_ms: 1 }, unread_output: true }),
        makePty({ id: "pty-3", session_id: "s2", status: { type: "RunningDetached", since_ms: 2 } }),
        makePty({ id: "orphan", session_id: "missing" }),
      ],
      new Set(["s1", "s2"]),
    );

    expect(summary.get("s1")).toEqual({
      attachedCount: 1,
      detachedCount: 1,
      detachedHasUnread: true,
    });
    expect(summary.get("s2")).toEqual({
      attachedCount: 0,
      detachedCount: 1,
      detachedHasUnread: false,
    });
    expect(summary.has("missing")).toBe(false);
  });

  it("refreshes all session inventory with one backend call", async () => {
    sessionState.set({ sessions: [makeSession("s1"), makeSession("s2")], activeSessionId: "s1" });
    mockListAllPtys.mockResolvedValue([
      makePty({ id: "pty-1", session_id: "s1", status: { type: "RunningAttached", pane_id: "p1" } }),
      makePty({ id: "pty-2", session_id: "s2", status: { type: "RunningDetached", since_ms: 1 } }),
    ]);

    await refreshPtyInventory();

    let snapshot = new Map();
    const unsubscribe = ptyInventoryBySession.subscribe((value) => {
      snapshot = value;
    });
    unsubscribe();

    expect(mockListAllPtys).toHaveBeenCalledTimes(1);
    expect(snapshot.get("s1")?.attachedCount).toBe(1);
    expect(snapshot.get("s2")?.detachedCount).toBe(1);
  });

  it("polls once per interval rather than once per session", async () => {
    sessionState.set({ sessions: [makeSession("s1"), makeSession("s2")], activeSessionId: "s1" });
    mockListAllPtys.mockResolvedValue([]);

    const stop = initPtyInventoryPolling();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockListAllPtys).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(5000);
    expect(mockListAllPtys).toHaveBeenCalledTimes(2);

    stop();
  });

  it("does not refresh when only the active session changes", async () => {
    const sessions = [makeSession("s1"), makeSession("s2")];
    sessionState.set({ sessions, activeSessionId: "s1" });
    mockListAllPtys.mockResolvedValue([]);

    const stop = initPtyInventoryPolling();
    await Promise.resolve();
    await Promise.resolve();
    expect(mockListAllPtys).toHaveBeenCalledTimes(1);

    sessionState.set({ sessions, activeSessionId: "s2" });
    await Promise.resolve();
    await Promise.resolve();

    expect(mockListAllPtys).toHaveBeenCalledTimes(1);
    stop();
  });
});
