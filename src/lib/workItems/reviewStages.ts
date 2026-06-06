export interface ReviewStage {
  id: string;
  label: string;
  order: number;
}

export const DEFAULT_REVIEW_STAGES: ReviewStage[] = [
  { id: "local_review", label: "Local Review", order: 0 },
  { id: "pr_review", label: "PR Review", order: 1 },
];

export function reviewStageLabel(id: string | null | undefined): string | null {
  if (!id) return null;
  return DEFAULT_REVIEW_STAGES.find((stage) => stage.id === id)?.label ?? id;
}
