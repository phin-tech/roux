import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/stores/workItems", () => ({
  deleteWorkItem: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/sessions/close", () => ({
  closeSession: vi.fn().mockResolvedValue(true),
}));

vi.mock("$lib/tauri", () => ({
  killSession: vi.fn().mockResolvedValue(undefined),
}));

import type { WorkItem } from "$lib/bindings";
import type { Session } from "$lib/types";
import { deleteWorkItem } from "$lib/stores/workItems";
import { sessionState } from "$lib/stores/sessions";
import { closeSession } from "$lib/sessions/close";
import { killSession } from "$lib/tauri";
import { deleteWorkItemWithMode } from "../deleteFlow";

function makeWorkItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "wi-1",
    projectId: null,
    parentId: null,
    branch: null,
    fetchFirst: null,
    title: "Ship the board",
    body: null,
    status: "todo",
    repoPath: null,
    agentProfile: null,
    baseBranch: null,
    worktreePath: null,
    startError: null,
    sessionId: null,
    provider: null,
    externalId: null,
    externalUrl: null,
    sortOrder: 0,
    pinnedPrUrl: null,
    cost: null,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  } as WorkItem;
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "sess-1",
    name: "Task",
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

describe("deleteWorkItemWithMode", () => {
  beforeEach(() => {
    vi.mocked(deleteWorkItem).mockReset().mockResolvedValue(undefined);
    vi.mocked(closeSession).mockReset().mockResolvedValue(true);
    vi.mocked(killSession).mockReset().mockResolvedValue(undefined);
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("deletes only the card in card-only mode", async () => {
    await deleteWorkItemWithMode(
      makeWorkItem({ sessionId: "sess-1" }),
      "card-only",
    );

    expect(closeSession).not.toHaveBeenCalled();
    expect(killSession).not.toHaveBeenCalled();
    expect(deleteWorkItem).toHaveBeenCalledWith("wi-1");
  });

  it("closes an active linked session when requested", async () => {
    const session = makeSession({ id: "sess-1" });
    sessionState.set({ sessions: [session], activeSessionId: session.id });

    await deleteWorkItemWithMode(
      makeWorkItem({ sessionId: session.id }),
      "card-and-stop-session",
    );

    expect(closeSession).toHaveBeenCalledWith(session, {
      force: true,
      preserveWorkItemBoundSession: false,
    });
    expect(killSession).not.toHaveBeenCalled();
    expect(deleteWorkItem).toHaveBeenCalledWith("wi-1");
  });

  it("kills the linked PTY directly when the session is not in the active store", async () => {
    await deleteWorkItemWithMode(
      makeWorkItem({ sessionId: "detached-session" }),
      "card-and-stop-session",
    );

    expect(closeSession).not.toHaveBeenCalled();
    expect(killSession).toHaveBeenCalledWith("detached-session");
    expect(deleteWorkItem).toHaveBeenCalledWith("wi-1");
  });
});
