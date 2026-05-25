import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import WorkItemEditor from "../WorkItemEditor.svelte";

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
import { editingWorkItemId } from "$lib/stores/ui";
import { workItems, updateWorkItem } from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import { projects } from "$lib/stores/projects";
import type { WorkItem } from "$lib/bindings";

vi.mock("$lib/stores/ui", async () => {
  const { writable } = await import("svelte/store");
  const editingWorkItemId = writable<string | null>(null);
  return {
    editingWorkItemId,
    closeWorkItemEditor: vi.fn(() => editingWorkItemId.set(null)),
  };
});

vi.mock("$lib/stores/workItems", async () => {
  const { writable } = await import("svelte/store");
  return {
    workItems: writable<WorkItem[]>([]),
    updateWorkItem: vi.fn().mockResolvedValue({}),
    WORK_ITEM_COLUMNS: ["todo", "doing", "review", "done"],
    COLUMN_LABELS: {
      todo: "To Do",
      doing: "In Progress",
      review: "Review",
      done: "Done",
    },
  };
});

vi.mock("$lib/workItems/deleteFlow", () => ({
  deleteWorkItemWithMode: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/stores/projects", async () => {
  const { writable } = await import("svelte/store");
  return { projects: writable([]) };
});

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

describe("WorkItemEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (workItems as ReturnType<typeof import("svelte/store").writable>).set([
      workItem(),
    ]);
    (projects as ReturnType<typeof import("svelte/store").writable>).set([
      { id: "proj-1", name: "Roux" },
    ]);
    editingWorkItemId.set(null);
  });

  it("renders nothing when no card is being edited", () => {
    render(WorkItemEditor);
    expect(screen.queryByText("Edit card")).toBeNull();
  });

  it("opens for the editing id and saves the chosen project", async () => {
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    // Modal shows with the card's title prefilled.
    const titleInput = (await screen.findByLabelText("Title")) as HTMLInputElement;
    expect(titleInput.value).toBe("Ship the board");

    // Assign a project, then save.
    const projectSelect = screen.getByRole("combobox", { name: "Project" });
    await fireEvent.change(projectSelect, { target: { value: "proj-1" } });
    await fireEvent.click(screen.getByText("Save"));

    expect(updateWorkItem).toHaveBeenCalledWith("wi-1", {
      title: "Ship the board",
      body: "",
      status: "todo",
      projectId: "proj-1",
    });
  });

  it("offers None only while the card is unassigned (no clearing once set)", async () => {
    (workItems as ReturnType<typeof import("svelte/store").writable>).set([
      workItem({ id: "wi-1", projectId: "proj-1" }),
    ]);
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");
    await screen.findByText("Edit card");

    // Card already has a project → the picker must not offer "None".
    const options = Array.from(
      screen.getByRole("combobox", { name: "Project" }).querySelectorAll("option"),
    ).map((o) => o.textContent);
    expect(options).not.toContain("None");
    expect(options).toContain("Roux");
  });

  it("opens the delete dialog and deletes only the card", async () => {
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");
    await screen.findByText("Edit card");

    await fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete card" })).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Delete card only" }));

    expect(deleteWorkItemWithMode).toHaveBeenCalledWith(
      expect.objectContaining({ id: "wi-1" }),
      "card-only",
    );
  });
});
