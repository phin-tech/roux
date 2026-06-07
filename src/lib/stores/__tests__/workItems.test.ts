import { describe, it, expect, beforeEach, vi } from "vitest";
import { get } from "svelte/store";
import {
  workItems,
  workItemRuns,
  workItemRunEvents,
  workItemDecisions,
  workItemAttachments,
  attachmentsByWorkItem,
  itemsByColumn,
  latestRunByItem,
  pendingDecisionByItem,
  runsByItem,
  hydrateWorkItems,
  applyWorkItemEvent,
  moveWorkItem,
  acceptWorkItemReview,
  requestWorkItemChanges,
  attachDocument,
  listDocuments,
  planWorkItem,
  startWorkItem,
  stopWorkItemRun,
  WORK_ITEM_COLUMNS,
  archivedWorkItems,
  archiveWorkItem,
  restoreWorkItem,
} from "../workItems";
import {
  documentAttach as tauriDocumentAttach,
  documentList as tauriDocumentList,
  workItemMove as tauriWorkItemMove,
  workItemPlan as tauriWorkItemPlan,
  workItemReviewAccept as tauriWorkItemReviewAccept,
  workItemReviewRequestChanges as tauriWorkItemReviewRequestChanges,
  workItemStart as tauriWorkItemStart,
  workItemArchive as tauriWorkItemArchive,
  workItemRestore as tauriWorkItemRestore,
  workItemRunsList as tauriWorkItemRunsList,
  workItemRunStop as tauriWorkItemRunStop,
  workItemRunEvents as tauriWorkItemRunEvents,
  workItemList as tauriWorkItemList,
} from "$lib/tauri";
import type { WorkItem } from "$lib/bindings";
import type {
  Attachment,
  WorkItemDecision,
  WorkItemRun,
  WorkItemRunEvent,
} from "$lib/types/workItems";

vi.mock("$lib/tauri", () => ({
  workItemList: vi.fn(),
  workItemCreate: vi.fn(),
  workItemUpdate: vi.fn(),
  workItemMove: vi.fn(),
  workItemDelete: vi.fn(),
  workItemArchive: vi.fn(),
  workItemRestore: vi.fn(),
  workItemPlan: vi.fn(),
  workItemReviewAccept: vi.fn(),
  workItemReviewRequestChanges: vi.fn(),
  workItemStart: vi.fn(),
  workItemRunStop: vi.fn(),
  documentAttach: vi.fn(),
  documentList: vi.fn().mockResolvedValue([]),
  documentGet: vi.fn(),
  workItemRunsList: vi.fn().mockResolvedValue([]),
  workItemRunEvents: vi.fn().mockResolvedValue([]),
  workItemDecisionsList: vi.fn().mockResolvedValue([]),
  workItemDecisionResolve: vi.fn(),
}));

function makeItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: crypto.randomUUID(),
    projectId: null,
    parentId: null,
    branch: null,
    fetchFirst: null,
    title: "Test item",
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
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

function makeRun(overrides: Partial<WorkItemRun> = {}): WorkItemRun {
  return {
    id: "run-1",
    workItemId: "wi-1",
    kind: "implementation",
    sessionId: "sess-1",
    ptyId: "sess-1",
    provider: "claude",
    profileId: "claude",
    status: "running",
    worktreePath: null,
    branch: null,
    cost: null,
    createdAt: 1,
    startedAt: 1,
    endedAt: null,
    updatedAt: 1,
    ...overrides,
  };
}

