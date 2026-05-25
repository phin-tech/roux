import { writable, derived, get } from "svelte/store";
import type { WorkItem, WorkItemInput, WorkItemStatus } from "$lib/bindings";
import type { WorkItemEvent } from "$lib/types/workItems";
import {
  workItemList as tauriWorkItemList,
  workItemCreate as tauriWorkItemCreate,
  workItemUpdate as tauriWorkItemUpdate,
  workItemMove as tauriWorkItemMove,
  workItemDelete as tauriWorkItemDelete,
  workItemDispatch as tauriWorkItemDispatch,
} from "$lib/tauri";

export { type WorkItem, type WorkItemEvent, type WorkItemInput, type WorkItemStatus };

export const WORK_ITEM_COLUMNS: WorkItemStatus[] = ["todo", "doing", "review", "done"];

export const COLUMN_LABELS: Record<WorkItemStatus, string> = {
  todo: "To Do",
  doing: "In Progress",
  review: "Review",
  done: "Done",
};

export const workItems = writable<WorkItem[]>([]);

export const itemsByColumn = derived(workItems, ($items) => {
  const map = new Map<WorkItemStatus, WorkItem[]>();
  for (const col of WORK_ITEM_COLUMNS) map.set(col, []);
  for (const item of $items) {
    const col = item.status;
    const bucket = map.get(col);
    if (bucket) bucket.push(item);
    else map.set(col, [item]);
  }
  for (const bucket of map.values()) {
    bucket.sort((a, b) => a.sortOrder - b.sortOrder || a.createdAt - b.createdAt);
  }
  return map;
});

function bindSessionToWorkItem(id: string, sessionId: string): void {
  workItems.update((list) =>
    list.map((i) => (i.id === id ? { ...i, sessionId } : i)),
  );
}

export async function hydrateWorkItems(): Promise<void> {
  try {
    const items = await tauriWorkItemList(null);
    workItems.set(items);
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
      void hydrateWorkItems();
      break;
    case "sessionBound":
      bindSessionToWorkItem(event.id, event.sessionId);
      break;
  }
}

export async function createWorkItem(input: WorkItemInput): Promise<WorkItem> {
  return tauriWorkItemCreate(input);
}

export async function updateWorkItem(id: string, input: WorkItemInput): Promise<WorkItem> {
  return tauriWorkItemUpdate(id, input);
}

export async function moveWorkItem(
  id: string,
  status: WorkItemStatus,
  sortOrder: number,
): Promise<WorkItem> {
  return tauriWorkItemMove(id, status, sortOrder);
}

export async function deleteWorkItem(id: string): Promise<void> {
  await tauriWorkItemDelete(id);
}

/**
 * Dispatch a work item: the daemon creates a session named after the item and
 * binds it. The returned session id is applied immediately so the board can
 * switch from Start to Open terminal without waiting for the broadcast event.
 * Throws if no daemon is connected.
 */
export interface WorkItemDispatchOptions {
  profile?: string | null;
  repoPath?: string | null;
  name?: string | null;
  worktreePath?: string | null;
  branch?: string | null;
  base?: string | null;
  fetchFirst?: boolean | null;
}

export async function dispatchWorkItem(
  id: string,
  options: WorkItemDispatchOptions = {},
): Promise<string> {
  const sessionId = await tauriWorkItemDispatch(id, options);
  bindSessionToWorkItem(id, sessionId);
  return sessionId;
}

export function getWorkItemSnapshot(id: string): WorkItem | undefined {
  return get(workItems).find((i) => i.id === id);
}
