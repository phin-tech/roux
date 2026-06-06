import type { Attachment, WorkItemRun } from "$lib/types/workItems";

export interface WorkItemCardHistoryEvent {
  kind: "created" | "archived";
  createdAt: number;
}

export interface WorkItemHistoryInput {
  cardEvents: WorkItemCardHistoryEvent[];
  runs: Pick<WorkItemRun, "id" | "kind" | "status" | "updatedAt">[];
  attachments: Pick<Attachment, "id" | "title" | "updatedAt">[];
}

export interface WorkItemHistoryRow {
  id: string;
  label: string;
  at: number;
}

export function buildWorkItemHistoryRows(
  input: WorkItemHistoryInput,
): WorkItemHistoryRow[] {
  return [
    ...input.cardEvents.map((event) => ({
      id: `card:${event.kind}:${event.createdAt}`,
      label: cardEventLabel(event.kind),
      at: event.createdAt,
    })),
    ...input.runs.map((run) => ({
      id: `run:${run.id}`,
      label: `${capitalize(run.kind)} run ${run.status}`,
      at: run.updatedAt,
    })),
    ...input.attachments.map((attachment) => ({
      id: `attachment:${attachment.id}`,
      label: `${attachment.title?.trim() || "Attachment"} attached`,
      at: attachment.updatedAt,
    })),
  ].sort((a, b) => b.at - a.at || a.id.localeCompare(b.id));
}

function cardEventLabel(kind: WorkItemCardHistoryEvent["kind"]): string {
  switch (kind) {
    case "archived":
      return "Archived card";
    case "created":
      return "Created card";
  }
}

function capitalize(value: string): string {
  return value.slice(0, 1).toUpperCase() + value.slice(1);
}
