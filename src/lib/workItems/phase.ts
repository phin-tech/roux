import type { WorkItemStatus } from "$lib/bindings";
import type { WorkItemDecision, WorkItemRun } from "$lib/types/workItems";

/**
 * Single source of truth for a work item card's phase.
 *
 * Two orthogonal axes feed this descriptor:
 *  - the board column (`status`/`lane`) is authoritative for PLACEMENT (which
 *    column the card sits in — driven by drag + daemon `moved` events);
 *  - the run axis (`activePlanningRun`, `sessionId`, attachments, pending
 *    decision) is authoritative for the card's ACTION affordance + live state.
 *
 * `lane` always mirrors `status` so the two axes can disagree without the card
 * silently collapsing them (e.g. a `ready` lane with no planning run yet, or a
 * blocked run while the column still reads `doing`).
 *
 * The action priority here ports `WorkItemCard.svelte`'s former nested
 * `{:else if}` chain verbatim. Note the action area never depended on a pending
 * decision — the blocked "attention" affordance is a separate button — so
 * `action` is computed independent of `isBlocked`.
 */
export type WorkItemPhaseName =
  | "todo"
  | "planning-active"
  | "plan-ready"
  | "implementing"
  | "blocked"
  | "review"
  | "done";

export type WorkItemAction =
  | { kind: "plan" }
  | { kind: "open-planning"; sessionId: string }
  | { kind: "approve-start" }
  | { kind: "configure" }
  | { kind: "start" }
  | { kind: "open-session"; sessionId: string }
  | { kind: "accept-review" }
  | { kind: "none" };

export interface WorkItemPhase {
  name: WorkItemPhaseName;
  /** Always `=== status`; authoritative for board placement. */
  lane: WorkItemStatus;
  action: WorkItemAction;
  /** Session to open for a pending decision (planning session preferred). */
  attentionSessionId: string | null;
  pendingDecision: WorkItemDecision | null;
  isBlocked: boolean;
  hasSession: boolean;
  hasPlanningSession: boolean;
  hasAttachedPlan: boolean;
  isPlanning: boolean;
  isStartable: boolean;
  /** Whether the menu's "Approve & start anyway" force-start is offered. */
  canForceStart: boolean;
}

export interface WorkItemPhaseInput {
  status: WorkItemStatus;
  /** The implementation session bound to the item (`item.sessionId`). */
  sessionId: string | null;
  activePlanningRun: WorkItemRun | null;
  hasAttachedPlan: boolean;
  pendingDecision: WorkItemDecision | null;
  /** `!!agentProfile && (!!repoPath || !!projectId)`. */
  isStartable: boolean;
}

export function workItemPhase(input: WorkItemPhaseInput): WorkItemPhase {
  const {
    status,
    sessionId,
    activePlanningRun,
    hasAttachedPlan,
    pendingDecision,
    isStartable,
  } = input;

  const planningSessionId = activePlanningRun?.sessionId ?? null;
  const hasSession = !!sessionId;
  const hasPlanningSession = !!planningSessionId;
  const isPlanning = status === "ready";
  const isTodo = status === "todo";
  const isBlocked = !!pendingDecision;

  const action: WorkItemAction = (() => {
    if (isTodo && !hasSession && !hasPlanningSession) return { kind: "plan" };
    if (status === "review") return { kind: "accept-review" };
    if (hasSession && sessionId) return { kind: "open-session", sessionId };
    if (isPlanning && hasAttachedPlan && !hasSession) {
      return isStartable ? { kind: "approve-start" } : { kind: "configure" };
    }
    if (hasPlanningSession && planningSessionId) {
      return { kind: "open-planning", sessionId: planningSessionId };
    }
    if (isPlanning && !hasAttachedPlan && !hasPlanningSession && isStartable) {
      return { kind: "plan" };
    }
    if (!hasSession && (!isPlanning || hasAttachedPlan || !isStartable)) {
      if (!isStartable) return { kind: "configure" };
      if (isPlanning) return { kind: "approve-start" };
      return { kind: "start" };
    }
    return { kind: "none" };
  })();

  const name: WorkItemPhaseName = (() => {
    if (isBlocked) return "blocked";
    if (status === "done") return "done";
    if (status === "review") return "review";
    if (hasSession) return "implementing";
    if (isPlanning && hasAttachedPlan) return "plan-ready";
    if (hasPlanningSession) return "planning-active";
    if (isPlanning) return "planning-active";
    return "todo";
  })();

  return {
    name,
    lane: status,
    action,
    attentionSessionId: pendingDecision
      ? (planningSessionId ?? sessionId ?? null)
      : null,
    pendingDecision: pendingDecision ?? null,
    isBlocked,
    hasSession,
    hasPlanningSession,
    hasAttachedPlan,
    isPlanning,
    isStartable,
    canForceStart: isPlanning && isStartable && !hasSession && !hasAttachedPlan,
  };
}
