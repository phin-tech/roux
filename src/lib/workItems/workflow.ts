import type {
  KanbanSettings,
  KanbanWorkflowPhaseCategory,
  KanbanWorkflowPhaseSettings,
  KanbanWorkflowPromptSettings,
  KanbanWorkflowSettings,
  KanbanWorkflowStageKind,
  KanbanWorkflowStageSettings,
} from "$lib/bindings";
import defaultWorkflow from "./defaultWorkflow.json";

export const WORKFLOW_PHASE_IDS = [
  "todo",
  "planning",
  "doing",
  "review",
  "done",
] as const;
export type WorkflowPhaseId = (typeof WORKFLOW_PHASE_IDS)[number];

export const WORKFLOW_STAGE_IDS = [
  "todo",
  "planning",
  "implementation",
  "fix_ci",
  "local_review",
  "pr_review",
  "done",
] as const;
export type WorkflowStageId = (typeof WORKFLOW_STAGE_IDS)[number];

export const REVIEW_STAGE_IDS = ["local_review", "pr_review"] as const;
export type ReviewStageId = (typeof REVIEW_STAGE_IDS)[number];
export const DEFAULT_REVIEW_STAGE_ID: ReviewStageId = "local_review";

export type RequiredStageSettings = Required<
  Pick<
    KanbanWorkflowStageSettings,
    "label" | "actionLabel" | "category" | "kind" | "agentProfile" | "instructions"
  >
> & {
  prompt: KanbanWorkflowPromptSettings;
  runner: KanbanWorkflowStageSettings["runner"];
  gate: KanbanWorkflowStageSettings["gate"];
  env: Record<string, string>;
  transitions: NonNullable<KanbanWorkflowStageSettings["transitions"]>;
  terminal: boolean;
};

export type RequiredPhaseSettings = Required<
  Pick<
    KanbanWorkflowPhaseSettings,
    "category" | "label" | "agentProfile" | "instructions"
  >
> & {
  prompt: KanbanWorkflowPromptSettings;
  env: Record<string, string>;
  stageOrder: string[];
  stages: Record<string, RequiredStageSettings>;
};

export type RequiredWorkflowSettings = Required<
  Pick<KanbanWorkflowSettings, "id" | "label">
> & {
  env: Record<string, string>;
  phaseOrder: string[];
  phases: Record<WorkflowPhaseId, RequiredPhaseSettings>;
};

export type RequiredKanbanSettings = Required<
  Pick<KanbanSettings, "startupSidebar" | "workflowPath" | "workflowLoadError">
> & {
  workflow: RequiredWorkflowSettings;
};

const BUNDLED_DEFAULT_WORKFLOW =
  defaultWorkflow as unknown as RequiredWorkflowSettings;
const DEFAULT_PROMPT_SETTINGS: KanbanWorkflowPromptSettings = {
  mode: "append",
  instructions: "",
};

export const DEFAULT_WORKFLOW_SETTINGS: RequiredWorkflowSettings =
  cloneWorkflow(BUNDLED_DEFAULT_WORKFLOW);

const DEFAULT_PHASES: Record<WorkflowPhaseId, RequiredPhaseSettings> =
  clonePhases(DEFAULT_WORKFLOW_SETTINGS.phases);

export const DEFAULT_KANBAN_SETTINGS: RequiredKanbanSettings = {
  startupSidebar: "restore",
  workflowPath: null,
  workflowLoadError: null,
  workflow: cloneWorkflow(DEFAULT_WORKFLOW_SETTINGS),
};

export function normalizeKanbanSettings(
  kanban: KanbanSettings | null | undefined,
): RequiredKanbanSettings {
  return {
    startupSidebar:
      kanban?.startupSidebar ?? DEFAULT_KANBAN_SETTINGS.startupSidebar,
    workflowPath: nonEmpty(kanban?.workflowPath) ?? null,
    workflowLoadError: nonEmpty(kanban?.workflowLoadError) ?? null,
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
    env: normalizeEnv(workflow?.env),
    phaseOrder: normalizeOrder(workflow?.phaseOrder, WORKFLOW_PHASE_IDS),
    phases,
  };
}

export function workflowStage(
  kanban: KanbanSettings | null | undefined,
  id: string | null | undefined,
): RequiredStageSettings | null {
  if (!id) return null;
  const workflow = normalizeKanbanSettings(kanban).workflow;
  for (const phaseId of WORKFLOW_PHASE_IDS) {
    const stage = workflow.phases[phaseId].stages[id];
    if (stage) return stage;
  }
  return null;
}

export function workflowStageLabel(
  id: string | null | undefined,
  kanban?: KanbanSettings | null,
): string | null {
  if (!id) return null;
  return nonEmpty(workflowStage(kanban, id)?.label) ?? id;
}

export function workflowStageActionLabel(
  id: string | null | undefined,
  kanban?: KanbanSettings | null,
): string | null {
  if (!id) return null;
  const stage = workflowStage(kanban, id);
  return nonEmpty(stage?.actionLabel) ?? nonEmpty(stage?.label) ?? id;
}

export function reviewStageLabel(
  id: string | null | undefined,
  kanban?: KanbanSettings | null,
): string | null {
  return workflowStageLabel(id, kanban);
}

export function workflowReviewStageLabel(
  kanban: KanbanSettings | null | undefined,
  id: string,
): string | null {
  return workflowStageLabel(id, kanban);
}

