import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardMainView from "../BoardMainView.svelte";
import {
  itemsByColumn,
  acceptWorkItemReview,
  requestWorkItemChanges,
  moveWorkItem,
  planWorkItem,
  startWorkItem,
  stopWorkItemRun,
  activePlanningRunByItem,
  attachmentsByWorkItem,
  pendingDecisionByItem,
  runsByItem,
  workItemRunEvents,
} from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import {
  openNewWorkItemEditor,
  openWorkItemSessionStart,
} from "$lib/stores/ui";
import { closeMainView } from "$lib/stores/mainView";
import { openSessionById } from "$lib/panes/openSession";
import { WORK_ITEM_DRAG_MIME } from "$lib/board/drag";
import type { WorkItem } from "$lib/bindings";
import type { Notification } from "$lib/types";
import type { Attachment, WorkItemRun } from "$lib/types/workItems";
import { notifications } from "$lib/stores/notifications";
import { createSessionShell, openPathInFinder } from "$lib/tauri";
import { addSession, setActiveSession } from "$lib/stores/sessions";
import { initSessionWithProfile } from "$lib/panes/actions";
import { connectPaneTerminal } from "$lib/panes/terminals";
import { runProfileInPane } from "$lib/panes/profileRunner";

// jsdom lacks the Web Animations API that Svelte's transition:fade/scale use.
if (typeof Element !== "undefined" && !Element.prototype.animate) {
  Element.prototype.animate = () =>
    ({
      cancel() {},
      play() {},
      pause() {},
      finished: Promise.resolve(),
      onfinish: null,
      currentTime: 0,
      playState: "finished",
      addEventListener() {},
      removeEventListener() {},
    }) as unknown as Animation;
}

// itemsByColumn is replaced with a plain writable so the test can drive the
// columns directly; moveWorkItem / closeMainView become spies.
vi.mock("$lib/stores/workItems", async () => {
  const { writable } = await import("svelte/store");
  return {
    WORK_ITEM_COLUMNS: ["todo", "ready", "doing", "review", "done"],
    COLUMN_LABELS: {
      todo: "To Do",
      ready: "Planning",
      doing: "In Progress",
      review: "Review",
      done: "Done",
    },
    itemsByColumn: writable(new Map()),
    pendingDecisionByItem: writable(new Map()),
    activePlanningRunByItem: writable(new Map()),
    attachmentsByWorkItem: writable(new Map()),
    runsByItem: writable(new Map()),
    workItemRunEvents: writable([]),
    acceptWorkItemReview: vi.fn().mockResolvedValue({}),
    requestWorkItemChanges: vi.fn().mockResolvedValue({}),
    moveWorkItem: vi.fn().mockResolvedValue({}),
    planWorkItem: vi.fn().mockResolvedValue("plan-sess-1"),
    startWorkItem: vi.fn().mockResolvedValue("sess-1"),
    stopWorkItemRun: vi.fn().mockResolvedValue({}),
    createWorkItem: vi.fn().mockResolvedValue({}),
  };
});

