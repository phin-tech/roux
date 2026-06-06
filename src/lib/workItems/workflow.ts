import type {
  KanbanReviewStageSettings,
  KanbanSettings,
  KanbanWorkflowPhaseSettings,
  KanbanWorkflowSettings,
} from "$lib/bindings";

export const WORKFLOW_PHASE_IDS = [
  "planning",
  "implementation",
  "review",
] as const;
export type WorkflowPhaseId = (typeof WORKFLOW_PHASE_IDS)[number];

export const REVIEW_STAGE_IDS = ["local_review", "pr_review"] as const;
export type ReviewStageId = (typeof REVIEW_STAGE_IDS)[number];
export const DEFAULT_REVIEW_STAGE_ID: ReviewStageId = "local_review";

const DEFAULT_PHASES: Record<WorkflowPhaseId, RequiredPhaseSettings> = {
  planning: {
    category: "planning",
    label: "Planning",
    agentProfile: null,
    instructions: "",
    stages: {},
  },
  implementation: {
    category: "implementation",
    label: "Implementation",
    agentProfile: null,
    instructions: "",
    stages: {},
  },
  review: {
    category: "review",
    label: "Review",
    agentProfile: null,
    instructions: "",
    stages: {
      local_review: {
        label: "Local Review",
        agentProfile: null,
        instructions: "",
      },
      pr_review: {
        label: "PR Review",
        agentProfile: null,
        instructions: "",
      },
    },
  },
};

export type RequiredReviewStageSettings = Required<
  Pick<KanbanReviewStageSettings, "label" | "agentProfile" | "instructions">
>;

export type RequiredPhaseSettings = Required<
  Pick<
    KanbanWorkflowPhaseSettings,
    "category" | "label" | "agentProfile" | "instructions"
  >
> & {
  stages: Record<string, RequiredReviewStageSettings>;
};

export type RequiredWorkflowSettings = Required<
  Pick<KanbanWorkflowSettings, "id" | "label">
> & {
  phases: Record<WorkflowPhaseId, RequiredPhaseSettings>;
};

export type RequiredKanbanSettings = Required<
  Pick<KanbanSettings, "startupSidebar">
> & {
  workflow: RequiredWorkflowSettings;
};

export const DEFAULT_KANBAN_SETTINGS: RequiredKanbanSettings = {
  startupSidebar: "restore",
  workflow: {
    id: "default",
    label: "Default",
    phases: clonePhases(DEFAULT_PHASES),
  },
};

export function normalizeKanbanSettings(
  kanban: KanbanSettings | null | undefined,
): RequiredKanbanSettings {
  return {
    startupSidebar:
      kanban?.startupSidebar ?? DEFAULT_KANBAN_SETTINGS.startupSidebar,
    workflow: normalizeWorkflow(kanban?.workflow),
  };
}

export function normalizeWorkflow(
  workflow: KanbanWorkflowSettings | null | undefined,
): RequiredWorkflowSettings {
  const phases = {} as Record<WorkflowPhaseId, RequiredPhaseSettings>;
  for (const id of WORKFLOW_PHASE_IDS) {
    phases[id] = normalizePhase(id, workflow?.phases?.[id]);
  }
  return {
    id: nonEmpty(workflow?.id) ?? DEFAULT_KANBAN_SETTINGS.workflow.id,
    label: nonEmpty(workflow?.label) ?? DEFAULT_KANBAN_SETTINGS.workflow.label,
    phases,
  };
}

export function reviewStageLabel(
  id: string | null | undefined,
  kanban?: KanbanSettings | null,
): string | null {
  if (!id) return null;
  return workflowReviewStageLabel(kanban, id) ?? id;
}

export function workflowReviewStageLabel(
  kanban: KanbanSettings | null | undefined,
  id: string,
): string | null {
  const workflow = normalizeKanbanSettings(kanban).workflow;
  return nonEmpty(workflow.phases.review.stages[id]?.label) ?? null;
}

export function reviewAgentProfileId(
  kanban: KanbanSettings | null | undefined,
  stageId: string | null | undefined,
): string | null {
  const review = normalizeKanbanSettings(kanban).workflow.phases.review;
  const activeStageId = stageId ?? DEFAULT_REVIEW_STAGE_ID;
  return (
    nonEmpty(review.stages[activeStageId]?.agentProfile) ??
    nonEmpty(review.agentProfile) ??
    null
  );
}

function normalizePhase(
  id: WorkflowPhaseId,
  phase: KanbanWorkflowPhaseSettings | null | undefined,
): RequiredPhaseSettings {
  const fallback = DEFAULT_PHASES[id];
  return {
    category: fallback.category,
    label: nonEmpty(phase?.label) ?? fallback.label,
    agentProfile: nonEmpty(phase?.agentProfile) ?? null,
    instructions: phase?.instructions?.trim() ?? "",
    stages: id === "review" ? normalizeReviewStages(phase?.stages) : {},
  };
}

function normalizeReviewStages(
  stages: KanbanWorkflowPhaseSettings["stages"],
): Record<ReviewStageId, RequiredReviewStageSettings> {
  const normalized = {} as Record<ReviewStageId, RequiredReviewStageSettings>;
  for (const id of REVIEW_STAGE_IDS) {
    const fallback = DEFAULT_PHASES.review.stages[id];
    const stage = stages?.[id];
    normalized[id] = {
      label: nonEmpty(stage?.label) ?? fallback.label,
      agentProfile: nonEmpty(stage?.agentProfile) ?? null,
      instructions: stage?.instructions?.trim() ?? "",
    };
  }
  return normalized;
}

function clonePhases(
  phases: Record<WorkflowPhaseId, RequiredPhaseSettings>,
): Record<WorkflowPhaseId, RequiredPhaseSettings> {
  const cloned = {} as Record<WorkflowPhaseId, RequiredPhaseSettings>;
  for (const phaseId of WORKFLOW_PHASE_IDS) {
    const stages: Record<string, RequiredReviewStageSettings> = {};
    for (const stageId of REVIEW_STAGE_IDS) {
      const stage = phases[phaseId].stages[stageId];
      if (stage) stages[stageId] = { ...stage };
    }
    cloned[phaseId] = { ...phases[phaseId], stages };
  }
  return cloned;
}

function nonEmpty(value: string | null | undefined): string | null {
  const trimmed = value?.trim() ?? "";
  return trimmed.length > 0 ? trimmed : null;
}
