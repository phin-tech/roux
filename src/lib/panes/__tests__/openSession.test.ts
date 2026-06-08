import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  listSessions: vi.fn().mockResolvedValue([]),
  listAllPtys: vi.fn().mockResolvedValue([]),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/stores/archivedSessions", () => ({
  restoreArchivedSession: vi.fn().mockResolvedValue(undefined),
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

vi.mock("$lib/sessions/reconnect", () => ({
  reattachSession: vi.fn(async (session) => session),
  continueSessionShell: vi.fn(async (session) => session),
}));

import { openSessionById } from "../openSession";
import { sessionState } from "$lib/stores/sessions";
import { paneInstances, resetInstances } from "../instances";
import { sessionLayouts, resetLayouts } from "../layout";
import { attachPtyToPane } from "../attach";
import { loadPaneState } from "../persistence";
import { listAllPtys, listSessions } from "$lib/tauri";
import { restoreArchivedSession } from "$lib/stores/archivedSessions";
import { reattachSession, continueSessionShell } from "$lib/sessions/reconnect";
import type { PtyInfo } from "$lib/bindings";
import type { Session } from "$lib/types";

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

  it("continues a disconnected regular session when no live PTY attaches", async () => {
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
    expect(continueSessionShell).toHaveBeenCalledWith(
      expect.objectContaining({ id: "s1" }),
    );
    expect(get(sessionState).activeSessionId).toBe("s1");
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

  it("restores and continues an archived session when no active session exists", async () => {
    const restored: Session = {
      id: "archived-plan",
      name: "Archived plan",
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
      primaryPtyId: null,
    };
    vi.mocked(listSessions)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([restored]);
    vi.mocked(listAllPtys).mockResolvedValue([]);

    const result = await openSessionById("archived-plan");

    expect(result).toBe("opened");
    expect(restoreArchivedSession).toHaveBeenCalledWith("archived-plan");
    expect(reattachSession).toHaveBeenCalledWith(restored);
    expect(get(sessionState).activeSessionId).toBe("archived-plan");
  });

  it("continues a disconnected planning session when its PTY is dead (foreign ptyId)", async () => {
    // Planning sessions have a ptyId that differs from sessionId.
    // When the planning PTY is dead, reconnect should use
    // continueSessionShell and the pane's ptyId must stay session.id
    // so reconnectPrimaryPaneOnly can locate it.
    vi.mocked(listAllPtys).mockResolvedValue([]);
    vi.mocked(listSessions).mockResolvedValue([
      {
        id: "plan-session",
        name: "Plan",
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
    ]);
    sessionState.set({ sessions: [], activeSessionId: null });

    const result = await openSessionById("plan-session", {
      ptyId: "planning-pty",
    });

    expect(result).toBe("opened");
    expect(continueSessionShell).toHaveBeenCalledWith(
      expect.objectContaining({ id: "plan-session" }),
    );
    // The pane's ptyId must be session.id (not the dead planning PTY)
    // so that findSessionPrimaryPaneId can find it.
    expect(get(paneInstances).get("plan-session-main")?.ptyId).toBe(
      "plan-session",
    );
    expect(get(sessionState).activeSessionId).toBe("plan-session");
  });

  it("adds the session to the store after reconnect, never before", async () => {
    // The session must land in the store with the status that
    // continueSessionShell returned — not the original disconnected
    // status — so the UI never renders the SessionPicker.
    vi.mocked(listAllPtys).mockResolvedValue([]);
    vi.mocked(listSessions).mockResolvedValue([
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
    ]);
    vi.mocked(continueSessionShell).mockResolvedValueOnce({
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
      primaryPtyId: null,
    } as Session);
    sessionState.set({ sessions: [], activeSessionId: null });

    const result = await openSessionById("s1");

    expect(result).toBe("opened");
    expect(continueSessionShell).toHaveBeenCalled();
    // The store must reflect the reconnected status, not "disconnected".
    expect(get(sessionState).sessions[0]?.status).toBe("idle");
  });

  it("reconnects even when daemon reports idle but PTY is dead", async () => {
    // The daemon may report "idle" for a session whose PTY was killed
    // during restart. If no live PTY attaches, reconnect anyway.
    vi.mocked(listAllPtys).mockResolvedValue([]);
    vi.mocked(listSessions).mockResolvedValue([
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
    ]);
    sessionState.set({ sessions: [], activeSessionId: null });

    const result = await openSessionById("s1");

    expect(result).toBe("opened");
    expect(continueSessionShell).toHaveBeenCalled();
  });

  it("does not add the session to the store when it is gone and not archived", async () => {
    vi.mocked(listSessions).mockResolvedValue([]);
    vi.mocked(listAllPtys).mockResolvedValue([]);

    const result = await openSessionById("missing-session");

    expect(result).toBe("gone");
    expect(get(sessionState).sessions).toHaveLength(0);
  });
});
