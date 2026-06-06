import { writable, derived, get } from "svelte/store";
import type { WorkItem, WorkItemInput, WorkItemStatus } from "$lib/bindings";
import type {
  Attachment,
  AttachmentDocument,
  AttachmentInput,
  AttachmentTargetKind,
  WorkItemDecision,
  WorkItemEvent,
  WorkItemRun,
  WorkItemRunEvent,
} from "$lib/types/workItems";
import {
  workItemList as tauriWorkItemList,
  workItemCreate as tauriWorkItemCreate,
  workItemUpdate as tauriWorkItemUpdate,
  workItemMove as tauriWorkItemMove,
  workItemDelete as tauriWorkItemDelete,
  workItemArchive as tauriWorkItemArchive,
  workItemRestore as tauriWorkItemRestore,
  workItemStart as tauriWorkItemStart,
  workItemPlan as tauriWorkItemPlan,
  workItemReviewAccept as tauriWorkItemReviewAccept,
  workItemReviewRequestChanges as tauriWorkItemReviewRequestChanges,
  workItemRunsList as tauriWorkItemRunsList,
  workItemRunEvents as tauriWorkItemRunEvents,
  workItemRunStop as tauriWorkItemRunStop,
  workItemDecisionsList as tauriWorkItemDecisionsList,
  workItemDecisionResolve as tauriWorkItemDecisionResolve,
  documentAttach as tauriDocumentAttach,
  documentList as tauriDocumentList,
  documentGet as tauriDocumentGet,
} from "$lib/tauri";
import { splitArchivedWorkItems } from "$lib/workItems/archive";

export {
  type WorkItem,
  type Attachment,
  type AttachmentDocument,
  type AttachmentInput,
  type AttachmentTargetKind,
  type WorkItemDecision,
  type WorkItemEvent,
  type WorkItemInput,
  type WorkItemRun,
  type WorkItemRunEvent,
  type WorkItemStatus,
};

export const WORK_ITEM_COLUMNS: WorkItemStatus[] = [
  "todo",
  "ready",
  "doing",
  "review",
  "done",
];

export const COLUMN_LABELS: Record<WorkItemStatus, string> = {
  todo: "To Do",
  ready: "Planning",
  doing: "In Progress",
  review: "Review",
  done: "Done",
};

export const workItems = writable<WorkItem[]>([]);
export const workItemRuns = writable<WorkItemRun[]>([]);
export const workItemRunEvents = writable<WorkItemRunEvent[]>([]);
export const workItemDecisions = writable<WorkItemDecision[]>([]);
export const workItemAttachments = writable<Attachment[]>([]);
const TERMINAL_RUN_STATUSES = new Set<WorkItemRun["status"]>([
  "review",
  "changesRequested",
  "failed",
  "stopped",
  "done",
]);

export const activeWorkItems = derived(workItems, ($items) => {
  return splitArchivedWorkItems($items).active;
});

export const archivedWorkItems = derived(workItems, ($items) => {
  return splitArchivedWorkItems($items).archived;
});

export const itemsByColumn = derived(activeWorkItems, ($items) => {
  const map = new Map<WorkItemStatus, WorkItem[]>();
  for (const col of WORK_ITEM_COLUMNS) map.set(col, []);
  for (const item of $items) {
    const col = item.status;
    const bucket = map.get(col);
    if (bucket) bucket.push(item);
    else map.set(col, [item]);
  }
  for (const bucket of map.values()) {
    bucket.sort(
      (a, b) => a.sortOrder - b.sortOrder || a.createdAt - b.createdAt,
    );
  }
  return map;
});

export const latestRunByItem = derived(workItemRuns, ($runs) => {
  const map = new Map<string, WorkItemRun>();
  for (const run of $runs) {
    map.set(run.workItemId, run);
  }
  return map;
});

export const runsByItem = derived(workItemRuns, ($runs) => {
  const map = new Map<string, WorkItemRun[]>();
  for (const run of $runs) {
    const bucket = map.get(run.workItemId) ?? [];
    bucket.push(run);
    map.set(run.workItemId, bucket);
  }
  for (const runs of map.values()) {
    runs.reverse();
  }
  return map;
});

export const activePlanningRunByItem = derived(workItemRuns, ($runs) => {
  const map = new Map<string, WorkItemRun>();
  for (const run of $runs) {
    if (run.kind !== "planning" || TERMINAL_RUN_STATUSES.has(run.status))
      continue;
    map.set(run.workItemId, run);
  }
  return map;
});

export const attachmentsByWorkItem = derived(
  workItemAttachments,
  ($attachments) => {
    const map = new Map<string, Attachment[]>();
    for (const attachment of $attachments) {
      if (attachment.targetKind !== "workItem") continue;
      const bucket = map.get(attachment.targetId) ?? [];
      bucket.push(attachment);
      map.set(attachment.targetId, bucket);
    }
    return map;
  },
);

