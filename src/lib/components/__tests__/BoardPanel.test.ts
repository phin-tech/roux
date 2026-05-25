import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardPanel from "../BoardPanel.svelte";
import {
  itemsByColumn,
  moveWorkItem,
  dispatchWorkItem,
} from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import { openWorkItemSessionStart } from "$lib/stores/ui";
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
    WORK_ITEM_COLUMNS: ["todo", "doing", "review", "done"],
    COLUMN_LABELS: {
      todo: "To Do",
      doing: "In Progress",
      review: "Review",
      done: "Done",
    },
    itemsByColumn: writable(new Map()),
    moveWorkItem: vi.fn().mockResolvedValue({}),
    dispatchWorkItem: vi.fn().mockResolvedValue("sess-1"),
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
  openBoardFullscreen: vi.fn(),
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
  for (const col of ["todo", "doing", "review", "done"]) map.set(col, []);
  for (const item of items) map.get(item.status)?.push(item);
  (itemsByColumn as ReturnType<typeof import("svelte/store").writable>).set(map);
}

describe("BoardPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seedColumns([]);
  });

  it("shows an inline error when Start dispatch fails", async () => {
    vi.mocked(dispatchWorkItem).mockRejectedValueOnce(
      new Error("project not found"),
    );
    seedColumns([
      workItem({ id: "wi-1", status: "todo", projectId: "proj-1", sessionId: null }),
    ]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByLabelText("Start work item"));

    expect(dispatchWorkItem).toHaveBeenCalledWith("wi-1");
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
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByLabelText("Start work item"));

    expect(openWorkItemSessionStart).toHaveBeenCalledWith({
      itemId: "wi-1",
      title: "Wire task start",
    });
    expect(dispatchWorkItem).not.toHaveBeenCalled();
    expect(moveWorkItem).not.toHaveBeenCalled();
  });

  it("opens a delete dialog from the right-click menu and deletes only the card", async () => {
    const item = workItem({ id: "wi-delete", title: "Delete me" });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"), {
      clientX: 64,
      clientY: 96,
    });
    await fireEvent.click(screen.getByRole("menuitem", { name: "Delete card" }));
    expect(screen.getByRole("dialog", { name: "Delete card" })).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Delete card only" }));

    expect(deleteWorkItemWithMode).toHaveBeenCalledWith(item, "card-only");
  });

  it("can delete a card and stop its linked terminal from the delete dialog", async () => {
    const item = workItem({ id: "wi-delete", title: "Stop me", sessionId: "sess-1" });
    seedColumns([item]);
    render(BoardPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.contextMenu(screen.getByTestId("work-item-card"), {
      clientX: 64,
      clientY: 96,
    });
    await fireEvent.click(screen.getByRole("menuitem", { name: "Delete card" }));
    await fireEvent.click(screen.getByRole("button", { name: "Delete card and stop terminal" }));

    expect(deleteWorkItemWithMode).toHaveBeenCalledWith(item, "card-and-stop-session");
  });
});
