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
import {
  runsByItem,
  stopWorkItemRun,
  workItems,
  updateWorkItem,
} from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import { projects } from "$lib/stores/projects";
import type { WorkItem } from "$lib/bindings";
import type { WorkItemRun } from "$lib/types/workItems";

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
    pendingDecisionByItem: writable(new Map()),
    runsByItem: writable(new Map()),
    updateWorkItem: vi.fn().mockResolvedValue({}),
    resolveWorkItemDecision: vi.fn().mockResolvedValue({}),
    stopWorkItemRun: vi.fn().mockResolvedValue({}),
    WORK_ITEM_COLUMNS: ["todo", "ready", "doing", "review", "done"],
    COLUMN_LABELS: {
      todo: "To Do",
      ready: "Ready",
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

function workItemRun(overrides: Partial<WorkItemRun> = {}): WorkItemRun {
  return {
    id: "run-1",
    workItemId: "wi-1",
    sessionId: "sess-1",
    provider: "claude",
    profileId: "claude",
    status: "running",
    worktreePath: "/tmp/repo",
    branch: "roux/run-1",
    cost: null,
    createdAt: 1_700_000_000,
    startedAt: 1_700_000_000,
    endedAt: null,
    updatedAt: 1_700_000_000,
    ...overrides,
  };
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
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(new Map());
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

  it("shows daemon run history for the card", async () => {
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map([["wi-1", [workItemRun({ status: "blocked" })]]]),
    );

    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    await screen.findByText("Run History");
    expect(screen.getByText("blocked")).toBeTruthy();
    expect(screen.getByText("roux/run-1")).toBeTruthy();
  });

  it("stops an active run from run history", async () => {
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map([["wi-1", [workItemRun({ id: "run-1", status: "running" })]]]),
    );

    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    await fireEvent.click(await screen.findByRole("button", { name: "Stop run run-1" }));

    expect(stopWorkItemRun).toHaveBeenCalledWith("run-1");
  });
});