function makeAttachment(overrides: Partial<Attachment> = {}): Attachment {
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

function makeDecision(
  overrides: Partial<WorkItemDecision> = {},
): WorkItemDecision {
  return {
    id: "dec-1",
    runId: "run-1",
    question: "Choose path?",
    options: [{ value: "go", label: "Go" }],
    defaultValue: "go",
    timeoutAt: null,
    status: "pending",
    resolvedValue: null,
    resolvedBy: null,
    createdAt: 2,
    resolvedAt: null,
    updatedAt: 2,
    ...overrides,
  };
}

function makeRunEvent(
  overrides: Partial<WorkItemRunEvent> = {},
): WorkItemRunEvent {
  return {
    id: "event-1",
    runId: "run-1",
    kind: "result",
    payload: { summary: "Implemented review package." },
    createdAt: 2,
    ...overrides,
  };
}

describe("workItems store", () => {
  beforeEach(() => {
    workItems.set([]);
    workItemRuns.set([]);
    workItemRunEvents.set([]);
    workItemDecisions.set([]);
    workItemAttachments.set([]);
    vi.clearAllMocks();
    vi.mocked(tauriDocumentList).mockResolvedValue([]);
    vi.mocked(tauriWorkItemRunEvents).mockResolvedValue([]);
  });

  describe("applyWorkItemEvent - created", () => {
    it("adds a new item", () => {
      const item = makeItem({ title: "new" });
      applyWorkItemEvent({ type: "created", item });
      expect(get(workItems)).toHaveLength(1);
      expect(get(workItems)[0].title).toBe("new");
    });

    it("dedupes on id", () => {
      const item = makeItem();
      applyWorkItemEvent({ type: "created", item });
      applyWorkItemEvent({ type: "created", item });
      expect(get(workItems)).toHaveLength(1);
    });
  });

  describe("applyWorkItemEvent - updated", () => {
    it("replaces by id", () => {
      const item = makeItem({ title: "original" });
      workItems.set([item]);
      const updated = { ...item, title: "updated" };
      applyWorkItemEvent({ type: "updated", item: updated });
      expect(get(workItems)[0].title).toBe("updated");
    });
  });

  describe("applyWorkItemEvent - moved", () => {
    it("updates status and sortOrder", () => {
      const item = makeItem({ status: "todo", sortOrder: 0 });
      workItems.set([item]);
      applyWorkItemEvent({
        type: "moved",
        id: item.id,
        status: "doing",
        sortOrder: 1,
      });
      const result = get(workItems)[0];
      expect(result.status).toBe("doing");
      expect(result.sortOrder).toBe(1);
    });
  });

  describe("moveWorkItem", () => {
    it("upserts the returned item so review stage metadata stays fresh", async () => {
      const item = makeItem({
        id: "wi-1",
        status: "todo",
        sortOrder: 0,
        reviewStageId: null,
      });
      const moved = {
        ...item,
        status: "review" as const,
        sortOrder: 1,
        reviewStageId: "local_review",
      };
      workItems.set([item]);
      vi.mocked(tauriWorkItemMove).mockResolvedValue(moved);

      await expect(moveWorkItem("wi-1", "review", 1)).resolves.toEqual(moved);

      expect(tauriWorkItemMove).toHaveBeenCalledWith("wi-1", "review", 1);
      expect(get(workItems)[0]).toEqual(moved);
    });
  });

  describe("applyWorkItemEvent - deleted", () => {
    it("removes by id", () => {
      const a = makeItem();
      const b = makeItem();
      workItems.set([a, b]);
      applyWorkItemEvent({ type: "deleted", id: a.id });
      const list = get(workItems);
      expect(list).toHaveLength(1);
      expect(list[0].id).toBe(b.id);
    });
  });

  describe("archive state", () => {
    it("hydrates active and archived cards then hides archived cards from columns", async () => {
      const active = makeItem({
        id: "active",
        status: "todo",
        archivedAt: null,
      });
      const archived = makeItem({
        id: "archived",
        status: "todo",
        archivedAt: 10,
      });
      vi.mocked(tauriWorkItemList).mockResolvedValueOnce([active, archived]);

      await hydrateWorkItems();

      expect(tauriWorkItemList).toHaveBeenCalledWith(null, true);
      expect(
        get(itemsByColumn)
          .get("todo")
          ?.map((item) => item.id),
      ).toEqual(["active"]);
      expect(get(archivedWorkItems).map((item) => item.id)).toEqual([
        "archived",
      ]);
    });

    it("archives and restores cards in local state", async () => {
      const active = makeItem({ id: "wi-1", archivedAt: null });
      const archived = { ...active, archivedAt: 10 };
      workItems.set([active]);
      vi.mocked(tauriWorkItemArchive).mockResolvedValueOnce(archived);
      vi.mocked(tauriWorkItemRestore).mockResolvedValueOnce(active);

      await archiveWorkItem("wi-1");
      expect(get(archivedWorkItems).map((item) => item.id)).toEqual(["wi-1"]);

      await restoreWorkItem("wi-1");
      expect(get(archivedWorkItems)).toEqual([]);
      expect(get(workItems)[0].archivedAt).toBeNull();
    });

    it("applies archived and restored work item events", () => {
      const active = makeItem({ id: "wi-1", archivedAt: null });
      const archived = { ...active, archivedAt: 10 };
      workItems.set([active]);

      applyWorkItemEvent({ type: "archived", item: archived });
      expect(get(workItems)[0].archivedAt).toBe(10);

      applyWorkItemEvent({ type: "restored", item: active });
      expect(get(workItems)[0].archivedAt).toBeNull();
    });
  });

  describe("applyWorkItemEvent - sessionBound", () => {
    it("binds sessionId to item", () => {
      const item = makeItem({ sessionId: null });
      workItems.set([item]);
      applyWorkItemEvent({
        type: "sessionBound",
        id: item.id,
        sessionId: "sess-1",
      });
      expect(get(workItems)[0].sessionId).toBe("sess-1");
    });
  });

  describe("applyWorkItemEvent - run and decision events", () => {
    it("stores created runs and binds their session to the card", () => {
      const item = makeItem({ id: "wi-1", sessionId: null });
      workItems.set([item]);

      applyWorkItemEvent({ type: "runCreated", run: makeRun() });

      expect(get(workItemRuns)).toHaveLength(1);
      expect(get(workItems)[0].sessionId).toBe("sess-1");
    });

    it("does not bind planning run sessions to the card", () => {
      const item = makeItem({ id: "wi-1", sessionId: null });
      workItems.set([item]);

      applyWorkItemEvent({
        type: "runCreated",
        run: makeRun({ kind: "planning", sessionId: "plan-sess-1" }),
      });

      expect(get(workItemRuns)).toHaveLength(1);
      expect(get(workItems)[0].sessionId).toBeNull();
    });

    it("treats the last stored run as latest even when timestamps match", () => {
      workItemRuns.set([
        makeRun({ id: "run-2", sessionId: "sess-2", createdAt: 1 }),
        makeRun({ id: "run-1", sessionId: "sess-1", createdAt: 1 }),
      ]);

      expect(get(latestRunByItem).get("wi-1")?.id).toBe("run-1");
      expect(
        get(runsByItem)
          .get("wi-1")
          ?.map((run) => run.id),
      ).toEqual(["run-1", "run-2"]);
    });

    it("updates a run from daemon runUpdated events", () => {
      workItemRuns.set([makeRun({ id: "run-1", status: "running" })]);

      applyWorkItemEvent({
        type: "runUpdated",
        run: makeRun({ id: "run-1", status: "stopped", endedAt: 3 }),
      });

      expect(get(workItemRuns)[0].status).toBe("stopped");
      expect(get(workItemRuns)[0].endedAt).toBe(3);
    });

    it("stores appended run events", () => {
      applyWorkItemEvent({
        type: "runEventAppended",
        event: {
          id: "event-1",
          runId: "run-1",
          kind: "statusChanged",
          payload: { status: "stopped" },
          createdAt: 3,
        },
      });

      expect(get(workItemRunEvents)).toHaveLength(1);
      expect(get(workItemRunEvents)[0].kind).toBe("statusChanged");
    });

    it("marks a card blocked when its latest run has a pending decision", () => {
      workItemRuns.set([makeRun({ id: "run-1", workItemId: "wi-1" })]);

      applyWorkItemEvent({ type: "decisionCreated", decision: makeDecision() });

      expect(get(workItemRuns)[0].status).toBe("blocked");
      expect(get(pendingDecisionByItem).get("wi-1")?.id).toBe("dec-1");
    });

    it("removes a resolved decision from pending card state", () => {
      workItemRuns.set([
        makeRun({ id: "run-1", workItemId: "wi-1", status: "blocked" }),
      ]);
      workItemDecisions.set([makeDecision()]);

      applyWorkItemEvent({
        type: "decisionResolved",
        decision: makeDecision({
          status: "resolved",
          resolvedValue: "go",
          resolvedAt: 3,
        }),
      });

      expect(get(workItemRuns)[0].status).toBe("running");
      expect(get(pendingDecisionByItem).has("wi-1")).toBe(false);
    });

    it("removes a timed out decision from pending card state", () => {
      workItemRuns.set([
        makeRun({ id: "run-1", workItemId: "wi-1", status: "blocked" }),
      ]);
      workItemDecisions.set([makeDecision({ timeoutAt: 3 })]);

      applyWorkItemEvent({
        type: "decisionTimedOut",
        decision: makeDecision({
          status: "timedOut",
          resolvedValue: "go",
          resolvedBy: "timeout",
          resolvedAt: 3,
          timeoutAt: 3,
        }),
      });

      expect(get(workItemRuns)[0].status).toBe("running");
      expect(get(pendingDecisionByItem).has("wi-1")).toBe(false);
    });

    it("keeps a run blocked while another decision on the run is pending", () => {
      workItemRuns.set([
        makeRun({ id: "run-1", workItemId: "wi-1", status: "blocked" }),
      ]);
      workItemDecisions.set([
        makeDecision({ id: "dec-1" }),
        makeDecision({ id: "dec-2", question: "Choose again?" }),
      ]);

      applyWorkItemEvent({
        type: "decisionResolved",
        decision: makeDecision({
          id: "dec-1",
          status: "resolved",
          resolvedValue: "go",
          resolvedAt: 3,
        }),
      });

      expect(get(workItemRuns)[0].status).toBe("blocked");
      expect(get(pendingDecisionByItem).get("wi-1")?.id).toBe("dec-2");
    });

    it("does not revive a terminal run when its decision resolves", () => {
      workItemRuns.set([
        makeRun({ id: "run-1", workItemId: "wi-1", status: "done" }),
      ]);
      workItemDecisions.set([makeDecision()]);

      applyWorkItemEvent({
        type: "decisionResolved",
        decision: makeDecision({
          status: "resolved",
          resolvedValue: "go",
          resolvedAt: 3,
        }),
      });

      expect(get(workItemRuns)[0].status).toBe("done");
      expect(get(pendingDecisionByItem).has("wi-1")).toBe(false);
    });

    it("does not revive a review run when its decision resolves", () => {
      workItemRuns.set([
        makeRun({ id: "run-1", workItemId: "wi-1", status: "review" }),
      ]);
      workItemDecisions.set([makeDecision()]);

      applyWorkItemEvent({
        type: "decisionResolved",
        decision: makeDecision({
          status: "resolved",
          resolvedValue: "go",
          resolvedAt: 3,
        }),
      });

      expect(get(workItemRuns)[0].status).toBe("review");
      expect(get(pendingDecisionByItem).has("wi-1")).toBe(false);
    });
  });

  describe("attachments", () => {
    it("loads work item attachments during hydration", async () => {
      const item = makeItem({ id: "wi-1" });
      const attachment = makeAttachment({ targetId: "wi-1" });
      vi.mocked(tauriWorkItemList).mockResolvedValueOnce([item]);
      vi.mocked(tauriDocumentList).mockResolvedValueOnce([attachment]);

      await hydrateWorkItems();

      expect(tauriDocumentList).toHaveBeenCalledWith("workItem", null);
      expect(get(workItemAttachments)).toEqual([attachment]);
      expect(get(attachmentsByWorkItem).get("wi-1")).toEqual([attachment]);
    });

    it("loads run events for review runs during hydration", async () => {
      const item = makeItem({ id: "wi-1", status: "review" });
      const reviewRun = makeRun({
        id: "run-review",
        workItemId: "wi-1",
        status: "review",
      });
      const doingRun = makeRun({
        id: "run-doing",
        workItemId: "wi-1",
        status: "running",
      });
      const event = makeRunEvent({ id: "event-review", runId: "run-review" });
      vi.mocked(tauriWorkItemList).mockResolvedValueOnce([item]);
      vi.mocked(tauriWorkItemRunsList).mockResolvedValueOnce([
        reviewRun,
        doingRun,
      ]);
      vi.mocked(tauriWorkItemRunEvents).mockResolvedValueOnce([event]);

      await hydrateWorkItems();

      expect(tauriWorkItemRunEvents).toHaveBeenCalledTimes(1);
      expect(tauriWorkItemRunEvents).toHaveBeenCalledWith("run-review");
      expect(get(workItemRunEvents)).toEqual([event]);
    });

    it("keeps hydrated work items when one review run event fetch fails", async () => {
      const item = makeItem({ id: "wi-1", status: "review" });
      const firstReviewRun = makeRun({
        id: "run-review-1",
        workItemId: "wi-1",
        status: "review",
      });
      const secondReviewRun = makeRun({
        id: "run-review-2",
        workItemId: "wi-1",
        status: "review",
      });
      const event = makeRunEvent({ id: "event-review", runId: "run-review-1" });
      vi.mocked(tauriWorkItemList).mockResolvedValueOnce([item]);
      vi.mocked(tauriWorkItemRunsList).mockResolvedValueOnce([
        firstReviewRun,
        secondReviewRun,
      ]);
      vi.mocked(tauriWorkItemRunEvents)
        .mockResolvedValueOnce([event])
        .mockRejectedValueOnce(new Error("run events unavailable"));

      await expect(hydrateWorkItems()).resolves.toBeUndefined();

      expect(get(workItems)).toEqual([item]);
      expect(get(workItemRuns)).toEqual([firstReviewRun, secondReviewRun]);
      expect(get(workItemRunEvents)).toEqual([event]);
    });

    it("upserts attached documents from daemon events", () => {
      const first = makeAttachment({ id: "att-1", title: "Old Plan" });
      const next = makeAttachment({ id: "att-1", title: "Plan" });
      workItemAttachments.set([first]);

      applyWorkItemEvent({ type: "documentAttached", attachment: next });

      expect(get(workItemAttachments)).toEqual([next]);
      expect(get(attachmentsByWorkItem).get("wi-1")).toEqual([next]);
    });

    it("upserts attachments returned by attach and list commands", async () => {
      const plan = makeAttachment({ id: "att-plan", title: "Plan" });
      vi.mocked(tauriDocumentAttach).mockResolvedValueOnce(plan);
      vi.mocked(tauriDocumentList).mockResolvedValueOnce([plan]);

      await expect(
        attachDocument({
          targetKind: "workItem",
          targetId: "wi-1",
          title: "Plan",
          contentKind: "text",
          content: "Plan body",
          mimeType: "text/markdown",
        }),
      ).resolves.toEqual(plan);
      await expect(listDocuments("workItem", "wi-1")).resolves.toEqual([plan]);

      expect(get(attachmentsByWorkItem).get("wi-1")).toEqual([plan]);
    });

    it("replaces only the requested work item cache when listing by target id without a kind", async () => {
      const oldPlan = makeAttachment({
        id: "att-old",
        targetId: "wi-1",
        title: "Old Plan",
      });
      const otherPlan = makeAttachment({
        id: "att-other",
        targetId: "wi-2",
        title: "Other Plan",
      });
      const newPlan = makeAttachment({
        id: "att-new",
        targetId: "wi-1",
        title: "New Plan",
      });
      workItemAttachments.set([oldPlan, otherPlan]);
      vi.mocked(tauriDocumentList).mockResolvedValueOnce([newPlan]);

      await expect(listDocuments(null, "wi-1")).resolves.toEqual([newPlan]);

      expect(get(workItemAttachments)).toEqual([otherPlan, newPlan]);
    });
  });

  describe("startWorkItem", () => {
    it("binds the returned session id immediately", async () => {
      const item = makeItem({ id: "wi-1", sessionId: null });
      workItems.set([item]);
      vi.mocked(tauriWorkItemStart).mockResolvedValueOnce({
        item: makeItem({ id: "wi-1", sessionId: "sess-1", status: "doing" }),
        run: makeRun(),
        session: {} as never,
      });

      await expect(startWorkItem("wi-1")).resolves.toBe("sess-1");

      expect(tauriWorkItemStart).toHaveBeenCalledWith("wi-1", {});
      expect(get(workItemRuns)).toHaveLength(1);
      expect(get(workItems)[0].sessionId).toBe("sess-1");
      expect(get(workItems)[0].status).toBe("doing");
    });

    it("passes forced starts through to the daemon adapter", async () => {
      const item = makeItem({ id: "wi-1", sessionId: null });
      workItems.set([item]);
      vi.mocked(tauriWorkItemStart).mockResolvedValueOnce({
        item: makeItem({ id: "wi-1", sessionId: "sess-1", status: "doing" }),
        run: makeRun(),
        session: {} as never,
      });

      await expect(startWorkItem("wi-1", { forceStart: true })).resolves.toBe(
        "sess-1",
      );

      expect(tauriWorkItemStart).toHaveBeenCalledWith("wi-1", {
        forceStart: true,
      });
    });

    it("passes fix CI starts through to the daemon adapter", async () => {
      const item = makeItem({ id: "wi-1", sessionId: null });
      workItems.set([item]);
      vi.mocked(tauriWorkItemStart).mockResolvedValueOnce({
        item: makeItem({ id: "wi-1", sessionId: "sess-1", status: "doing" }),
        run: makeRun(),
        session: {} as never,
      });

      await expect(startWorkItem("wi-1", { fixCi: true })).resolves.toBe(
        "sess-1",
      );

      expect(tauriWorkItemStart).toHaveBeenCalledWith("wi-1", {
        fixCi: true,
      });
    });

    it("rejects when the daemon returns a run without a session id", async () => {
      const item = makeItem({ id: "wi-1", sessionId: null, status: "todo" });
      workItems.set([item]);
      vi.mocked(tauriWorkItemStart).mockResolvedValueOnce({
        item,
        run: makeRun({ sessionId: null }),
        session: {} as never,
      });

      await expect(startWorkItem("wi-1")).rejects.toThrow(
        "Work item run run-1 did not include a session id",
      );

      expect(get(workItemRuns)).toHaveLength(1);
      expect(get(workItems)[0].sessionId).toBeNull();
      expect(get(workItems)[0].status).toBe("todo");
    });
  });

  describe("planWorkItem", () => {
    it("stores the returned planning run without moving the card", async () => {
      const item = makeItem({ id: "wi-1", sessionId: null, status: "todo" });
      workItems.set([item]);
      vi.mocked(tauriWorkItemPlan).mockResolvedValueOnce({
        item,
        run: makeRun({ kind: "planning", sessionId: "plan-sess-1" }),
        session: {} as never,
      });

      await expect(planWorkItem("wi-1")).resolves.toBe("plan-sess-1");

      expect(tauriWorkItemPlan).toHaveBeenCalledWith("wi-1", {});
      expect(get(workItemRuns)[0]).toMatchObject({
        kind: "planning",
        sessionId: "plan-sess-1",
      });
      expect(get(workItems)[0].status).toBe("todo");
      expect(get(workItems)[0].sessionId).toBeNull();
    });
  });

  describe("acceptWorkItemReview", () => {
    it("stores the daemon-accepted card and run", async () => {
      const item = makeItem({
        id: "wi-1",
        status: "review",
        sessionId: "sess-1",
      });
      workItems.set([item]);
      workItemRuns.set([makeRun({ id: "run-1", status: "review" })]);
      vi.mocked(tauriWorkItemReviewAccept).mockResolvedValueOnce({
        item: makeItem({ id: "wi-1", status: "done", sessionId: "sess-1" }),
        run: makeRun({ id: "run-1", status: "done", endedAt: 5 }),
      });

      await expect(acceptWorkItemReview("wi-1")).resolves.toMatchObject({
        status: "done",
      });

      expect(tauriWorkItemReviewAccept).toHaveBeenCalledWith("wi-1");
      expect(get(workItems)[0].status).toBe("done");
      expect(get(workItemRuns)[0]).toMatchObject({
        id: "run-1",
        status: "done",
        endedAt: 5,
      });
    });
  });

  describe("requestWorkItemChanges", () => {
    it("stores the returned card, run, and feedback attachment", async () => {
      const item = makeItem({
        id: "wi-1",
        status: "review",
        sessionId: "sess-1",
      });
      workItems.set([item]);
      workItemRuns.set([makeRun({ id: "run-1", status: "review" })]);
      vi.mocked(tauriWorkItemReviewRequestChanges).mockResolvedValueOnce({
        item: makeItem({ id: "wi-1", status: "doing", sessionId: null }),
        run: makeRun({
          id: "run-1",
          status: "changesRequested",
          endedAt: 5,
        }),
        attachment: makeAttachment({
          id: "feedback-1",
          title: "Review feedback",
          documentId: "wi-1.feedback",
        }),
      });

      await expect(
        requestWorkItemChanges("run-1", "Please add coverage."),
      ).resolves.toMatchObject({ status: "doing", sessionId: null });

      expect(tauriWorkItemReviewRequestChanges).toHaveBeenCalledWith(
        "run-1",
        "Please add coverage.",
        null,
      );
      expect(get(workItems)[0]).toMatchObject({
        id: "wi-1",
        status: "doing",
        sessionId: null,
      });
      expect(get(workItemRuns)[0]).toMatchObject({
        id: "run-1",
        status: "changesRequested",
        endedAt: 5,
      });
      expect(get(workItemAttachments)[0]).toMatchObject({
        id: "feedback-1",
        title: "Review feedback",
      });
    });
  });

  describe("stopWorkItemRun", () => {
    it("upserts the stopped run returned by the daemon", async () => {
      workItemRuns.set([makeRun({ id: "run-1", status: "running" })]);
      vi.mocked(tauriWorkItemRunStop).mockResolvedValueOnce(
        makeRun({ id: "run-1", status: "stopped", endedAt: 3 }),
      );

      await expect(stopWorkItemRun("run-1")).resolves.toMatchObject({
        id: "run-1",
        status: "stopped",
      });

      expect(tauriWorkItemRunStop).toHaveBeenCalledWith("run-1");
      expect(get(workItemRuns)[0].status).toBe("stopped");
    });
  });

  describe("itemsByColumn derived store", () => {
    it("groups items by status", () => {
      const a = makeItem({ status: "todo", sortOrder: 0 });
      const b = makeItem({ status: "doing", sortOrder: 0 });
      const c = makeItem({ status: "todo", sortOrder: 1 });
      workItems.set([a, b, c]);
      const cols = get(itemsByColumn);
      expect(cols.get("todo")).toHaveLength(2);
      expect(cols.get("doing")).toHaveLength(1);
      expect(cols.get("ready")).toHaveLength(0);
      expect(cols.get("review")).toHaveLength(0);
      expect(cols.get("done")).toHaveLength(0);
    });

    it("sorts by sortOrder within a column", () => {
      const a = makeItem({ status: "todo", sortOrder: 5 });
      const b = makeItem({ status: "todo", sortOrder: 2 });
      workItems.set([a, b]);
      const todos = get(itemsByColumn).get("todo")!;
      expect(todos[0].sortOrder).toBe(2);
      expect(todos[1].sortOrder).toBe(5);
    });

    it("all defined columns are present even when empty", () => {
      workItems.set([]);
      const cols = get(itemsByColumn);
      for (const col of WORK_ITEM_COLUMNS) {
        expect(cols.has(col)).toBe(true);
      }
    });
  });
});