export const pendingDecisionByRun = derived(workItemDecisions, ($decisions) => {
  const map = new Map<string, WorkItemDecision>();
  for (const decision of $decisions) {
    if (decision.status !== "pending") continue;
    map.set(decision.runId, decision);
  }
  return map;
});

export const pendingDecisionByItem = derived(
  [latestRunByItem, pendingDecisionByRun],
  ([$latestRunByItem, $pendingDecisionByRun]) => {
    const map = new Map<string, WorkItemDecision>();
    for (const [itemId, run] of $latestRunByItem) {
      const decision = $pendingDecisionByRun.get(run.id);
      if (decision) map.set(itemId, decision);
    }
    return map;
  },
);

function bindSessionToWorkItem(id: string, sessionId: string): void {
  workItems.update((list) =>
    list.map((i) => (i.id === id ? { ...i, sessionId } : i)),
  );
}

function upsertItem(item: WorkItem): void {
  workItems.update((list) =>
    list.some((i) => i.id === item.id)
      ? list.map((i) => (i.id === item.id ? item : i))
      : [...list, item],
  );
}

export async function hydrateWorkItems(): Promise<void> {
  try {
    const [items, runs, decisions, attachments] = await Promise.all([
      tauriWorkItemList(null, true),
      tauriWorkItemRunsList(null),
      tauriWorkItemDecisionsList(null),
      tauriDocumentList("workItem", null),
    ]);
    const reviewRunEvents = (
      await Promise.allSettled(
        runs
          .filter(
            (run) => run.kind === "implementation" && run.status === "review",
          )
          .map((run) => tauriWorkItemRunEvents(run.id)),
      )
    ).flatMap((result) => (result.status === "fulfilled" ? result.value : []));
    workItems.set(items);
    workItemRuns.set(runs);
    workItemRunEvents.set(reviewRunEvents);
    workItemDecisions.set(decisions);
    workItemAttachments.set(attachments);
  } catch (err) {
    console.error("Failed to hydrate work items", err);
    throw err;
  }
}

export function applyWorkItemEvent(event: WorkItemEvent): void {
  switch (event.type) {
    case "created":
      workItems.update((list) => {
        if (list.some((i) => i.id === event.item.id)) return list;
        return [...list, event.item];
      });
      break;
    case "updated":
      workItems.update((list) =>
        list.map((i) => (i.id === event.item.id ? event.item : i)),
      );
      break;
    case "archived":
      upsertItem(event.item);
      break;
    case "restored":
      upsertItem(event.item);
      break;
    case "moved":
      workItems.update((list) =>
        list.map((i) =>
          i.id === event.id
            ? { ...i, status: event.status, sortOrder: event.sortOrder }
            : i,
        ),
      );
      break;
    case "deleted":
      workItems.update((list) => list.filter((i) => i.id !== event.id));
      break;
    case "imported":
      void hydrateWorkItems().catch((err) => {
        console.error("Failed to hydrate imported work items", err);
      });
      break;
    case "documentAttached":
      upsertAttachment(event.attachment);
      break;
    case "sessionBound":
      bindSessionToWorkItem(event.id, event.sessionId);
      break;
    case "runCreated":
      upsertRun(event.run);
      if (event.run.kind === "implementation" && event.run.sessionId) {
        bindSessionToWorkItem(event.run.workItemId, event.run.sessionId);
      }
      break;
    case "runUpdated":
      upsertRun(event.run);
      break;
    case "runEventAppended":
      workItemRunEvents.update((events) => [...events, event.event]);
      break;
    case "decisionCreated":
      upsertDecision(event.decision);
      markRunStatus(event.decision.runId, "blocked");
      break;
    case "decisionResolved":
      upsertDecision(event.decision);
      markRunStatusAfterDecisionResolved(event.decision.runId);
      break;
    case "decisionTimedOut":
      upsertDecision(event.decision);
      markRunStatusAfterDecisionResolved(event.decision.runId);
      break;
  }
}

function upsertRun(run: WorkItemRun): void {
  workItemRuns.update((runs) =>
    runs.some((r) => r.id === run.id)
      ? runs.map((r) => (r.id === run.id ? run : r))
      : [...runs, run],
  );
}

function upsertDecision(decision: WorkItemDecision): void {
  workItemDecisions.update((decisions) =>
    decisions.some((d) => d.id === decision.id)
      ? decisions.map((d) => (d.id === decision.id ? decision : d))
      : [...decisions, decision],
  );
}

function upsertAttachment(attachment: Attachment): void {
  if (attachment.targetKind !== "workItem") return;
  workItemAttachments.update((attachments) =>
    attachments.some((a) => a.id === attachment.id)
      ? attachments.map((a) => (a.id === attachment.id ? attachment : a))
      : [...attachments, attachment],
  );
}

function replaceWorkItemAttachments(
  targetId: string | null,
  attachments: Attachment[],
): void {
  const workItemAttachmentsFromList = attachments.filter(
    (attachment) => attachment.targetKind === "workItem",
  );
  workItemAttachments.update((current) => {
    const kept =
      targetId === null
        ? []
        : current.filter((attachment) => attachment.targetId !== targetId);
    return [...kept, ...workItemAttachmentsFromList];
  });
}

