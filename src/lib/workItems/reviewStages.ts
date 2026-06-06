import type { KanbanSettings } from "$lib/bindings";
import { reviewStageLabel as workflowStageLabel } from "./workflow";

export function reviewStageLabel(
  id: string | null | undefined,
  kanban?: KanbanSettings | null,
): string | null {
  return workflowStageLabel(id, kanban);
}
