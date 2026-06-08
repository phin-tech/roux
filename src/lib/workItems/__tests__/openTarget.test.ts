import { describe, expect, it } from "vitest";
import { resolveWorkItemOpenTarget } from "../openTarget";
import type { WorkItem, WorkItemRun } from "$lib/bindings";
import type { WorkItemReviewPackage } from "$lib/workItems/reviewPackage";

function makeItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "item-1",
    projectId: null,
    parentId: null,
    title: "Test Item",
    body: null,
    status: "todo",
    workflowId: null,
    workflowStageId: null,
    workflowStageLabel: null,
    reviewStageId: null,
    repoPath: null,
    agentProfile: null,
    baseBranch: null,
    worktreePath: null,
    branch: null,
    fetchFirst: null,
    startError: null,
    sessionId: null,
    provider: null,
    externalId: null,
    externalUrl: null,
    sortOrder: 0,
    pinnedPrUrl: null,
    archivedAt: null,
    cost: null,
    createdAt: 1000,
    updatedAt: 2000,
    ...overrides,
  };
}

function makeRun(overrides: Partial<WorkItemRun> = {}): WorkItemRun {
  return {
    id: "run-1",
    workItemId: "item-1",
    kind: "implementation",
    sessionId: null,
    ptyId: null,
    provider: null,
    profileId: null,
    status: "running",
    worktreePath: null,
    branch: null,
    cost: null,
    createdAt: 1000,
    startedAt: null,
    endedAt: null,
    updatedAt: 2000,
    ...overrides,
  };
}

function makeReviewPackage(
  overrides: Partial<WorkItemReviewPackage> = {},
): WorkItemReviewPackage {
  return {
    runId: null,
    sessionId: null,
    plan: null,
    feedback: null,
    agentSummary: null,
    tests: null,
    changedFiles: [],
    worktreePath: null,
    worktreeLabel: null,
    branch: null,
    prUrl: null,
    ...overrides,
  };
}

