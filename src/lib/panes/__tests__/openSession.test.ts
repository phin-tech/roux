import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  listSessions: vi.fn().mockResolvedValue([]),
  listAllPtys: vi.fn().mockResolvedValue([]),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../persistence", () => ({
  loadPaneState: vi.fn().mockResolvedValue(null),
}));

vi.mock("../terminals", () => ({
  initTerminal: vi.fn(),
  attachPtyListeners: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../attach", () => ({
  attachPtyToPane: vi.fn().mockResolvedValue(undefined),
}));

import { openSessionById } from "../openSession";
import { sessionState } from "$lib/stores/sessions";
import { paneInstances, resetInstances } from "../instances";
import { sessionLayouts, resetLayouts } from "../layout";
import { attachPtyToPane } from "../attach";
import { loadPaneState } from "../persistence";
import { listAllPtys } from "$lib/tauri";
import type { PtyInfo } from "$lib/bindings";

function ptyInfo(id: string, sessionId = id): PtyInfo {
  return {
    id,
    session_id: sessionId,
    role: "sessionPrimary",
    status: { type: "RunningDetached", since_ms: 1 },
    name: null,
    working_dir: "/repo",
    profile: "claude",
    unread_output: false,
    bell_pending: false,
  };
}

describe("openSessionById", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    resetInstances();
    resetLayouts();
    vi.clearAllMocks();
    vi.mocked(listAllPtys).mockResolvedValue([ptyInfo("s1")]);
  });

  it("recreates a pane and attaches the old PTY for a disconnected session", async () => {
    sessionState.set({
      sessions: [
        {
          id: "s1",
          name: "Session",
          repoRoot: "/repo",
          worktreePath: "/repo",
          branch: "main",
          isWorktree: false,
          status: "disconnected",
          model: null,
          cost: null,
          createdAt: 1,
          projectId: null,
          isGitRepo: true,
        },
      ],
      activeSessionId: null,
    });

    const result = await openSessionById("s1");

    expect(result).toBe("opened");
    expect(get(sessionLayouts).get("s1")).toEqual({
      kind: "leaf",
      paneId: "s1-main",
    });
    expect(get(paneInstances).get("s1-main")?.ptyId).toBe("s1");
    expect(attachPtyToPane).toHaveBeenCalledWith("s1-main", "s1");
    expect(get(sessionState).activeSessionId).toBe("s1");
    expect(get(sessionState).sessions[0].status).toBe("idle");
  });

  it("restores an idle local session when no pane is attached", async () => {
    sessionState.set({
      sessions: [
        {
          id: "s1",
          name: "Session",
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
        },
      ],
      activeSessionId: null,
    });

    const result = await openSessionById("s1");

    expect(result).toBe("opened");
    expect(attachPtyToPane).toHaveBeenCalledWith("s1-main", "s1");
    expect(get(sessionState).activeSessionId).toBe("s1");
  });

  it("does not mark a disconnected session idle when no live PTY attaches", async () => {
    vi.mocked(listAllPtys).mockResolvedValue([]);
    sessionState.set({
      sessions: [
        {
          id: "s1",
          name: "Session",
          repoRoot: "/repo",
          worktreePath: "/repo",
          branch: "main",
          isWorktree: false,
          status: "disconnected",
          model: null,
          cost: null,
          createdAt: 1,
          projectId: null,
          isGitRepo: true,
        },
      ],
      activeSessionId: null,
    });

    const result = await openSessionById("s1");

    expect(result).toBe("opened");
    expect(attachPtyToPane).not.toHaveBeenCalled();
    expect(get(sessionState).sessions[0].status).toBe("disconnected");
  });

  it("reattaches the supplied work-item PTY to the session main pane", async () => {
    vi.mocked(listAllPtys).mockResolvedValue([ptyInfo("planning-pty", "s1")]);
    sessionState.set({
      sessions: [
        {
          id: "s1",
          name: "Session",
          repoRoot: "/repo",
          worktreePath: "/repo",
          branch: "main",
          isWorktree: false,
          status: "disconnected",
          model: null,
          cost: null,
          createdAt: 1,
          projectId: null,
          isGitRepo: true,
        },
      ],
      activeSessionId: null,
    });

    const result = await openSessionById("s1", { ptyId: "planning-pty" });

    expect(result).toBe("opened");
    expect(get(paneInstances).get("s1-main")?.ptyId).toBe("planning-pty");
    expect(attachPtyToPane).toHaveBeenCalledWith("s1-main", "planning-pty");
    expect(get(sessionState).sessions[0].status).toBe("idle");
  });

  it("reattaches a session manager open using the session primary PTY id", async () => {
    vi.mocked(listAllPtys).mockResolvedValue([ptyInfo("primary-pty", "s1")]);
    sessionState.set({
      sessions: [
        {
          id: "s1",
          name: "Session",
          repoRoot: "/repo",
          worktreePath: "/repo",
          branch: "main",
          isWorktree: false,
          status: "disconnected",
          model: null,
          cost: null,
          createdAt: 1,
          projectId: null,
          isGitRepo: true,
          primaryPtyId: "primary-pty",
        },
      ],
      activeSessionId: null,
    });

    const result = await openSessionById("s1");

    expect(result).toBe("opened");
    expect(get(paneInstances).get("s1-main")?.ptyId).toBe("primary-pty");
    expect(attachPtyToPane).toHaveBeenCalledWith("s1-main", "primary-pty");
    expect(get(sessionState).sessions[0].status).toBe("idle");
  });

  it("overrides a persisted primary descriptor with the supplied work-item PTY", async () => {
    vi.mocked(listAllPtys).mockResolvedValue([ptyInfo("planning-pty", "s1")]);
    vi.mocked(loadPaneState).mockResolvedValueOnce({
      schemaVersion: 5,
      layout: { kind: "leaf", paneId: "s1-main" },
      descriptors: [{ id: "s1-main", type: "shell", ptyId: "s1" }],
    });
    sessionState.set({
      sessions: [
        {
          id: "s1",
          name: "Session",
          repoRoot: "/repo",
          worktreePath: "/repo",
          branch: "main",
          isWorktree: false,
          status: "disconnected",
          model: null,
          cost: null,
          createdAt: 1,
          projectId: null,
          isGitRepo: true,
        },
      ],
      activeSessionId: null,
    });

    const result = await openSessionById("s1", { ptyId: "planning-pty" });

    expect(result).toBe("opened");
    expect(get(paneInstances).get("s1-main")?.ptyId).toBe("planning-pty");
    expect(attachPtyToPane).toHaveBeenCalledWith("s1-main", "planning-pty");
  });
});
