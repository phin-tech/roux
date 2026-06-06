import { describe, expect, it } from "vitest";
import type { WorkItem } from "$lib/bindings";
import { splitArchivedWorkItems, workItemDetailKeyAction } from "../archive";

function item(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "wi-1",
    projectId: null,
    parentId: null,
    title: "Card",
    body: null,
    status: "todo",
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
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("work item archive core", () => {
  it("splits active and archived cards without changing order", () => {
    const active = item({ id: "active", sortOrder: 1 });
    const archived = item({ id: "archived", sortOrder: 2, archivedAt: 10 });

    expect(splitArchivedWorkItems([active, archived])).toEqual({
      active: [active],
      archived: [archived],
    });
  });

  it("maps Escape to close for card detail", () => {
    expect(workItemDetailKeyAction({ key: "Escape" })).toBe("close");
    expect(workItemDetailKeyAction({ key: "Enter" })).toBe("none");
  });
});
