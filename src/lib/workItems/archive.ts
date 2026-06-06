import type { WorkItem } from "$lib/bindings";

export interface ArchivedWorkItems {
  active: WorkItem[];
  archived: WorkItem[];
}

export function splitArchivedWorkItems(items: WorkItem[]): ArchivedWorkItems {
  const active: WorkItem[] = [];
  const archived: WorkItem[] = [];
  for (const item of items) {
    if (item.archivedAt == null) active.push(item);
    else archived.push(item);
  }
  return { active, archived };
}

export type WorkItemDetailKeyAction = "close" | "none";

export function workItemDetailKeyAction(event: {
  key: string;
}): WorkItemDetailKeyAction {
  return event.key === "Escape" ? "close" : "none";
}
