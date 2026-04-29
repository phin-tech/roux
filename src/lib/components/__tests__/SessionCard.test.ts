import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { PtyInfo, Session } from "$lib/types";
import { notifications } from "$lib/stores/notifications";
import { projects } from "$lib/stores/projects";
import { flashingSessions } from "$lib/stores/watches";
import { sessionLayouts } from "$lib/panes/layout";
import { agentStates } from "$lib/panes/agentState";
import SessionCard from "../SessionCard.svelte";
import { listSessionPtys } from "$lib/tauri";

vi.mock("$lib/tauri", () => ({
  listSessionPtys: vi.fn(),
}));

const mockListSessionPtys = vi.mocked(listSessionPtys);

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "session-1",
    name: "main",
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
    nameOverride: null,
    primaryPtyId: "pty-1",
    archived: false,
    endedAt: null,
    ...overrides,
  };
}

function makePty(overrides: Partial<PtyInfo> = {}): PtyInfo {
  return {
    id: "pty-1",
    session_id: "session-1",
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

describe("SessionCard", () => {
  beforeEach(() => {
    notifications.set([]);
    projects.set([]);
    flashingSessions.set(new Set());
    sessionLayouts.set(new Map());
    agentStates.set(new Map());
    mockListSessionPtys.mockReset();
  });

  afterEach(() => {
    notifications.set([]);
    projects.set([]);
    flashingSessions.set(new Set());
    sessionLayouts.set(new Map());
    agentStates.set(new Map());
    mockListSessionPtys.mockReset();
  });

  it("shows active and detached inventory separately when a session has detached terminals", async () => {
    mockListSessionPtys.mockResolvedValue([
      makePty({ id: "pty-1", status: { type: "RunningAttached", pane_id: "pane-1" } }),
      makePty({ id: "pty-2", role: "secondary", status: { type: "RunningAttached", pane_id: "pane-2" } }),
      makePty({
        id: "pty-3",
        role: "secondary",
        status: { type: "RunningDetached", since_ms: 123 },
        unread_output: true,
      }),
    ]);

    render(SessionCard, {
      session: makeSession(),
      active: false,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect: () => {},
    });

    const activeBadge = await screen.findByTitle("2 active panes");
    expect(activeBadge.textContent).toBe("2");

    const detachedBadge = await screen.findByTitle("1 detached terminal (unread output)");
    expect(detachedBadge.textContent).toBe("1");
  });

  it("hides the pane inventory badge for an ordinary single-pane session", async () => {
    mockListSessionPtys.mockResolvedValue([
      makePty({ id: "pty-1", status: { type: "RunningAttached", pane_id: "pane-1" } }),
    ]);

    render(SessionCard, {
      session: makeSession(),
      active: false,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect: () => {},
    });

    await waitFor(() => expect(mockListSessionPtys).toHaveBeenCalledWith("session-1"));
    expect(screen.queryByTitle("1 active pane")).toBeNull();
    expect(screen.queryByTitle(/detached terminal/)).toBeNull();
  });

  it("labels disconnected session action as continue", async () => {
    mockListSessionPtys.mockResolvedValue([
      makePty({ id: "pty-1", status: { type: "RunningAttached", pane_id: "pane-1" } }),
    ]);
    const onreconnect = vi.fn();

    render(SessionCard, {
      session: makeSession({ status: "disconnected" }),
      active: false,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect,
    });

    const button = screen.getByRole("button", { name: "continue" });
    await fireEvent.click(button);

    expect(onreconnect).toHaveBeenCalledTimes(1);
  });
});
