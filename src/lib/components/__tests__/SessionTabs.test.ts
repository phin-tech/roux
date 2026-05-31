import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import SessionTabs from "../SessionTabs.svelte";
import { sessionState } from "$lib/stores/sessions";
import { projects } from "$lib/stores/projects";
import { openMainView } from "$lib/stores/mainView";
import type { Session } from "$lib/types";

vi.mock("$lib/stores/mainView", () => ({
  openMainView: vi.fn(),
}));

vi.mock("$lib/stores/tasks", () => ({
  refreshTasks: vi.fn(),
  initTaskOverrides: vi.fn(),
}));

vi.mock("$lib/tauri", () => ({
  createSessionShell: vi.fn(),
  openInEditor: vi.fn(),
  refreshSessionGitStatus: vi.fn().mockResolvedValue(true),
  setSessionProject: vi.fn().mockResolvedValue(undefined),
  listArchivedSessions: vi.fn().mockResolvedValue([]),
  listSessions: vi.fn().mockResolvedValue([]),
  openPathInFinder: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/sessions/close", () => ({
  closeSession: vi.fn(),
}));

vi.mock("$lib/sessions/reconnect", () => ({
  continueSession: vi.fn(),
}));

vi.mock("$lib/sessions/spawnBlueprint", () => ({
  spawnBlueprintForProject: vi.fn(),
}));

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "session-1",
    name: "feature-a",
    repoRoot: "/repo",
    worktreePath: "/repo/.worktrees/feature-a",
    branch: "feature-a",
    isWorktree: true,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: null,
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: "pty-1",
    archived: false,
    endedAt: null,
    ...overrides,
  };
}

describe("SessionTabs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    projects.set([]);
    sessionState.set({
      sessions: [
        makeSession(),
        makeSession({
          id: "session-2",
          name: "feature-b",
          worktreePath: "/repo/.worktrees/feature-b",
          branch: "feature-b",
          primaryPtyId: "pty-2",
        }),
      ],
      activeSessionId: "session-2",
    });
  });

  it("opens session details from the session context menu", async () => {
    render(SessionTabs, { onNewSession: vi.fn() });

    await fireEvent.contextMenu(screen.getByText("feature-a"));
    await fireEvent.click(screen.getByText("Session Details"));

    expect(openMainView).toHaveBeenCalledWith({
      kind: "sessionDetail",
      sessionId: "session-1",
    });
    expect(get(sessionState).activeSessionId).toBe("session-1");
  });
});
