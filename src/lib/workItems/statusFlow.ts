import type { WorkItemStatus } from "$lib/bindings";

const NEXT_STATUS: Partial<Record<WorkItemStatus, WorkItemStatus>> = {
  todo: "planning",
  planning: "doing",
  doing: "review",
  review: "done",
};

export interface WorkItemMoveTargetOptions {
  reviewAcceptsDone?: boolean;
}

export function nextWorkItemStatuses(
  status: WorkItemStatus,
  options: WorkItemMoveTargetOptions = {},
): WorkItemStatus[] {
  const next = NEXT_STATUS[status];
  if (!next) return [];
  if (status === "review" && next === "done" && options.reviewAcceptsDone) {
    return [];
  }
  return [next];
}