vi.mock("$lib/workItems/deleteFlow", () => ({
  deleteWorkItemWithMode: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/stores/sessions", async () => {
  const { writable } = await import("svelte/store");
  return {
    sessionList: writable([]),
    addSession: vi.fn(),
    setActiveSession: vi.fn(),
  };
});

vi.mock("$lib/stores/ui", () => ({
  openNewWorkItemEditor: vi.fn(),
  openWorkItemEditor: vi.fn(),
  openWorkItemSessionStart: vi.fn(),
}));

vi.mock("$lib/stores/mainView", () => ({
  closeMainView: vi.fn(),
}));

vi.mock("$lib/panes/openSession", () => ({
  openSessionById: vi.fn().mockResolvedValue("opened"),
}));

vi.mock("$lib/tauri", () => ({
  createSessionShell: vi.fn().mockResolvedValue({
    id: "review-agent-session",
    name: "Review me review",
    worktreePath: "/repo/.worktrees/review-card",
    repoRoot: "/repo",
    branch: "feature/review-card",
    isGitRepo: true,
    status: "idle",
    createdAt: 1,
  }),
  openPathInFinder: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/panes/actions", () => ({
  initSessionWithProfile: vi.fn().mockReturnValue("review-agent-session"),
}));

vi.mock("$lib/panes/terminals", () => ({
  connectPaneTerminal: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/panes/profileRunner", () => ({
  runProfileInPane: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/panes/profiles", async () => {
  const { writable } = await import("svelte/store");
  const profile = {
    id: "claude",
    name: "Claude",
    setupCommand: null,
    startupCommand: "claude",
    startupBehavior: null,
    env: null,
    cwdOverride: null,
    icon: null,
    provider: "claude",
    source: "builtin",
  };
  return {
    profileList: writable([profile]),
    resolveProfileRef: vi.fn(() => profile),
  };
});

function workItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "wi-1",
    projectId: null,
    parentId: null,
    title: "Ship the board",
    body: null,
    status: "todo",
    repoPath: null,
    agentProfile: null,
    baseBranch: null,
    worktreePath: null,
    branch: null,
    fetchFirst: null,
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

function workItemRun(overrides: Partial<WorkItemRun> = {}): WorkItemRun {
  return {
    id: "run-1",
    workItemId: "wi-1",
    kind: "implementation",
    sessionId: "sess-1",
    ptyId: "sess-1",
    provider: "claude",
    profileId: "claude",
    status: "running",
    worktreePath: "/repo",
    branch: "main",
    cost: null,
    createdAt: 1,
    startedAt: 1,
    endedAt: null,
    updatedAt: 1,
    ...overrides,
  };
}

function attachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: "att-1",
    documentId: "wi-1.att-1",
    targetKind: "workItem",
    targetId: "wi-1",
    title: "Plan",
    contentKind: "text",
    mimeType: "text/markdown",
    sourcePath: null,
    byteLen: 12,
    sha256: "sha",
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

function notification(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "notification-1",
    createdAt: 1,
    level: "info",
    source: { type: "internal" },
    title: "Heads up",
    subtitle: null,
    body: null,
    sessionId: "sess-1",
    read: false,
    actions: [],
    ...overrides,
  };
}

function seedColumns(items: WorkItem[]) {
  const map = new Map<string, WorkItem[]>();
  for (const col of ["todo", "ready", "doing", "review", "done"])
    map.set(col, []);
  for (const item of items) map.get(item.status)?.push(item);
  (itemsByColumn as ReturnType<typeof import("svelte/store").writable>).set(
    map,
  );
}

function seedWorkItemAttachments(entries: Array<[string, Attachment[]]>): void {
  (
    attachmentsByWorkItem as ReturnType<typeof import("svelte/store").writable>
  ).set(new Map(entries));
}

function dragData(payload: {
  itemId: string;
  fromStatus: string;
}): DataTransfer {
  return {
    getData: (type: string) =>
      type === WORK_ITEM_DRAG_MIME ? JSON.stringify(payload) : "",
    types: [WORK_ITEM_DRAG_MIME],
    dropEffect: "none",
  } as unknown as DataTransfer;
}

describe("BoardMainView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seedColumns([]);
    (
      activePlanningRunByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(new Map());
    (
      pendingDecisionByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(new Map());
    seedWorkItemAttachments([]);
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map(),
    );
    (workItemRunEvents as ReturnType<typeof import("svelte/store").writable>).set(
      [],
    );
    notifications.set([]);
  });

  it("renders one section per column with labels", () => {
    render(BoardMainView);
    expect(screen.getAllByTestId("board-column")).toHaveLength(5);
    expect(screen.getByText("To Do")).toBeTruthy();
    expect(screen.getByText("Planning")).toBeTruthy();
    expect(screen.getByText("In Progress")).toBeTruthy();
    expect(screen.getByText("Review")).toBeTruthy();
    expect(screen.getByText("Done")).toBeTruthy();
  });

  it("uses Plan as the primary action for To Do cards", async () => {
    seedColumns([workItem({ id: "wi-1", status: "todo" })]);
    render(BoardMainView);

    expect(screen.getByLabelText("Plan work item")).toBeTruthy();
    expect(screen.queryByLabelText("Move to Planning")).toBeNull();

    await fireEvent.click(screen.getByLabelText("Plan work item"));

    expect(planWorkItem).toHaveBeenCalledWith("wi-1");
    expect(startWorkItem).not.toHaveBeenCalled();
  });

  it("moves a card to the next dropped-on column", async () => {
    seedColumns([workItem({ id: "wi-1", status: "todo" })]);
    render(BoardMainView);

    const planningColumn = document.querySelector('[data-column="ready"]')!;
    await fireEvent.drop(planningColumn, {
      dataTransfer: dragData({ itemId: "wi-1", fromStatus: "todo" }),
    });

    expect(moveWorkItem).toHaveBeenCalledWith(
      "wi-1",
      "ready",
      expect.any(Number),
    );
  });

  it("ignores a drop that skips workflow columns", async () => {
    seedColumns([workItem({ id: "wi-1", status: "todo" })]);
    render(BoardMainView);

    const doneColumn = document.querySelector('[data-column="done"]')!;
    await fireEvent.drop(doneColumn, {
      dataTransfer: dragData({ itemId: "wi-1", fromStatus: "todo" }),
    });

    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("ignores a drop onto the card's current column", async () => {
    seedColumns([workItem({ id: "wi-1", status: "todo" })]);
    render(BoardMainView);

    const todoColumn = document.querySelector('[data-column="todo"]')!;
    await fireEvent.drop(todoColumn, {
      dataTransfer: dragData({ itemId: "wi-1", fromStatus: "todo" }),
    });

    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("Approve and start delegates to daemon start without issuing a second move", async () => {
    seedColumns(
      [
        workItem({
          id: "wi-1",
          status: "ready",
          projectId: "proj-1",
          sessionId: null,
        }),
      ].map((item) => ({ ...item, agentProfile: "claude" }) as WorkItem),
    );
    seedWorkItemAttachments([["wi-1", [attachment({ targetId: "wi-1" })]]]);
    render(BoardMainView);

    await fireEvent.click(screen.getByLabelText("Approve and start work item"));

    expect(startWorkItem).toHaveBeenCalledWith("wi-1");
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("shows an inline error when Start dispatch fails", async () => {
    vi.mocked(startWorkItem).mockRejectedValueOnce(
      new Error("project not found"),
    );
    seedColumns(
      [
        workItem({
          id: "wi-1",
          status: "ready",
          projectId: "proj-1",
          sessionId: null,
        }),
      ].map((item) => ({ ...item, agentProfile: "claude" }) as WorkItem),
    );
    seedWorkItemAttachments([["wi-1", [attachment({ targetId: "wi-1" })]]]);
    render(BoardMainView);

    await fireEvent.click(screen.getByLabelText("Approve and start work item"));

    expect(startWorkItem).toHaveBeenCalledWith("wi-1");
    await vi.waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain(
        "The assigned project no longer exists.",
      ),
    );
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("keeps a planning card in its planning terminal until a plan is attached", async () => {
    seedColumns([
      workItem({
        id: "wi-plan",
        status: "ready",
        projectId: "proj-1",
        sessionId: null,
        agentProfile: "claude",
      }),
    ]);
    (
      activePlanningRunByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        [
          "wi-plan",
          workItemRun({
            id: "run-plan",
            workItemId: "wi-plan",
            kind: "planning",
            sessionId: "plan-sess-1",
          }),
        ],
      ]),
    );
    render(BoardMainView);

    expect(screen.queryByLabelText("Approve and start work item")).toBeNull();
    await fireEvent.click(screen.getByLabelText("Open planning terminal"));

    expect(openSessionById).toHaveBeenCalledWith("plan-sess-1");
    expect(startWorkItem).not.toHaveBeenCalled();
  });

  it("stops the planning run before starting implementation when a plan is attached", async () => {
    seedColumns([
      workItem({
        id: "wi-plan",
        status: "ready",
        projectId: "proj-1",
        sessionId: null,
        agentProfile: "claude",
      }),
    ]);
    seedWorkItemAttachments([
      ["wi-plan", [attachment({ targetId: "wi-plan", title: "Plan" })]],
    ]);
    (
      activePlanningRunByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        [
          "wi-plan",
          workItemRun({
            id: "run-plan",
            workItemId: "wi-plan",
            kind: "planning",
            sessionId: "plan-sess-1",
          }),
        ],
      ]),
    );
    render(BoardMainView);

    await fireEvent.click(screen.getByLabelText("Approve and start work item"));

    expect(stopWorkItemRun).toHaveBeenCalledWith("run-plan");
    expect(startWorkItem).toHaveBeenCalledWith("wi-plan");
  });

  it("offers a force start action for planning cards without attached plans", async () => {
    seedColumns([
      workItem({
        id: "wi-plan",
        status: "ready",
        projectId: "proj-1",
        sessionId: null,
        agentProfile: "claude",
      }),
    ]);
    (
      activePlanningRunByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        [
          "wi-plan",
          workItemRun({
            id: "run-plan",
            workItemId: "wi-plan",
            kind: "planning",
            sessionId: "plan-sess-1",
          }),
        ],
      ]),
    );
    render(BoardMainView);

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"));
    await fireEvent.click(
      screen.getByRole("menuitem", { name: "Approve & start anyway" }),
    );

    expect(stopWorkItemRun).toHaveBeenCalledWith("run-plan");
    expect(startWorkItem).toHaveBeenCalledWith("wi-plan", {
      forceStart: true,
    });
  });

  it("opens the session prompt when Start is clicked on an unprojected card", async () => {
    const item = workItem({
      id: "wi-1",
      title: "Wire task start",
      status: "ready",
      projectId: null,
      sessionId: null,
    });
    seedColumns([item]);
    render(BoardMainView);

    expect(screen.getByText("Configure")).toBeTruthy();
    await fireEvent.click(screen.getByLabelText("Configure work item"));

    expect(openWorkItemSessionStart).toHaveBeenCalledWith({
      itemId: "wi-1",
      title: "Wire task start",
    });
    expect(startWorkItem).not.toHaveBeenCalled();
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("shows unread activity from any attached run session and opens that session", async () => {
    seedColumns([workItem({ id: "wi-activity", title: "Needs attention" })]);
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map([
        [
          "wi-activity",
          [
            workItemRun({
              id: "run-plan",
              workItemId: "wi-activity",
              kind: "planning",
              sessionId: "plan-sess-1",
              status: "stopped",
            }),
            workItemRun({
              id: "run-impl",
              workItemId: "wi-activity",
              kind: "implementation",
              sessionId: "impl-sess-1",
            }),
          ],
        ],
      ]),
    );
    notifications.set([
      notification({ id: "n-plan", sessionId: "plan-sess-1" }),
      notification({ id: "n-impl", sessionId: "impl-sess-1" }),
      notification({ id: "n-other", sessionId: "other-sess" }),
    ]);
    render(BoardMainView);

    const badge = screen.getByLabelText("Open session with unread activity");
    expect(badge.textContent).toBe("2");
    expect(
      screen.getByTitle("2 unread notifications across 2 attached sessions"),
    ).toBe(badge);

    await fireEvent.click(badge);

    expect(openSessionById).toHaveBeenCalledWith("plan-sess-1");
    await vi.waitFor(() => expect(closeMainView).toHaveBeenCalled());
  });

  it("does not delete from the right-click menu when the delete dialog is canceled", async () => {
    seedColumns([workItem({ id: "wi-delete", title: "Keep me" })]);
    render(BoardMainView);

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"), {
      clientX: 64,
      clientY: 96,
    });
    await fireEvent.click(
      screen.getByRole("menuitem", { name: "Delete card" }),
    );
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(deleteWorkItemWithMode).not.toHaveBeenCalled();
  });

  it("shows Open terminal for a dispatched card and opens + closes the board", async () => {
    seedColumns([
      workItem({ id: "wi-1", status: "doing", sessionId: "sess-1" }),
    ]);
    render(BoardMainView);

    // Session-bound cards show Open terminal, not Start.
    expect(screen.queryByLabelText("Start work item")).toBeNull();
    await fireEvent.click(screen.getByLabelText("Open terminal"));

    expect(openSessionById).toHaveBeenCalledWith("sess-1");
    await vi.waitFor(() => expect(closeMainView).toHaveBeenCalled());
  });

  it("accepts review from the card primary action without directly moving the card", async () => {
    seedColumns([
      workItem({
        id: "wi-review",
        title: "Review me",
        status: "review",
        sessionId: "sess-1",
      }),
    ]);
    render(BoardMainView);

    await fireEvent.click(screen.getByLabelText("Accept work item review"));

    expect(acceptWorkItemReview).toHaveBeenCalledWith("wi-review");
    expect(moveWorkItem).not.toHaveBeenCalledWith(
      "wi-review",
      "done",
      expect.any(Number),
    );
  });

  it("shows review package details and requests changes with a note", async () => {
    seedColumns([
      workItem({
        id: "wi-review",
        title: "Review me",
        status: "review",
        repoPath: "/repo",
        sessionId: null,
        pinnedPrUrl: "https://github.com/phin-tech/roux/pull/90",
      }),
    ]);
    seedWorkItemAttachments([
      [
        "wi-review",
        [
          attachment({
            id: "plan-1",
            targetId: "wi-review",
            title: "Implementation Plan",
            documentId: "wi-review.plan",
          }),
          attachment({
            id: "feedback-1",
            targetId: "wi-review",
            title: "Review feedback",
            documentId: "wi-review.feedback",
            updatedAt: 2,
          }),
        ],
      ],
    ]);
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map([
        [
          "wi-review",
          [
            workItemRun({
              id: "run-review",
              workItemId: "wi-review",
              status: "review",
              sessionId: "sess-review",
              worktreePath: "/repo/.worktrees/review-card",
              branch: "feature/review-card",
            }),
          ],
        ],
      ]),
    );
    (workItemRunEvents as ReturnType<typeof import("svelte/store").writable>).set(
      [
        {
          id: "event-1",
          runId: "run-review",
          kind: "result",
          payload: {
            summary: "Implemented review package.",
            tests: ["npm run test"],
            changedFiles: ["src/lib/components/WorkItemCard.svelte"],
          },
          createdAt: 3,
        },
      ],
    );
    render(BoardMainView);

    const reviewPackage = screen.getByTestId("work-item-review-package");
    expect(reviewPackage).toBeTruthy();
    expect(screen.getByText("Plan")).toBeTruthy();
    expect(
      within(reviewPackage).queryByText("Implementation Plan"),
    ).toBeNull();
    expect(
      within(reviewPackage).getByText("Implemented review package."),
    ).toBeTruthy();
    expect(within(reviewPackage).getByText("npm run test")).toBeTruthy();
    expect(within(reviewPackage).getByText("feature/review-card")).toBeTruthy();

    await fireEvent.click(screen.getByText("Open worktree"));
    expect(openPathInFinder).toHaveBeenCalledWith(
      "/repo/.worktrees/review-card",
    );

    await fireEvent.click(screen.getByText("Open agent"));
    await vi.waitFor(() =>
      expect(createSessionShell).toHaveBeenCalledWith(
        "/repo",
        "Review me review",
        "/repo/.worktrees/review-card",
        "feature/review-card",
        { profile: "claude" },
      ),
    );
    expect(addSession).toHaveBeenCalledWith(
      expect.objectContaining({ id: "review-agent-session" }),
    );
    expect(initSessionWithProfile).toHaveBeenCalledWith(
      "review-agent-session",
      { kind: "registered", id: "claude" },
    );
    expect(connectPaneTerminal).toHaveBeenCalledWith("review-agent-session");
    expect(runProfileInPane).toHaveBeenCalled();
    expect(setActiveSession).toHaveBeenCalledWith("review-agent-session");
    expect(closeMainView).toHaveBeenCalled();

    await fireEvent.click(screen.getByText("Request changes"));
    const form = screen.getByTestId("work-item-request-changes-form");
    await fireEvent.input(screen.getByPlaceholderText("Requested changes"), {
      target: { value: "Add retry coverage." },
    });
    await fireEvent.click(
      within(form).getByRole("button", { name: "Request changes" }),
    );

    expect(requestWorkItemChanges).toHaveBeenCalledWith(
      "wi-review",
      "Add retry coverage.",
    );
  });

  it("shows Open planning terminal for an active planning run", async () => {
    seedColumns([workItem({ id: "wi-plan", status: "todo", sessionId: null })]);
    (
      activePlanningRunByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        [
          "wi-plan",
          {
            id: "run-plan",
            workItemId: "wi-plan",
            kind: "planning",
            sessionId: "plan-sess-1",
            provider: "claude",
            profileId: "claude",
            status: "running",
            worktreePath: "/repo",
            branch: "main",
            cost: null,
            createdAt: 1,
            startedAt: 1,
            endedAt: null,
            updatedAt: 1,
          },
        ],
      ]),
    );
    render(BoardMainView);

    expect(screen.queryByLabelText("Start work item")).toBeNull();
    await fireEvent.click(screen.getByLabelText("Open planning terminal"));

    expect(openSessionById).toHaveBeenCalledWith("plan-sess-1");
    await vi.waitFor(() => expect(closeMainView).toHaveBeenCalled());
  });

  it("routes pending question attention to the planning session without rendering the question body", async () => {
    seedColumns([workItem({ id: "wi-plan", status: "todo", sessionId: null })]);
    (
      activePlanningRunByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        [
          "wi-plan",
          {
            id: "run-plan",
            workItemId: "wi-plan",
            kind: "planning",
            sessionId: "plan-sess-1",
            provider: "claude",
            profileId: "claude",
            status: "blocked",
            worktreePath: "/repo",
            branch: "main",
            cost: null,
            createdAt: 1,
            startedAt: 1,
            endedAt: null,
            updatedAt: 1,
          },
        ],
      ]),
    );
    (
      pendingDecisionByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        [
          "wi-plan",
          {
            id: "decision-1",
            runId: "run-plan",
            question: "What should the report include?",
            options: [{ value: "github", label: "GitHub activity" }],
            defaultValue: null,
            timeoutAt: null,
            status: "pending",
            resolvedValue: null,
            resolvedBy: null,
            createdAt: 1,
            resolvedAt: null,
            updatedAt: 1,
          },
        ],
      ]),
    );
    render(BoardMainView);

    expect(screen.queryByText("What should the report include?")).toBeNull();
    await fireEvent.click(
      screen.getByLabelText("Open session with pending question"),
    );

    expect(openSessionById).toHaveBeenCalledWith("plan-sess-1");
    await vi.waitFor(() => expect(closeMainView).toHaveBeenCalled());
  });

  it("replans an active planning run from the card actions menu", async () => {
    seedColumns([workItem({ id: "wi-plan", status: "todo", sessionId: null })]);
    (
      activePlanningRunByItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        [
          "wi-plan",
          {
            id: "run-plan",
            workItemId: "wi-plan",
            kind: "planning",
            sessionId: "plan-sess-1",
            provider: "claude",
            profileId: "claude",
            status: "running",
            worktreePath: "/repo",
            branch: "main",
            cost: null,
            createdAt: 1,
            startedAt: 1,
            endedAt: null,
            updatedAt: 1,
          },
        ],
      ]),
    );
    render(BoardMainView);

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"));
    await fireEvent.click(screen.getByText("Retry planning"));

    expect(planWorkItem).toHaveBeenCalledWith("wi-plan", {
      replaceActive: true,
    });
  });

  it("opens the new card editor for the selected column", async () => {
    render(BoardMainView);

    // The Review column's add button (4 columns → 4 add buttons).
    const reviewColumn = document.querySelector('[data-column="review"]')!;
    const addButton = reviewColumn.querySelector(
      '[aria-label="Add card"]',
    ) as HTMLButtonElement;
    await fireEvent.click(addButton);

    expect(openNewWorkItemEditor).toHaveBeenCalledWith({ status: "review" });
  });
});