describe("resolveWorkItemOpenTarget", () => {
  // ── Planning card ────────────────────────────────────────────────────

  it("resolves latest planning run for a planning card", () => {
    const item = makeItem({ status: "planning" });
    const run = makeRun({
      id: "plan-run-1",
      kind: "planning",
      sessionId: "plan-session",
      ptyId: "plan-pty",
    });
    const target = resolveWorkItemOpenTarget(item, [run], null);
    expect(target).toEqual({
      sessionId: "plan-session",
      ptyId: "plan-pty",
      runId: "plan-run-1",
      kind: "planning",
      label: "Open planning terminal",
    });
  });

  it("resolves planning run after completion/archive (terminal status)", () => {
    const item = makeItem({ status: "planning" });
    const run = makeRun({
      id: "plan-done",
      kind: "planning",
      sessionId: "plan-session",
      status: "done",
      endedAt: 3000,
    });
    const target = resolveWorkItemOpenTarget(item, [run], null);
    expect(target).toEqual({
      sessionId: "plan-session",
      ptyId: null,
      runId: "plan-done",
      kind: "planning",
      label: "Open planning terminal",
    });
  });

  it("resolves planning run with stopped status", () => {
    const item = makeItem({ status: "planning" });
    const run = makeRun({
      id: "plan-stopped",
      kind: "planning",
      sessionId: "plan-session",
      status: "stopped",
    });
    const target = resolveWorkItemOpenTarget(item, [run], null);
    expect(target?.sessionId).toBe("plan-session");
    expect(target?.kind).toBe("planning");
  });

  it("resolves planning run with failed status", () => {
    const item = makeItem({ status: "planning" });
    const run = makeRun({
      id: "plan-failed",
      kind: "planning",
      sessionId: "plan-session",
      status: "failed",
    });
    const target = resolveWorkItemOpenTarget(item, [run], null);
    expect(target?.sessionId).toBe("plan-session");
    expect(target?.kind).toBe("planning");
  });

  it("planning card passes ptyId when present on the run", () => {
    const item = makeItem({ status: "planning" });
    const run = makeRun({
      id: "plan-with-pty",
      kind: "planning",
      sessionId: "plan-session",
      ptyId: "live-pty",
    });
    const target = resolveWorkItemOpenTarget(item, [run], null);
    expect(target?.ptyId).toBe("live-pty");
  });

  it("planning card resolves latest planning run among multiple", () => {
    const item = makeItem({ status: "planning" });
    const older = makeRun({
      id: "older",
      kind: "planning",
      sessionId: "old-session",
      updatedAt: 1000,
      createdAt: 1000,
    });
    const newer = makeRun({
      id: "newer",
      kind: "planning",
      sessionId: "new-session",
      updatedAt: 2000,
      createdAt: 2000,
    });
    const target = resolveWorkItemOpenTarget(item, [older, newer], null);
    expect(target?.sessionId).toBe("new-session");
    expect(target?.runId).toBe("newer");
  });

  it("planning card prefers planning run over item.sessionId", () => {
    const item = makeItem({
      status: "planning",
      sessionId: "item-session",
    });
    const planRun = makeRun({
      id: "plan-run",
      kind: "planning",
      sessionId: "plan-session",
    });
    const target = resolveWorkItemOpenTarget(item, [planRun], null);
    // Planning card should prefer the planning run's session
    expect(target?.sessionId).toBe("plan-session");
    expect(target?.kind).toBe("planning");
  });

  it("planning card without planning run falls through to item.sessionId", () => {
    const item = makeItem({
      status: "planning",
      sessionId: "item-session",
    });
    // No planning runs, but item has a sessionId
    const target = resolveWorkItemOpenTarget(item, [], null);
    expect(target?.sessionId).toBe("item-session");
    expect(target?.kind).toBe("implementation");
  });

  // ── Bound session (item.sessionId) ───────────────────────────────────

  it("resolves item.sessionId for an implementation card", () => {
    const item = makeItem({
      status: "doing",
      sessionId: "impl-session",
    });
    const target = resolveWorkItemOpenTarget(item, [], null);
    expect(target).toEqual({
      sessionId: "impl-session",
      ptyId: null,
      runId: null,
      kind: "implementation",
      label: "Open terminal",
    });
  });

  it("item.sessionId remains valid for normal implementation cards", () => {
    const item = makeItem({
      status: "doing",
      sessionId: "bound-session",
    });
    const runs = [
      makeRun({
        id: "impl-run",
        kind: "implementation",
        sessionId: "bound-session",
        status: "running",
      }),
    ];
    const target = resolveWorkItemOpenTarget(item, runs, null);
    expect(target?.sessionId).toBe("bound-session");
    expect(target?.kind).toBe("implementation");
  });

  // ── Implementation run fallback ──────────────────────────────────────

  it("implementation run resolves when item.sessionId is absent", () => {
    const item = makeItem({ status: "doing", sessionId: null });
    const implRun = makeRun({
      id: "impl-run",
      kind: "implementation",
      sessionId: "impl-session",
      ptyId: "impl-pty",
    });
    const target = resolveWorkItemOpenTarget(item, [implRun], null);
    expect(target).toEqual({
      sessionId: "impl-session",
      ptyId: "impl-pty",
      runId: "impl-run",
      kind: "implementation",
      label: "Open terminal",
    });
  });

  it("implementation run resolves when item.sessionId is null and runs exist", () => {
    const item = makeItem({ status: "doing", sessionId: null });
    const implRun = makeRun({
      id: "impl-1",
      kind: "implementation",
      sessionId: "impl-session-1",
    });
    const target = resolveWorkItemOpenTarget(item, [implRun], null);
    expect(target?.sessionId).toBe("impl-session-1");
  });

  it("stale/missing ptyId does not block resolving by sessionId", () => {
    const item = makeItem({ status: "doing", sessionId: null });
    const implRun = makeRun({
      id: "impl-no-pty",
      kind: "implementation",
      sessionId: "impl-session",
      ptyId: null,
    });
    const target = resolveWorkItemOpenTarget(item, [implRun], null);
    expect(target?.sessionId).toBe("impl-session");
    expect(target?.ptyId).toBeNull();
  });

  // ── Review card ──────────────────────────────────────────────────────

  it("review card resolves from reviewPackage.sessionId", () => {
    const item = makeItem({ status: "review" });
    const rp = makeReviewPackage({
      sessionId: "review-session",
      runId: "review-run-1",
    });
    const target = resolveWorkItemOpenTarget(item, [], rp);
    expect(target).toEqual({
      sessionId: "review-session",
      ptyId: null,
      runId: "review-run-1",
      kind: "review",
      label: "Open review terminal",
    });
  });

  it("review card resolves from review run when reviewPackage has no sessionId", () => {
    const item = makeItem({ status: "review" });
    const rp = makeReviewPackage({ sessionId: null });
    const reviewRun = makeRun({
      id: "review-run",
      kind: "review",
      sessionId: "review-session",
      ptyId: "review-pty",
    });
    const target = resolveWorkItemOpenTarget(item, [reviewRun], rp);
    expect(target).toEqual({
      sessionId: "review-session",
      ptyId: "review-pty",
      runId: "review-run",
      kind: "review",
      label: "Open review terminal",
    });
  });

  it("review-kind run resolves when latest and has sessionId", () => {
    const item = makeItem({ status: "review" });
    const rp = makeReviewPackage({ sessionId: null });
    const olderReview = makeRun({
      id: "old-review",
      kind: "review",
      sessionId: "old-review-session",
      updatedAt: 1000,
    });
    const newerImpl = makeRun({
      id: "new-impl",
      kind: "implementation",
      sessionId: "new-impl-session",
      updatedAt: 2000,
    });
    // Review kind should be preferred over later implementation for review cards
    const target = resolveWorkItemOpenTarget(item, [olderReview, newerImpl], rp);
    expect(target?.sessionId).toBe("old-review-session");
    expect(target?.kind).toBe("review");
  });

  it("review card falls back to implementation run when no review run has sessionId", () => {
    const item = makeItem({ status: "review" });
    const rp = makeReviewPackage({ sessionId: null });
    // No review-kind runs, only implementation
    const implRun = makeRun({
      id: "impl-run",
      kind: "implementation",
      sessionId: "impl-session",
    });
    const target = resolveWorkItemOpenTarget(item, [implRun], rp);
    expect(target?.sessionId).toBe("impl-session");
  });

  // ── Fallback to any run ──────────────────────────────────────────────

  it("falls back to any run with sessionId when no kind-specific match", () => {
    const item = makeItem({ status: "doing", sessionId: null });
    // No implementation runs, but has planning and review runs
    const planRun = makeRun({
      id: "plan-run",
      kind: "planning",
      sessionId: "plan-session",
      updatedAt: 3000,
    });
    const reviewRun = makeRun({
      id: "review-run",
      kind: "review",
      sessionId: "review-session",
      updatedAt: 2000,
    });
    const target = resolveWorkItemOpenTarget(item, [planRun, reviewRun], null);
    // Should pick the latest (planning run at updatedAt=3000)
    expect(target?.sessionId).toBe("plan-session");
    expect(target?.kind).toBe("planning");
  });

  it("falls back correctly with mixed run kinds", () => {
    const item = makeItem({ status: "doing", sessionId: null });
    const runs = [
      makeRun({
        id: "r1",
        kind: "review",
        sessionId: "rev-session",
        updatedAt: 1500,
      }),
      makeRun({
        id: "r2",
        kind: "planning",
        sessionId: "plan-session",
        updatedAt: 2500,
      }),
    ];
    // No implementation runs, falls to any-run → picks planning (latest)
    const target = resolveWorkItemOpenTarget(item, runs, null);
    expect(target?.sessionId).toBe("plan-session");
  });

  // ── Null / not found ─────────────────────────────────────────────────

  it("returns null when nothing is found", () => {
    const item = makeItem({ status: "todo", sessionId: null });
    const target = resolveWorkItemOpenTarget(item, [], null);
    expect(target).toBeNull();
  });

  it("returns null when runs exist but none have sessionId", () => {
    const item = makeItem({ status: "doing", sessionId: null });
    const runs = [
      makeRun({ id: "r1", kind: "implementation", sessionId: null }),
      makeRun({ id: "r2", kind: "planning", sessionId: null }),
    ];
    const target = resolveWorkItemOpenTarget(item, runs, null);
    expect(target).toBeNull();
  });

  // ── Tie-breaking ─────────────────────────────────────────────────────

  it("deterministic tie-breaking by id when timestamps match", () => {
    const item = makeItem({ status: "doing", sessionId: null });
    const runA = makeRun({
      id: "run-a",
      kind: "implementation",
      sessionId: "session-a",
      updatedAt: 2000,
      createdAt: 1000,
    });
    const runB = makeRun({
      id: "run-b",
      kind: "implementation",
      sessionId: "session-b",
      updatedAt: 2000,
      createdAt: 1000,
    });
    const target = resolveWorkItemOpenTarget(item, [runA, runB], null);
    // run-b has larger id
    expect(target?.sessionId).toBe("session-b");
  });

  // ── Purity ───────────────────────────────────────────────────────────

  it("does not mutate input arrays or objects", () => {
    const item = makeItem({ status: "planning" });
    const runs = [
      makeRun({
        id: "plan-run",
        kind: "planning",
        sessionId: "plan-session",
      }),
    ];
    const rp = makeReviewPackage();

    const itemJson = JSON.stringify(item);
    const runsJson = JSON.stringify(runs);
    const rpJson = JSON.stringify(rp);

    resolveWorkItemOpenTarget(item, runs, rp);

    expect(JSON.stringify(item)).toBe(itemJson);
    expect(JSON.stringify(runs)).toBe(runsJson);
    expect(JSON.stringify(rp)).toBe(rpJson);
  });
});
