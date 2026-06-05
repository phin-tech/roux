import { describe, expect, it } from "vitest";
import { resolveReviewAgentRepoRoot } from "../reviewAgent";

describe("resolveReviewAgentRepoRoot", () => {
  it("uses an explicit item repo path first", () => {
    expect(
      resolveReviewAgentRepoRoot({
        itemRepoPath: "/repo/main",
        projectRepoRoots: ["/repo/other"],
        worktreePath: "/repo/other/.worktrees/card",
      }),
    ).toBe("/repo/main");
  });

  it("selects the project root that owns a child worktree path", () => {
    expect(
      resolveReviewAgentRepoRoot({
        itemRepoPath: null,
        projectRepoRoots: ["/repo/api", "/repo/web"],
        worktreePath: "/repo/web/.worktrees/card",
      }),
    ).toBe("/repo/web");
  });

  it("selects the longest matching project root for sibling card worktrees", () => {
    expect(
      resolveReviewAgentRepoRoot({
        itemRepoPath: null,
        projectRepoRoots: ["/repo", "/repo/web"],
        worktreePath: "/repo/web.roux-card-abc",
      }),
    ).toBe("/repo/web");
  });

  it("returns null when no project root owns the worktree", () => {
    expect(
      resolveReviewAgentRepoRoot({
        itemRepoPath: null,
        projectRepoRoots: ["/repo/api"],
        worktreePath: "/repo/web/.worktrees/card",
      }),
    ).toBeNull();
  });
});
