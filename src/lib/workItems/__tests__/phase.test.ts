import { describe, expect, it } from "vitest";
import type { WorkItemStatus } from "$lib/bindings";
import type { WorkItemDecision, WorkItemRun } from "$lib/types/workItems";
import { workItemPhase, type WorkItemPhaseInput } from "../phase";

function planningRun(sessionId: string | null): WorkItemRun {
  return {
    id: "run-plan",
    workItemId: "item-1",
    kind: "planning",
    sessionId,
    ptyId: null,
    provider: null,
    profileId: null,
    status: "running",
    worktreePath: null,
    branch: null,
    cost: null,
    createdAt: 0,
    startedAt: null,
    endedAt: null,
    updatedAt: 0,
  };
}

function decision(): WorkItemDecision {
  return {
    id: "dec-1",
    runId: "run-1",
    question: "Pick one",
    options: [],
    defaultValue: null,
    timeoutAt: null,
    status: "pending",
    resolvedValue: null,
    resolvedBy: null,
    createdAt: 0,
    resolvedAt: null,
    updatedAt: 0,
  };
}

function input(
  overrides: Partial<WorkItemPhaseInput> = {},
): WorkItemPhaseInput {
  return {
    status: "todo",
    sessionId: null,
    activePlanningRun: null,
    hasAttachedPlan: false,
    pendingDecision: null,
    isStartable: true,
    ...overrides,
  };
}

describe("workItemPhase", () => {
  it("todo with no runs offers Plan", () => {
    const phase = workItemPhase(input({ status: "todo" }));
    expect(phase.name).toBe("todo");
    expect(phase.action.kind).toBe("plan");
  });

  it("planning with no planning run and startable offers Plan", () => {
    const phase = workItemPhase(input({ status: "planning" }));
    expect(phase.name).toBe("planning-active");
    expect(phase.action.kind).toBe("plan");
  });

  it("planning with an active planning session offers Open planning", () => {
    const phase = workItemPhase(
      input({ status: "planning", activePlanningRun: planningRun("sess-plan") }),
    );
    expect(phase.action).toEqual({
      kind: "open-planning",
      sessionId: "sess-plan",
    });
  });

  it("planning with an attached plan offers Approve & start", () => {
    const phase = workItemPhase(
      input({ status: "planning", hasAttachedPlan: true }),
    );
    expect(phase.name).toBe("plan-planning");
    expect(phase.action.kind).toBe("approve-start");
  });

  it("planning with an attached plan but not startable offers Configure", () => {
    const phase = workItemPhase(
      input({ status: "planning", hasAttachedPlan: true, isStartable: false }),
    );
    expect(phase.action.kind).toBe("configure");
  });

  it("a bound implementation session offers Open terminal", () => {
    const phase = workItemPhase(
      input({ status: "doing", sessionId: "sess-impl" }),
    );
    expect(phase.name).toBe("implementing");
    expect(phase.action).toEqual({
      kind: "open-session",
      sessionId: "sess-impl",
    });
  });

  it("review lane offers accept review", () => {
    const phase = workItemPhase(input({ status: "review" }));
    expect(phase.name).toBe("review");
    expect(phase.action.kind).toBe("accept-review");
  });

  it("doing lane with no session and startable offers Start", () => {
    const phase = workItemPhase(input({ status: "doing" }));
    expect(phase.action.kind).toBe("start");
  });

  it("not-startable, unbound item offers Configure", () => {
    const phase = workItemPhase(input({ status: "doing", isStartable: false }));
    expect(phase.action.kind).toBe("configure");
  });

  it("marks blocked and resolves the attention session (planning preferred)", () => {
    const withPlanning = workItemPhase(
      input({
        status: "planning",
        sessionId: "sess-impl",
        activePlanningRun: planningRun("sess-plan"),
        pendingDecision: decision(),
      }),
    );
    expect(withPlanning.isBlocked).toBe(true);
    expect(withPlanning.name).toBe("blocked");
    expect(withPlanning.attentionSessionId).toBe("sess-plan");

    const sessionOnly = workItemPhase(
      input({
        status: "doing",
        sessionId: "sess-impl",
        pendingDecision: decision(),
      }),
    );
    expect(sessionOnly.attentionSessionId).toBe("sess-impl");
  });

  it("computes the action independent of a pending decision", () => {
    const blocked = workItemPhase(
      input({
        status: "doing",
        sessionId: "sess-impl",
        pendingDecision: decision(),
      }),
    );
    // The action area still shows Open terminal; the attention button is separate.
    expect(blocked.action).toEqual({
      kind: "open-session",
      sessionId: "sess-impl",
    });
  });

  it("offers force-start only for a startable, plan-less planning item", () => {
    expect(workItemPhase(input({ status: "planning" })).canForceStart).toBe(true);
    expect(
      workItemPhase(input({ status: "planning", hasAttachedPlan: true }))
        .canForceStart,
    ).toBe(false);
    expect(
      workItemPhase(input({ status: "planning", isStartable: false }))
        .canForceStart,
    ).toBe(false);
    expect(workItemPhase(input({ status: "todo" })).canForceStart).toBe(false);
  });

  it("always mirrors status onto lane (placement authority)", () => {
    const statuses: WorkItemStatus[] = [
      "todo",
      "planning",
      "doing",
      "review",
      "done",
    ];
    for (const status of statuses) {
      expect(workItemPhase(input({ status })).lane).toBe(status);
    }
  });
});
