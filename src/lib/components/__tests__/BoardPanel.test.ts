import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardPanel from "../BoardPanel.svelte";
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
  runsByItem,
  workItemRunEvents,
} from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import { openWorkItemEditor, openWorkItemSessionStart } from "$lib/stores/ui";
import { openMainView } from "$lib/stores/mainView";
import type { WorkItem } from "$lib/bindings";
import type { Project } from "$lib/types";
import type { Attachment, WorkItemRun } from "$lib/types/workItems";
import { projects } from "$lib/stores/projects";
import { createSessionShell, openPathInFinder } from "$lib/tauri";
import { addSession, setActiveSession } from "$lib/stores/sessions";
import { openSessionById } from "$lib/panes/openSession";
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
  openMainView: vi.fn(),
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
    branch: "main",
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

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: "proj-1",
    name: "Test project",
    repoRoots: ["/repo"],
    contextPaths: [],
    sessionBlueprints: [],
    projectPrompt: "",
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

describe("BoardPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seedColumns([]);
    (
      activePlanningRunByItem as ReturnType<
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
    projects.set([project()]);
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
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByLabelText("Approve and start work item"));

    expect(startWorkItem).toHaveBeenCalledWith("wi-1");
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("opens the board in the main view from the header", async () => {
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByLabelText("Open board in main view"));

    expect(openMainView).toHaveBeenCalledWith({ kind: "board" });
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
    render(BoardPanel, { visible: true, onclose: vi.fn() });

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
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    expect(screen.queryByLabelText("Approve and start work item")).toBeNull();
    expect(screen.getByLabelText("Open planning terminal")).toBeTruthy();
    expect(startWorkItem).not.toHaveBeenCalled();
  });

  it("force starts a planning card without an attached plan from the menu", async () => {
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
    render(BoardPanel, { visible: true, onclose: vi.fn() });

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
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    expect(screen.getByText("Configure")).toBeTruthy();
    await fireEvent.click(screen.getByLabelText("Configure work item"));

    expect(openWorkItemSessionStart).toHaveBeenCalledWith({
      itemId: "wi-1",
      title: "Wire task start",
    });
    expect(startWorkItem).not.toHaveBeenCalled();
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("starts planning from the card actions menu without moving the card", async () => {
    const item = workItem({ id: "wi-1", title: "Plan me", sessionId: null });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"));
    await fireEvent.click(screen.getByRole("menuitem", { name: "Plan" }));

    expect(planWorkItem).toHaveBeenCalledWith("wi-1");
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("accepts review from the card primary action without directly moving the card", async () => {
    const item = workItem({
      id: "wi-review",
      title: "Review me",
      status: "review",
      sessionId: "sess-1",
    });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByLabelText("Accept work item review"));

    expect(acceptWorkItemReview).toHaveBeenCalledWith("wi-review");
    expect(moveWorkItem).not.toHaveBeenCalledWith(
      "wi-review",
      "done",
      expect.any(Number),
    );
  });

  it("requests changes from a review card with a human note", async () => {
    const item = workItem({
      id: "wi-review",
      title: "Review me",
      status: "review",
      projectId: "proj-1",
      repoPath: null,
      sessionId: null,
    });
    seedColumns([item]);
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
            }),
          ],
        ],
      ]),
    );
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(
      screen.getByRole("button", { name: "Open plan attachments" }),
    );
    expect(openWorkItemEditor).toHaveBeenCalledWith("wi-review");
    const reviewPackage = screen.getByTestId("work-item-review-package");
    expect(screen.queryByText("Open worktree")).toBeNull();
    expect(screen.queryByText("Open terminal")).toBeNull();
    expect(screen.getByText("Open agent")).toBeTruthy();
    expect(screen.getByText("Request changes")).toBeTruthy();
    expect(screen.getByText("Accept done")).toBeTruthy();

    await fireEvent.click(
      within(reviewPackage).getByRole("button", { name: "Open worktree" }),
    );
    expect(openPathInFinder).toHaveBeenCalledWith(
      "/repo/.worktrees/review-card",
    );

    await fireEvent.click(
      within(reviewPackage).getByRole("button", { name: "Open terminal" }),
    );
    expect(openSessionById).toHaveBeenCalledWith("sess-review");

    await fireEvent.click(screen.getByText("Open agent"));
    await vi.waitFor(() =>
      expect(createSessionShell).toHaveBeenCalledWith(
        "/repo",
        "Review me review",
        "/repo/.worktrees/review-card",
        null,
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

    await fireEvent.click(screen.getByText("Request changes"));
    const form = screen.getByTestId("work-item-request-changes-form");
    await fireEvent.input(screen.getByPlaceholderText("Requested changes"), {
      target: { value: "Tighten the tests." },
    });
    await fireEvent.click(
      within(form).getByRole("button", { name: "Request changes" }),
    );

    expect(requestWorkItemChanges).toHaveBeenCalledWith(
      "wi-review",
      "Tighten the tests.",
    );
  });

  it("opens a delete dialog from the right-click menu and deletes only the card", async () => {
    const item = workItem({ id: "wi-delete", title: "Delete me" });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"), {
      clientX: 64,
      clientY: 96,
    });
    await fireEvent.click(
      screen.getByRole("menuitem", { name: "Delete card" }),
    );
    expect(screen.getByRole("dialog", { name: "Delete card" })).toBeTruthy();

    await fireEvent.click(
      screen.getByRole("button", { name: "Delete card only" }),
    );

    expect(deleteWorkItemWithMode).toHaveBeenCalledWith(item, "card-only");
  });

  it("can delete a card and stop its linked terminal from the delete dialog", async () => {
    const item = workItem({
      id: "wi-delete",
      title: "Stop me",
      sessionId: "sess-1",
    });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"), {
      clientX: 64,
      clientY: 96,
    });
    await fireEvent.click(
      screen.getByRole("menuitem", { name: "Delete card" }),
    );
    await fireEvent.click(
      screen.getByRole("button", { name: "Delete card and stop terminal" }),
    );

    expect(deleteWorkItemWithMode).toHaveBeenCalledWith(
      item,
      "card-and-stop-session",
    );
  });
});
