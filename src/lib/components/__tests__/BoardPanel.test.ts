import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardPanel from "../BoardPanel.svelte";
import {
  itemsByColumn,
  acceptWorkItemReview,
  moveWorkItem,
  planWorkItem,
  startWorkItem,
  activePlanningRunByItem,
  runsByItem,
} from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import { openWorkItemSessionStart } from "$lib/stores/ui";
import { openMainView } from "$lib/stores/mainView";
import type { WorkItem } from "$lib/bindings";

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
      ready: "Ready",
      doing: "In Progress",
      review: "Review",
      done: "Done",
    },
    itemsByColumn: writable(new Map()),
    pendingDecisionByItem: writable(new Map()),
    activePlanningRunByItem: writable(new Map()),
    runsByItem: writable(new Map()),
    acceptWorkItemReview: vi.fn().mockResolvedValue({}),
    moveWorkItem: vi.fn().mockResolvedValue({}),
    planWorkItem: vi.fn().mockResolvedValue("plan-sess-1"),
    startWorkItem: vi.fn().mockResolvedValue("sess-1"),
    createWorkItem: vi.fn().mockResolvedValue({}),
  };
});

vi.mock("$lib/workItems/deleteFlow", () => ({
  deleteWorkItemWithMode: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/stores/sessions", async () => {
  const { writable } = await import("svelte/store");
  return { sessionList: writable([]) };
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

function seedColumns(items: WorkItem[]) {
  const map = new Map<string, WorkItem[]>();
  for (const col of ["todo", "ready", "doing", "review", "done"])
    map.set(col, []);
  for (const item of items) map.get(item.status)?.push(item);
  (itemsByColumn as ReturnType<typeof import("svelte/store").writable>).set(
    map,
  );
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
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map(),
    );
  });

  it("Start delegates to daemon start without issuing a second move", async () => {
    seedColumns(
      [
        workItem({
          id: "wi-1",
          status: "todo",
          projectId: "proj-1",
          sessionId: null,
        }),
      ].map((item) => ({ ...item, agentProfile: "claude" }) as WorkItem),
    );
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByLabelText("Start work item"));

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
          status: "todo",
          projectId: "proj-1",
          sessionId: null,
        }),
      ].map((item) => ({ ...item, agentProfile: "claude" }) as WorkItem),
    );
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByLabelText("Start work item"));

    expect(startWorkItem).toHaveBeenCalledWith("wi-1");
    await vi.waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain(
        "The assigned project no longer exists.",
      ),
    );
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("opens the session prompt when Start is clicked on an unprojected card", async () => {
    const item = workItem({
      id: "wi-1",
      title: "Wire task start",
      projectId: null,
      sessionId: null,
    });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    expect(screen.getByText("Configure")).toBeTruthy();
    await fireEvent.click(screen.getByLabelText("Start work item"));

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
    await fireEvent.click(screen.getByText("Plan"));

    expect(planWorkItem).toHaveBeenCalledWith("wi-1");
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("accepts review from the card actions menu without directly moving the card", async () => {
    const item = workItem({
      id: "wi-review",
      title: "Review me",
      status: "review",
      sessionId: "sess-1",
    });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"));
    await fireEvent.click(screen.getByText("Accept done"));

    expect(acceptWorkItemReview).toHaveBeenCalledWith("wi-review");
    expect(moveWorkItem).not.toHaveBeenCalledWith(
      "wi-review",
      "done",
      expect.any(Number),
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
