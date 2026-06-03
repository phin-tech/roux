import { writable } from "svelte/store";
import type { WorkItem, WorkItemStatus } from "$lib/bindings";

export const WORK_ITEM_DRAG_MIME = "application/x-roux-work-item";
const WORK_ITEM_STATUSES = new Set<WorkItemStatus>([
  "todo",
  "ready",
  "doing",
  "review",
  "done",
]);

export interface WorkItemDragPayload {
  itemId: string;
  fromStatus: WorkItemStatus;
}

/**
 * The card currently being dragged, mirrored as a store so drop zones can
 * highlight themselves without reading the (write-only during drag)
 * DataTransfer. Cleared on dragend.
 */
export const draggedWorkItem = writable<WorkItemDragPayload | null>(null);

export function workItemDragPayload(item: WorkItem): WorkItemDragPayload {
  return { itemId: item.id, fromStatus: item.status };
}

function isWorkItemStatus(value: string): value is WorkItemStatus {
  return WORK_ITEM_STATUSES.has(value as WorkItemStatus);
}

export function writeWorkItemDragData(
  dataTransfer: DataTransfer | null,
  item: WorkItem,
): boolean {
  if (!dataTransfer) return false;
  const payload = workItemDragPayload(item);
  draggedWorkItem.set(payload);
  dataTransfer.effectAllowed = "move";
  dataTransfer.setData(WORK_ITEM_DRAG_MIME, JSON.stringify(payload));
  dataTransfer.setData("text/plain", item.title);
  return true;
}

export function clearDraggedWorkItem(): void {
  draggedWorkItem.set(null);
}

export function hasWorkItemDragData(
  dataTransfer: DataTransfer | null,
): boolean {
  if (!dataTransfer) return false;
  return Array.from(dataTransfer.types).includes(WORK_ITEM_DRAG_MIME);
}

export function readWorkItemDragData(
  dataTransfer: DataTransfer | null,
): WorkItemDragPayload | null {
  if (!dataTransfer) return null;

  const raw = dataTransfer.getData(WORK_ITEM_DRAG_MIME);
  if (!raw) return null;

  try {
    const parsed = JSON.parse(raw) as Partial<WorkItemDragPayload>;
    if (typeof parsed.itemId !== "string" || parsed.itemId.trim() === "") {
      return null;
    }
    if (
      typeof parsed.fromStatus !== "string" ||
      !isWorkItemStatus(parsed.fromStatus)
    ) {
      return null;
    }
    return {
      itemId: parsed.itemId,
      fromStatus: parsed.fromStatus,
    };
  } catch {
    return null;
  }
}