export function reviewAgentProfileId(
  kanban: KanbanSettings | null | undefined,
  stageId: string | null | undefined,
): string | null {
  const activeStageId = stageId ?? DEFAULT_REVIEW_STAGE_ID;
  const stage = workflowStage(kanban, activeStageId);
  const review = normalizeKanbanSettings(kanban).workflow.phases.review;
  return nonEmpty(stage?.agentProfile) ?? nonEmpty(review.agentProfile) ?? null;
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
    prompt: normalizePrompt(phase?.prompt ?? fallback.prompt),
    env: normalizeEnv(phase?.env),
    stageOrder: normalizeOrder(phase?.stageOrder, fallback.stageOrder),
    stages: normalizeStages(id, phase?.stages),
  };
}

function normalizeStages(
  phaseId: WorkflowPhaseId,
  stages: KanbanWorkflowPhaseSettings["stages"],
): Record<string, RequiredStageSettings> {
  const fallbackPhase = DEFAULT_PHASES[phaseId];
  const normalized: Record<string, RequiredStageSettings> = {};
  const ids = new Set([...fallbackPhase.stageOrder, ...Object.keys(stages ?? {})]);
  for (const id of ids) {
    const fallback = fallbackPhase.stages[id];
    const stage = stages?.[id];
    if (!fallback && !stage) continue;
    normalized[id] = normalizeStage(
      stage,
      fallback,
      fallbackPhase.category,
      "manual",
    );
  }
  return normalized;
}

function normalizeStage(
  stage: KanbanWorkflowStageSettings | undefined,
  fallback: RequiredStageSettings | undefined,
  category: KanbanWorkflowPhaseCategory,
  kind: KanbanWorkflowStageKind,
): RequiredStageSettings {
  return {
    label: nonEmpty(stage?.label) ?? fallback?.label ?? "Stage",
    actionLabel: nonEmpty(stage?.actionLabel) ?? fallback?.actionLabel ?? "Run",
    category: stage?.category ?? fallback?.category ?? category,
    kind: stage?.kind ?? fallback?.kind ?? kind,
    agentProfile: nonEmpty(stage?.agentProfile) ?? fallback?.agentProfile ?? null,
    instructions: stage?.instructions?.trim() ?? fallback?.instructions ?? "",
    prompt: normalizePrompt(stage?.prompt ?? fallback?.prompt),
    runner: stage?.runner ?? fallback?.runner ?? null,
    gate: stage?.gate ?? fallback?.gate ?? null,
    env: normalizeEnv(stage?.env ?? fallback?.env),
    transitions: stage?.transitions ?? fallback?.transitions ?? {},
    terminal: stage?.terminal ?? fallback?.terminal ?? false,
  };
}

function normalizeOrder<T extends string>(
  order: readonly string[] | null | undefined,
  fallback: readonly T[] | readonly string[],
): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const id of order ?? []) {
    const trimmed = id.trim();
    if (trimmed && !seen.has(trimmed)) {
      seen.add(trimmed);
      normalized.push(trimmed);
    }
  }
  for (const id of fallback) {
    if (!seen.has(id)) normalized.push(id);
  }
  return normalized;
}

function normalizeEnv(
  env: Record<string, string | undefined> | null | undefined,
): Record<string, string> {
  const normalized: Record<string, string> = {};
  for (const [key, value] of Object.entries(env ?? {})) {
    const normalizedKey = key.trim();
    if (normalizedKey && value != null) normalized[normalizedKey] = value;
  }
  return normalized;
}

function normalizePrompt(
  prompt: KanbanWorkflowPromptSettings | null | undefined,
): KanbanWorkflowPromptSettings {
  return {
    mode: prompt?.mode ?? DEFAULT_PROMPT_SETTINGS.mode,
    instructions: prompt?.instructions?.trim() ?? "",
  };
}

function clonePhases(
  phases: Record<WorkflowPhaseId, RequiredPhaseSettings>,
): Record<WorkflowPhaseId, RequiredPhaseSettings> {
  const cloned = {} as Record<WorkflowPhaseId, RequiredPhaseSettings>;
  for (const phaseId of WORKFLOW_PHASE_IDS) {
    const phase = phases[phaseId];
    cloned[phaseId] = {
      ...phase,
      prompt: { ...phase.prompt },
      env: { ...phase.env },
      stageOrder: [...phase.stageOrder],
      stages: cloneStages(phase.stages),
    };
  }
  return cloned;
}

function cloneStages(
  stages: Record<string, RequiredStageSettings>,
): Record<string, RequiredStageSettings> {
  const cloned: Record<string, RequiredStageSettings> = {};
  for (const [id, stage] of Object.entries(stages)) {
    cloned[id] = {
      ...stage,
      prompt: { ...stage.prompt },
      env: { ...stage.env },
      transitions: { ...stage.transitions },
    };
  }
  return cloned;
}

function cloneWorkflow(
  workflow: RequiredWorkflowSettings,
): RequiredWorkflowSettings {
  return {
    id: workflow.id,
    label: workflow.label,
    env: { ...workflow.env },
    phaseOrder: [...workflow.phaseOrder],
    phases: clonePhases(workflow.phases),
  };
}

function nonEmpty(value: string | null | undefined): string | null {
  const trimmed = value?.trim() ?? "";
  return trimmed.length > 0 ? trimmed : null;
}
