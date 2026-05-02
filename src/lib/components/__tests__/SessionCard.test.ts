import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Notification, Session, WorktrunkMetadata } from "$lib/types";
import { notifications } from "$lib/stores/notifications";
import { projects } from "$lib/stores/projects";
import { flashingSessions } from "$lib/stores/watches";
import { sessionLayouts } from "$lib/panes/layout";
import { agentStates } from "$lib/panes/agentState";
import {
  _resetWorktreeMetadataForTests,
  upsertWorktreeMetadata,
} from "$lib/stores/worktreeMetadata";
import {
  _resetPtyInventoryForTests,
  ptyInventoryBySession,
} from "$lib/stores/ptyInventory";
import SessionCard from "../SessionCard.svelte";

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

function makeMetadata(overrides: Partial<WorktrunkMetadata> = {}): WorktrunkMetadata {
  return {
    dirty: false,
    ahead: 0,
    behind: 0,
    locked: false,
    lockReason: null,
    prunable: false,
    prunableReason: null,
    isCurrent: false,
    isPrevious: false,
    devServerUrl: null,
    mainState: null,
    ciStatus: null,
    ciUrl: null,
    ciStale: false,
    ...overrides,
  };
}

function makeNotification(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "notification-1",
    createdAt: 1,
    level: "info",
    source: { type: "internal" },
    title: "Heads up",
    subtitle: null,
    body: null,
    sessionId: "session-1",
    read: false,
    actions: [],
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
    _resetWorktreeMetadataForTests();
    _resetPtyInventoryForTests();
  });

  afterEach(() => {
    notifications.set([]);
    projects.set([]);
    flashingSessions.set(new Set());
    sessionLayouts.set(new Map());
    agentStates.set(new Map());
    _resetWorktreeMetadataForTests();
    _resetPtyInventoryForTests();
  });

  it("shows active and detached inventory separately when a session has detached terminals", () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 2, detachedCount: 1, detachedHasUnread: true }],
    ]));

    render(SessionCard, {
      session: makeSession(),
      active: false,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect: () => {},
    });

    const activeBadge = screen.getByTitle("2 active panes");
    expect(activeBadge.textContent).toBe("2");

    const detachedBadge = screen.getByTitle("1 detached terminal (unread output)");
    expect(detachedBadge.textContent).toBe("1");
  });

  it("hides the pane inventory badge for an ordinary single-pane session", () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 1, detachedCount: 0, detachedHasUnread: false }],
    ]));

    render(SessionCard, {
      session: makeSession(),
      active: false,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect: () => {},
    });

    expect(screen.queryByTitle("1 active pane")).toBeNull();
    expect(screen.queryByTitle(/detached terminal/)).toBeNull();
  });

  it("renders worktree identity and metadata chips for a worktree session", () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 1, detachedCount: 0, detachedHasUnread: false }],
    ]));
    upsertWorktreeMetadata([
      {
        path: "/repo/.worktrees/restore-closed-sessions",
        branch: "feature/restore-closed-sessions",
        isMain: false,
        worktrunk: makeMetadata({ dirty: true, ahead: 1, behind: 5 }),
      },
    ]);

    const { container } = render(SessionCard, {
      session: makeSession({
        worktreePath: "/repo/.worktrees/restore-closed-sessions",
        branch: "feature/restore-closed-sessions",
        isWorktree: true,
      }),
      active: true,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect: () => {},
    });

    expect(container.textContent).toContain("feature/");
    expect(container.textContent).toContain("restore-closed-sessions");
    expect(screen.getByText("worktree")).toBeDefined();
    expect(screen.getByTestId("session-wt-dirty")).toBeDefined();
    expect(screen.getByTestId("session-wt-ahead-behind").textContent).toContain("↑1");
    expect(screen.getByTestId("session-wt-ahead-behind").textContent).toContain("↓5");
  });

  it("does not branch-split custom names that contain slashes", () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 1, detachedCount: 0, detachedHasUnread: false }],
    ]));

    render(SessionCard, {
      session: makeSession({
        worktreePath: "/repo/.worktrees/restore-closed-sessions",
        branch: "feature/restore-closed-sessions",
        isWorktree: true,
        nameOverride: "notes/design",
      }),
      active: false,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect: () => {},
    });

    expect(screen.getByTestId("session-primary-label").textContent).toBe("notes/design");
    expect(screen.queryByTestId("session-primary-prefix")).toBeNull();
    expect(screen.getByText("feature/restore-closed-sessions")).toBeDefined();
  });

  it("keeps rename and close controls accessible", async () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 1, detachedCount: 0, detachedHasUnread: false }],
    ]));
    const onrename = vi.fn();
    const onclose = vi.fn();

    render(SessionCard, {
      session: makeSession({ branch: "feature/restore-closed-sessions" }),
      active: false,
      onselect: () => {},
      onclose,
      onrename,
      onreconnect: () => {},
    });

    await fireEvent.click(screen.getByLabelText("Rename session"));
    const input = screen.getByDisplayValue("feature/restore-closed-sessions");
    await fireEvent.input(input, { target: { value: "Renamed session" } });
    await fireEvent.blur(input);
    expect(onrename).toHaveBeenCalledWith("Renamed session");

    await fireEvent.click(screen.getByLabelText("Close session"));
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it("renders unread notifications independently from pane inventory", () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 2, detachedCount: 0, detachedHasUnread: false }],
    ]));
    notifications.set([
      makeNotification({ id: "notification-1", sessionId: "session-1" }),
      makeNotification({ id: "notification-2", sessionId: "session-1" }),
    ]);

    render(SessionCard, {
      session: makeSession(),
      active: false,
      onselect: () => {},
      onclose: () => {},
      onrename: () => {},
      onreconnect: () => {},
    });

    expect(screen.getByTitle("2 active panes").textContent).toBe("2");
    expect(screen.getByTitle("2 unread notifications").textContent).toBe("2");
  });

  it("labels disconnected session action as continue", async () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 1, detachedCount: 0, detachedHasUnread: false }],
    ]));
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
