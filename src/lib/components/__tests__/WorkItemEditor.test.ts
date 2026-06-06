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
if (typeof Element !== "undefined" && !Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = vi.fn();
}
import { editingWorkItemId, newWorkItemEditor } from "$lib/stores/ui";
import {
  attachmentsByWorkItem,
  runsByItem,
  createWorkItem,
  getDocument,
  listDocuments,
  stopWorkItemRun,
  workItems,
  updateWorkItem,
} from "$lib/stores/workItems";
import { deleteWorkItemWithMode } from "$lib/workItems/deleteFlow";
import { projects } from "$lib/stores/projects";
import type { WorkItem } from "$lib/bindings";
import type {
  Attachment,
  AttachmentDocument,
  WorkItemRun,
} from "$lib/types/workItems";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("$lib/stores/ui", async () => {
  const { writable } = await import("svelte/store");
  const editingWorkItemId = writable<string | null>(null);
  const newWorkItemEditor = writable(null);
  return {
    editingWorkItemId,
    newWorkItemEditor,
    closeWorkItemEditor: vi.fn(() => {
      editingWorkItemId.set(null);
      newWorkItemEditor.set(null);
    }),
  };
});

vi.mock("$lib/stores/workItems", async () => {
  const { writable } = await import("svelte/store");
  return {
    workItems: writable<WorkItem[]>([]),
    pendingDecisionByItem: writable(new Map()),
    runsByItem: writable(new Map()),
    attachmentsByWorkItem: writable(new Map()),
    createWorkItem: vi.fn().mockResolvedValue({}),
    updateWorkItem: vi.fn().mockResolvedValue({}),
    resolveWorkItemDecision: vi.fn().mockResolvedValue({}),
    stopWorkItemRun: vi.fn().mockResolvedValue({}),
    listDocuments: vi.fn().mockResolvedValue([]),
    getDocument: vi.fn(),
    WORK_ITEM_COLUMNS: ["todo", "ready", "doing", "review", "done"],
    COLUMN_LABELS: {
      todo: "To Do",
      ready: "Planning",
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

vi.mock("$lib/stores/settings", async () => {
  const { writable } = await import("svelte/store");
  return {
    settings: writable({
      defaultProjectPath: "/default/repo",
      repoRoots: ["/default/repo", "/other/repo"],
    }),
  };
});

vi.mock("$lib/panes/profiles", async () => {
  const { writable } = await import("svelte/store");
  return {
    profileList: writable([
      {
        id: "claude",
        name: "Claude",
        setupCommand: null,
        startupCommand: null,
        startupBehavior: null,
        env: null,
        cwdOverride: null,
        icon: null,
        provider: "claude",
        source: "builtin",
      },
      {
        id: "codex",
        name: "Codex",
        setupCommand: null,
        startupCommand: null,
        startupBehavior: null,
        env: null,
        cwdOverride: null,
        icon: null,
        provider: "codex",
        source: "builtin",
      },
    ]),
  };
});

vi.mock("$lib/tauri", () => ({
  listWorktrees: vi.fn().mockResolvedValue([]),
}));

function workItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "wi-1",
    projectId: null,
    parentId: null,
    branch: null,
    fetchFirst: null,
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
    reviewStageId: null,
    archivedAt: null,
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

function attachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: "att-plan",
    documentId: "wi-1.plan",
    targetKind: "workItem",
    targetId: "wi-1",
    title: "Implementation Plan",
    contentKind: "text",
    mimeType: "text/markdown",
    sourcePath: null,
    byteLen: 1024,
    sha256: "sha",
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("WorkItemEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    invokeMock.mockResolvedValue(["/workspace/roux", "/workspace/kanbots"]);
    (workItems as ReturnType<typeof import("svelte/store").writable>).set([
      workItem(),
    ]);
    (projects as ReturnType<typeof import("svelte/store").writable>).set([
      {
        id: "proj-1",
        name: "Roux",
        repoRoots: ["/repo"],
        contextPaths: [],
        sessionBlueprints: [],
        projectPrompt: "",
      },
    ]);
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map(),
    );
    (
      attachmentsByWorkItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(new Map());
    vi.mocked(listDocuments).mockResolvedValue([]);
    vi.mocked(getDocument).mockResolvedValue({
      attachment: attachment(),
      content: "Plan body",
    } satisfies AttachmentDocument);
    editingWorkItemId.set(null);
    newWorkItemEditor.set(null);
  });

  it("renders nothing when no card is being edited", () => {
    render(WorkItemEditor);
    expect(screen.queryByText("Edit card")).toBeNull();
  });

  it("opens for the editing id and saves the chosen project", async () => {
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    // Modal shows with the card's title prefilled.
    const titleInput = (await screen.findByLabelText(
      "Title",
    )) as HTMLInputElement;
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
      repoPath: null,
      agentProfile: null,
      worktreePath: null,
      branch: null,
      baseBranch: null,
      fetchFirst: null,
    });
  });

  it("allows clearing an assigned project", async () => {
    (workItems as ReturnType<typeof import("svelte/store").writable>).set([
      workItem({ id: "wi-1", projectId: "proj-1" }),
    ]);
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");
    await screen.findByText("Edit card");

    const projectSelect = screen.getByRole("combobox", { name: "Project" });
    await fireEvent.change(projectSelect, { target: { value: "" } });
    await fireEvent.click(screen.getByText("Save"));

    expect(updateWorkItem).toHaveBeenCalledWith(
      "wi-1",
      expect.objectContaining({ projectId: null }),
    );
  });

  it("shows the current review stage as workflow metadata", async () => {
    (workItems as ReturnType<typeof import("svelte/store").writable>).set([
      workItem({
        id: "wi-1",
        status: "review",
        reviewStageId: "pr_review",
      }),
    ]);
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    await screen.findByText("Edit card");

    expect(screen.getByText("Review stage")).toBeTruthy();
    expect(screen.getByText("PR Review")).toBeTruthy();
  });

  it("labels preserved review stage as the next gate during fixes", async () => {
    (workItems as ReturnType<typeof import("svelte/store").writable>).set([
      workItem({
        id: "wi-1",
        status: "doing",
        reviewStageId: "pr_review",
      }),
    ]);
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    await screen.findByText("Edit card");

    expect(screen.getByText("Next review stage")).toBeTruthy();
    expect(screen.queryByText("Review stage")).toBeNull();
    expect(screen.getByText("PR Review")).toBeTruthy();
  });

  it("creates a new card with default repo and workflow default profile", async () => {
    render(WorkItemEditor);
    newWorkItemEditor.set({ status: "review" });

    const titleInput = (await screen.findByLabelText(
      "Title",
    )) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "Review the PR" } });
    await fireEvent.click(screen.getByText("Create"));

    expect(createWorkItem).toHaveBeenCalledWith({
      title: "Review the PR",
      body: "",
      status: "review",
      projectId: null,
      repoPath: "/default/repo",
      agentProfile: null,
      worktreePath: null,
      branch: null,
      baseBranch: null,
      fetchFirst: null,
      sortOrder: expect.any(Number),
    });
  });

  it("persists an explicit card profile override when selected", async () => {
    render(WorkItemEditor);
    newWorkItemEditor.set({ status: "todo" });

    await fireEvent.input(await screen.findByLabelText("Title"), {
      target: { value: "Use Codex" },
    });
    await fireEvent.change(screen.getByLabelText("Spawn profile"), {
      target: { value: "codex" },
    });
    await fireEvent.click(screen.getByText("Create"));

    expect(createWorkItem).toHaveBeenCalledWith(
      expect.objectContaining({ agentProfile: "codex" }),
    );
  });

  it("loads repository picker options from discovered repos under configured roots", async () => {
    render(WorkItemEditor);
    newWorkItemEditor.set({ status: "todo" });

    await fireEvent.input(await screen.findByDisplayValue("/default/repo"), {
      target: { value: "" },
    });

    expect(await screen.findByText("workspace/roux")).toBeTruthy();
    expect(invokeMock).toHaveBeenCalledWith("list_git_repos_in_roots", {
      roots: ["/default/repo", "/other/repo"],
      excludeWorktrees: true,
    });
  });

  it("saves a new branch from origin main with fetch first", async () => {
    render(WorkItemEditor);
    newWorkItemEditor.set({ status: "todo" });

    const titleInput = (await screen.findByLabelText(
      "Title",
    )) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "Use origin main" } });
    await fireEvent.input(
      screen.getByPlaceholderText("main, feat/my-branch, or existing path"),
      {
        target: { value: "feat/origin-card" },
      },
    );
    await fireEvent.change(screen.getByLabelText("Branch from"), {
      target: { value: "originMain" },
    });
    await fireEvent.click(screen.getByText("Create"));

    expect(createWorkItem).toHaveBeenCalledWith(
      expect.objectContaining({
        branch: "feat/origin-card",
        baseBranch: "origin/main",
        fetchFirst: true,
      }),
    );
  });

  it("opens the delete dialog and deletes only the card", async () => {
    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");
    await screen.findByText("Edit card");

    await fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete card" })).toBeTruthy();
    await fireEvent.click(
      screen.getByRole("button", { name: "Delete card only" }),
    );

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

  it("renders as a full-height work view when requested", async () => {
    render(WorkItemEditor, { surface: "main" });
    editingWorkItemId.set("wi-1");

    const detail = await screen.findByRole("region", { name: "Edit card" });
    expect(detail.className).toContain("h-full");
    expect(screen.queryByRole("dialog", { name: "Edit card" })).toBeNull();
  });

  it("stops an active run from run history", async () => {
    (runsByItem as ReturnType<typeof import("svelte/store").writable>).set(
      new Map([["wi-1", [workItemRun({ id: "run-1", status: "running" })]]]),
    );

    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    await fireEvent.click(
      await screen.findByRole("button", { name: "Stop run run-1" }),
    );

    expect(stopWorkItemRun).toHaveBeenCalledWith("run-1");
  });

  it("shows and previews work item attachments in the editor", async () => {
    const plan = attachment();
    (
      attachmentsByWorkItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(new Map([["wi-1", [plan]]]));
    vi.mocked(getDocument).mockResolvedValueOnce({
      attachment: plan,
      content: "Use the approved plan.",
    });

    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");

    await screen.findByText("Attachments");
    await fireEvent.click(
      screen.getByRole("button", { name: "Implementation Plan" }),
    );

    expect(getDocument).toHaveBeenCalledWith("wi-1.plan");
    expect(await screen.findByText("Use the approved plan.")).toBeTruthy();
  });

  it("ignores attachment loads that resolve after switching cards", async () => {
    const staleLoad = deferred<AttachmentDocument>();
    const plan = attachment({
      id: "att-plan-1",
      documentId: "wi-1.plan",
      targetId: "wi-1",
    });
    (workItems as ReturnType<typeof import("svelte/store").writable>).set([
      workItem({ id: "wi-1", title: "First card" }),
      workItem({ id: "wi-2", title: "Second card" }),
    ]);
    (
      attachmentsByWorkItem as ReturnType<
        typeof import("svelte/store").writable
      >
    ).set(
      new Map([
        ["wi-1", [plan]],
        ["wi-2", []],
      ]),
    );
    vi.mocked(getDocument).mockReturnValueOnce(staleLoad.promise);

    render(WorkItemEditor);
    editingWorkItemId.set("wi-1");
    await screen.findByDisplayValue("First card");
    await fireEvent.click(
      screen.getByRole("button", { name: "Implementation Plan" }),
    );

    editingWorkItemId.set("wi-2");
    await screen.findByDisplayValue("Second card");
    staleLoad.resolve({
      attachment: plan,
      content: "Stale plan body",
    });
    await Promise.resolve();

    expect(screen.queryByText("Stale plan body")).toBeNull();
    expect(screen.getByText("Select an attachment to read it.")).toBeTruthy();
  });
});
