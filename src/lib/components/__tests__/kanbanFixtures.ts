import { DEFAULT_SETTINGS } from "$lib/types";
import { normalizeKanbanSettings } from "$lib/workItems/workflow";

export function kanbanWithPrReviewProfile() {
  const kanban = normalizeKanbanSettings(DEFAULT_SETTINGS.kanban);
  return {
    ...kanban,
    workflow: {
      ...kanban.workflow,
      phases: {
        ...kanban.workflow.phases,
        review: {
          ...kanban.workflow.phases.review,
          agentProfile: "phase-review",
          stages: {
            ...kanban.workflow.phases.review.stages,
            pr_review: {
              ...kanban.workflow.phases.review.stages.pr_review,
              agentProfile: "codex-review",
            },
          },
        },
      },
    },
  };
}
