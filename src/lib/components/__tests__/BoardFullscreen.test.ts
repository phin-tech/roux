import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardFullscreen from "../BoardFullscreen.svelte";
import {
  itemsByColumn,
  moveWorkItem,
  startWorkItem,
  createWorkItem,
} from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import { closeBoardFullscreen, openWorkItemSessionStart } from "$lib/stores/ui";
import { openSessionById } from "$lib/panes/openSession";
import { WORK_ITEM_DRAG_MIME } from "$lib/board/drag";
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

// itemsByColumn is replaced with a plain writable so the test can drive the
// columns directly; moveWorkItem / closeBoardFullscreen become spies.
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
    moveWorkItem: vi.fn().mockResolvedValue({}),
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
  closeBoardFullscreen: vi.fn(),
  openWorkItemEditor: vi.fn(),
  openWorkItemSessionStart: vi.fn(),
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
  for (const col of ["todo", "ready", "doing", "review", "done"]) map.set(col, []);
  for (const item of items) map.get(item.status)?.push(item);
  (itemsByColumn as ReturnType<typeof import("svelte/store").writable>).set(map);
}

function dragData(payload: { itemId: string; fromStatus: string }): DataTransfer {
  return {
    getData: (type: string) =>
      type === WORK_ITEM_DRAG_MIME ? JSON.stringify(payload) : "",
    types: [WORK_ITEM_DRAG_MIME],
    dropEffect: "none",
  } as unknown as DataTransfer;
}

describe("BoardFullscreen", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seedColumns([]);
  });

  it("renders one section per column with labels", () => {
    render(BoardFullscreen);
    expect(screen.getAllByTestId("board-column")).toHaveLength(5);
    expect(screen.getByText("To Do")).toBeTruthy();
    expect(screen.getByText("Ready")).toBeTruthy();
    expect(screen.getByText("In Progress")).toBeTruthy();
    expect(screen.getByText("Review")).toBeTruthy();
    expect(screen.getByText("Done")).toBeTruthy();
  });

  it("moves a card to the dropped-on column", async () => {
    seedColumns([workItem({ id: "wi-1", status: "todo" })]);
    render(BoardFullscreen);

    const doneColumn = document.querySelector('[data-column="done"]')!;
    await fireEvent.drop(doneColumn, {
      dataTransfer: dragData({ itemId: "wi-1", fromStatus: "todo" }),
    });

    expect(moveWorkItem).toHaveBeenCalledWith("wi-1", "done", expect.any(Number));
  });

  it("ignores a drop onto the card's current column", async () => {
    seedColumns([workItem({ id: "wi-1", status: "todo" })]);
    render(BoardFullscreen);

    const todoColumn = document.querySelector('[data-column="todo"]')!;
    await fireEvent.drop(todoColumn, {
      dataTransfer: dragData({ itemId: "wi-1", fromStatus: "todo" }),
    });

    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("closes via the header button", async () => {
    render(BoardFullscreen);
    await fireEvent.click(screen.getByLabelText("Close board"));
    expect(closeBoardFullscreen).toHaveBeenCalled();
  });

  it("Start delegates to daemon start without issuing a second move", async () => {
    seedColumns([
      workItem({ id: "wi-1", status: "todo", projectId: "proj-1", sessionId: null }),
    ].map((item) => ({ ...item, agentProfile: "claude" } as WorkItem)));
    render(BoardFullscreen);

    await fireEvent.click(screen.getByLabelText("Start work item"));

    expect(startWorkItem).toHaveBeenCalledWith("wi-1");
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("shows an inline error when Start dispatch fails", async () => {
    vi.mocked(startWorkItem).mockRejectedValueOnce(
      new Error("project not found"),
    );
    seedColumns([
      workItem({ id: "wi-1", status: "todo", projectId: "proj-1", sessionId: null }),
    ].map((item) => ({ ...item, agentProfile: "claude" } as WorkItem)));
    render(BoardFullscreen);

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
    const item = workItem({ id: "wi-1", title: "Wire task start", projectId: null, sessionId: null });
    seedColumns([item]);
    render(BoardFullscreen);

    expect(screen.getByText("Configure")).toBeTruthy();
    await fireEvent.click(screen.getByLabelText("Start work item"));

    expect(openWorkItemSessionStart).toHaveBeenCalledWith({
      itemId: "wi-1",
      title: "Wire task start",
    });
    expect(startWorkItem).not.toHaveBeenCalled();
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("does not delete from the right-click menu when the delete dialog is canceled", async () => {
    seedColumns([workItem({ id: "wi-delete", title: "Keep me" })]);
    render(BoardFullscreen);

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"), {
      clientX: 64,
      clientY: 96,
    });
    await fireEvent.click(screen.getByRole("menuitem", { name: "Delete card" }));
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(deleteWorkItemWithMode).not.toHaveBeenCalled();
  });

  it("shows Open terminal for a dispatched card and opens + closes the board", async () => {
    seedColumns([
      workItem({ id: "wi-1", status: "doing", sessionId: "sess-1" }),
    ]);
    render(BoardFullscreen);

    // Session-bound cards show Open terminal, not Start.
    expect(screen.queryByLabelText("Start work item")).toBeNull();
    await fireEvent.click(screen.getByLabelText("Open terminal"));

    expect(openSessionById).toHaveBeenCalledWith("sess-1");
    await vi.waitFor(() => expect(closeBoardFullscreen).toHaveBeenCalled());
  });

  it("quick-adds a card to the column it was typed in", async () => {
    render(BoardFullscreen);

    // The Review column's add button (4 columns → 4 add buttons).
    const reviewColumn = document.querySelector('[data-column="review"]')!;
    const addButton = reviewColumn.querySelector(
      '[aria-label="Add card"]',
    ) as HTMLButtonElement;
    await fireEvent.click(addButton);

    const input = reviewColumn.querySelector(
      '[aria-label="New card title"]',
    ) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "Review the PR" } });
    await fireEvent.keyDown(input, { key: "Enter" });

    expect(createWorkItem).toHaveBeenCalledWith({
      title: "Review the PR",
      status: "review",
      sortOrder: expect.any(Number),
    });
  });
});
