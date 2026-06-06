import { describe, expect, it } from "vitest";
import type { WorkItem } from "$lib/bindings";
import type {
  Attachment,
  WorkItemRun,
  WorkItemRunEvent,
} from "$lib/types/workItems";
import { buildWorkItemReviewPackage } from "../reviewPackage";

function workItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "wi-1",
    projectId: null,
    parentId: null,
    title: "Review me",
    body: null,
    status: "review",
    repoPath: "/repo/main",
    agentProfile: "claude",
    baseBranch: "main",
    worktreePath: "/repo/.worktrees/card",
    branch: "feature/card",
    fetchFirst: null,
    startError: null,
    sessionId: null,
    provider: null,
    externalId: null,
    externalUrl: null,
    sortOrder: 0,
    pinnedPrUrl: "https://github.com/phin-tech/roux/pull/90",
    reviewStageId: null,
    archivedAt: null,
    cost: null,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

function run(overrides: Partial<WorkItemRun> = {}): WorkItemRun {
  return {
    id: "run-1",
    workItemId: "wi-1",
    kind: "implementation",
    sessionId: "sess-1",
    ptyId: "pty-1",
    provider: "claude",
    profileId: "claude",
    status: "review",
    worktreePath: "/repo/.worktrees/review-card",
    branch: "feature/review-card",
    cost: null,
    createdAt: 1,
    startedAt: 1,
    endedAt: 2,
    updatedAt: 2,
    ...overrides,
  };
}

function attachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: "att-1",
    documentId: "wi-1.plan",
    targetKind: "workItem",
    targetId: "wi-1",
    title: "Implementation Plan",
    contentKind: "text",
    mimeType: "text/markdown",
    sourcePath: null,
    byteLen: 12,
    sha256: "sha",
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

function event(overrides: Partial<WorkItemRunEvent> = {}): WorkItemRunEvent {
  return {
    id: "event-1",
    runId: "run-1",
    kind: "result",
    payload: {
      summary: "Implemented the review package.",
      tests: ["npm run test"],
      changedFiles: ["src/lib/components/WorkItemCard.svelte"],
    },
    createdAt: 3,
    ...overrides,
  };
}

describe("buildWorkItemReviewPackage", () => {
  it("extracts plan, feedback, run context, and result details", () => {
    const pkg = buildWorkItemReviewPackage(
      workItem(),
      [run()],
      [
        attachment(),
        attachment({
          id: "att-2",
          documentId: "wi-1.feedback",
          title: "Review feedback",
          updatedAt: 2,
        }),
      ],
      [event()],
    );

    expect(pkg).toMatchObject({
      runId: "run-1",
      sessionId: "sess-1",
      plan: { title: "Implementation Plan", documentId: "wi-1.plan" },
      feedback: { title: "Review feedback", documentId: "wi-1.feedback" },
      agentSummary: "Implemented the review package.",
      tests: "npm run test",
      changedFiles: ["src/lib/components/WorkItemCard.svelte"],
      worktreePath: "/repo/.worktrees/review-card",
      worktreeLabel: ".worktrees/review-card",
      branch: "feature/review-card",
      prUrl: "https://github.com/phin-tech/roux/pull/90",
    });
  });

  it("uses the latest non-empty run result details", () => {
    const pkg = buildWorkItemReviewPackage(
      workItem(),
      [run()],
      [],
      [
        event({
          id: "event-1",
          payload: {
            summary: "First summary.",
            tests: ["npm run old"],
            changedFiles: ["src/old.ts"],
          },
        }),
        event({
          id: "event-2",
          payload: {
            summary: "Latest summary.",
            tests: ["npm run new"],
            changedFiles: ["src/new.ts"],
          },
        }),
      ],
    );

    expect(pkg.agentSummary).toBe("Latest summary.");
    expect(pkg.tests).toBe("npm run new");
    expect(pkg.changedFiles).toEqual(["src/old.ts", "src/new.ts"]);
  });
});
