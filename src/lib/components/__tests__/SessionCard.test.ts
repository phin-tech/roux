import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type {
  GroupBy,
  Notification,
  RouxSettings,
  Session,
  WorktrunkMetadata,
} from "$lib/types";
import { DEFAULT_SETTINGS, EXPERIMENT_DEFAULTS } from "$lib/types";
import { notifications } from "$lib/stores/notifications";
import { projects } from "$lib/stores/projects";
import { settings } from "$lib/stores/settings";
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

type RenderProps = {
  session?: Session;
  active?: boolean;
  groupBy?: GroupBy;
  onselect?: () => void;
  onclose?: () => void;
  onrename?: (newName: string) => void;
  onreconnect?: () => void;
};

function renderCard(overrides: RenderProps = {}) {
  return render(SessionCard, {
    session: overrides.session ?? makeSession(),
    active: overrides.active ?? false,
    groupBy: overrides.groupBy ?? "repo",
    onselect: overrides.onselect ?? (() => {}),
    onclose: overrides.onclose ?? (() => {}),
    onrename: overrides.onrename ?? (() => {}),
    onreconnect: overrides.onreconnect ?? (() => {}),
  });
}

function setSimplifiedFlag(enabled: boolean) {
  settings.update((s: RouxSettings) => ({
    ...s,
    experiments: { ...EXPERIMENT_DEFAULTS, simplifiedSessionTabs: enabled },
  }));
}

function resetStores() {
  notifications.set([]);
  projects.set([]);
  flashingSessions.set(new Set());
  sessionLayouts.set(new Map());
  agentStates.set(new Map());
  settings.set(DEFAULT_SETTINGS);
  _resetWorktreeMetadataForTests();
  _resetPtyInventoryForTests();
}

describe("SessionCard — legacy rendering (simplifiedSessionTabs off)", () => {
  beforeEach(() => {
    resetStores();
  });

  afterEach(() => {
    resetStores();
  });

  it("shows active and detached inventory separately when a session has detached terminals", () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 2, detachedCount: 1, detachedHasUnread: true }],
    ]));

    renderCard();

    const activeBadge = screen.getByTitle("2 active panes");
    expect(activeBadge.textContent).toBe("2");

    const detachedBadge = screen.getByTitle("1 detached terminal (unread output)");
    expect(detachedBadge.textContent).toBe("1");
  });

  it("hides the pane inventory badge for an ordinary single-pane session", () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 1, detachedCount: 0, detachedHasUnread: false }],
    ]));

    renderCard();

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

    const { container } = renderCard({
      session: makeSession({
        worktreePath: "/repo/.worktrees/restore-closed-sessions",
        branch: "feature/restore-closed-sessions",
        isWorktree: true,
      }),
      active: true,
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

    renderCard({
      session: makeSession({
        worktreePath: "/repo/.worktrees/restore-closed-sessions",
        branch: "feature/restore-closed-sessions",
        isWorktree: true,
        nameOverride: "notes/design",
      }),
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

    renderCard({
      session: makeSession({ branch: "feature/restore-closed-sessions" }),
      onrename,
      onclose,
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

    renderCard();

    expect(screen.getByTitle("2 active panes").textContent).toBe("2");
    expect(screen.getByTitle("2 unread notifications").textContent).toBe("2");
  });

  it("labels disconnected session action as continue", async () => {
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 1, detachedCount: 0, detachedHasUnread: false }],
    ]));
    const onreconnect = vi.fn();

    renderCard({
      session: makeSession({ status: "disconnected" }),
      onreconnect,
    });

    const button = screen.getByRole("button", { name: "continue" });
    await fireEvent.click(button);

    expect(onreconnect).toHaveBeenCalledTimes(1);
  });
});