function markRunStatus(runId: string, status: WorkItemRun["status"]): void {
  workItemRuns.update((runs) =>
    runs.map((run) => (run.id === runId ? { ...run, status } : run)),
  );
}

function markRunStatusAfterDecisionResolved(runId: string): void {
  const hasPendingDecision = get(workItemDecisions).some(
    (decision) => decision.runId === runId && decision.status === "pending",
  );
  if (hasPendingDecision) {
    markRunStatus(runId, "blocked");
    return;
  }
  workItemRuns.update((runs) =>
    runs.map((run) =>
      run.id === runId && !TERMINAL_RUN_STATUSES.has(run.status)
        ? { ...run, status: "running" }
        : run,
    ),
  );
}

export async function createWorkItem(input: WorkItemInput): Promise<WorkItem> {
  return tauriWorkItemCreate(input);
}

export async function updateWorkItem(
  id: string,
  input: WorkItemInput,
): Promise<WorkItem> {
  return tauriWorkItemUpdate(id, input);
}

export async function moveWorkItem(
  id: string,
  status: WorkItemStatus,
  sortOrder: number,
): Promise<WorkItem> {
  const item = await tauriWorkItemMove(id, status, sortOrder);
  upsertItem(item);
  return item;
}

export async function deleteWorkItem(id: string): Promise<void> {
  await tauriWorkItemDelete(id);
}

export async function archiveWorkItem(id: string): Promise<WorkItem> {
  const item = await tauriWorkItemArchive(id);
  upsertItem(item);
  return item;
}

export async function restoreWorkItem(id: string): Promise<WorkItem> {
  const item = await tauriWorkItemRestore(id);
  upsertItem(item);
  return item;
}

/**
 * Start a work item: the daemon creates or reuses the worktree, creates a
 * session, writes the task prompt, then returns the updated card/run/session.
 */
export interface WorkItemStartOptions {
  profile?: string | null;
  repoPath?: string | null;
  name?: string | null;
  worktreePath?: string | null;
  branch?: string | null;
  base?: string | null;
  fetchFirst?: boolean | null;
  forceStart?: boolean | null;
}

export async function startWorkItem(
  id: string,
  options: WorkItemStartOptions = {},
): Promise<string> {
  const result = await tauriWorkItemStart(id, options);
  upsertItem(result.item);
  upsertRun(result.run);
  if (result.run.sessionId) {
    bindSessionToWorkItem(id, result.run.sessionId);
    return result.run.sessionId;
  }
  throw new Error(
    `Work item run ${result.run.id} did not include a session id`,
  );
}

export interface WorkItemPlanOptions {
  profile?: string | null;
  repoPath?: string | null;
  name?: string | null;
  worktreePath?: string | null;
  replaceActive?: boolean;
}

export async function planWorkItem(
  id: string,
  options: WorkItemPlanOptions = {},
): Promise<string> {
  const result = await tauriWorkItemPlan(id, options);
  upsertItem(result.item);
  upsertRun(result.run);
  if (result.run.sessionId) return result.run.sessionId;
  throw new Error(
    `Work item planning run ${result.run.id} did not include a session id`,
  );
}

export async function acceptWorkItemReview(id: string): Promise<WorkItem> {
  const result = await tauriWorkItemReviewAccept(id);
  upsertItem(result.item);
  upsertRun(result.run);
  return result.item;
}

export async function requestWorkItemChanges(
  id: string,
  note: string,
  status: "doing" | "ready" | null = null,
): Promise<WorkItem> {
  const result = await tauriWorkItemReviewRequestChanges(id, note, status);
  upsertItem(result.item);
  upsertRun(result.run);
  upsertAttachment(result.attachment);
  return result.item;
}

export async function resolveWorkItemDecision(
  id: string,
  value: string,
): Promise<WorkItemDecision> {
  const decision = await tauriWorkItemDecisionResolve(id, value, "user");
  upsertDecision(decision);
  markRunStatusAfterDecisionResolved(decision.runId);
  return decision;
}

export async function stopWorkItemRun(runId: string): Promise<WorkItemRun> {
  const run = await tauriWorkItemRunStop(runId);
  upsertRun(run);
  return run;
}

export async function attachDocument(
  input: AttachmentInput,
): Promise<Attachment> {
  const attachment = await tauriDocumentAttach(input);
  upsertAttachment(attachment);
  return attachment;
}

export async function listDocuments(
  targetKind: AttachmentTargetKind | null = null,
  targetId: string | null = null,
): Promise<Attachment[]> {
  const attachments = await tauriDocumentList(targetKind, targetId);
  if (targetKind === "workItem" || targetKind === null) {
    replaceWorkItemAttachments(targetId, attachments);
  }
  return attachments;
}

export async function getDocument(id: string): Promise<AttachmentDocument> {
  return tauriDocumentGet(id);
}

export function getWorkItemSnapshot(id: string): WorkItem | undefined {
  return get(workItems).find((i) => i.id === id);
}