describe("SessionCard — simplified rendering (simplifiedSessionTabs on)", () => {
  beforeEach(() => {
    resetStores();
    setSimplifiedFlag(true);
    ptyInventoryBySession.set(new Map([
      ["session-1", { attachedCount: 2, detachedCount: 1, detachedHasUnread: true }],
    ]));
  });

  afterEach(() => {
    resetStores();
  });

  it("hides pane inventory badges and worktree metadata chips", () => {
    upsertWorktreeMetadata([
      {
        path: "/repo/.worktrees/feature-x",
        branch: "feature/x",
        isMain: false,
        worktrunk: makeMetadata({ dirty: true, ahead: 2, behind: 0 }),
      },
    ]);

    renderCard({
      session: makeSession({
        worktreePath: "/repo/.worktrees/feature-x",
        branch: "feature/x",
        isWorktree: true,
      }),
    });

    expect(screen.queryByTitle(/active pane/)).toBeNull();
    expect(screen.queryByTitle(/detached terminal/)).toBeNull();
    expect(screen.queryByText("worktree")).toBeNull();
    expect(screen.queryByTestId("session-wt-dirty")).toBeNull();
    expect(screen.queryByTestId("session-wt-ahead-behind")).toBeNull();
  });

  it("shows the worktree directory name as secondary when grouping by repo", () => {
    renderCard({
      session: makeSession({
        worktreePath: "/repo/.worktrees/feature-x",
        branch: "feature/x",
        isWorktree: true,
      }),
      groupBy: "repo",
    });

    expect(screen.getByTestId("session-secondary-context").textContent).toBe("feature-x");
  });

  it("shows the worktree directory name as secondary when grouping by session", () => {
    renderCard({
      session: makeSession({
        worktreePath: "/repo/.worktrees/feature-x",
        branch: "feature/x",
        isWorktree: true,
      }),
      groupBy: "session",
    });

    expect(screen.getByTestId("session-secondary-context").textContent).toBe("feature-x");
  });

  it("derives the basename from Windows-style paths", () => {
    renderCard({
      session: makeSession({
        repoRoot: "C:\\src\\cool-repo",
        worktreePath: "C:\\src\\cool-repo\\.worktrees\\feature-x",
        branch: "feature/x",
        isWorktree: true,
      }),
      groupBy: "repo",
    });

    expect(screen.getByTestId("session-secondary-context").textContent).toBe("feature-x");
  });

  it("shows the repo name as secondary when grouping by project", () => {
    renderCard({
      session: makeSession({
        repoRoot: "/Users/me/code/cool-repo",
        worktreePath: "/Users/me/code/cool-repo/.worktrees/feature-x",
        branch: "feature/x",
        isWorktree: true,
      }),
      groupBy: "project",
    });

    expect(screen.getByTestId("session-secondary-context").textContent).toBe("cool-repo");
  });

  it("renders branch and contextual name together when a custom name overrides the branch", () => {
    renderCard({
      session: makeSession({
        worktreePath: "/repo/.worktrees/restore-closed-sessions",
        branch: "feature/restore-closed-sessions",
        isWorktree: true,
        nameOverride: "notes/design",
      }),
      groupBy: "repo",
    });

    expect(screen.getByTestId("session-primary-label").textContent).toBe("notes/design");
    expect(screen.getByTestId("session-secondary-branch").textContent).toBe(
      "feature/restore-closed-sessions",
    );
    expect(screen.getByTestId("session-secondary-context").textContent).toBe(
      "restore-closed-sessions",
    );
  });

  it("keeps rename, close, and continue controls accessible", async () => {
    const onrename = vi.fn();
    const onclose = vi.fn();
    const onreconnect = vi.fn();

    renderCard({
      session: makeSession({
        branch: "feature/restore-closed-sessions",
        status: "disconnected",
      }),
      onrename,
      onclose,
      onreconnect,
    });

    const continueButton = screen.getByRole("button", { name: "continue" });
    await fireEvent.click(continueButton);
    expect(onreconnect).toHaveBeenCalledTimes(1);

    await fireEvent.click(screen.getByLabelText("Rename session"));
    const input = screen.getByDisplayValue("feature/restore-closed-sessions");
    await fireEvent.input(input, { target: { value: "Renamed session" } });
    await fireEvent.blur(input);
    expect(onrename).toHaveBeenCalledWith("Renamed session");

    await fireEvent.click(screen.getByLabelText("Close session"));
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});
