import { describe, expect, it } from "vitest";
import type { Project, Session } from "$lib/types";
import { getGroupedSessions, getVisualSessionOrder } from "../order";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s",
    name: "s",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 0,
    projectId: null,
    isGitRepo: true,
    ...overrides,
  };
}

describe("getGroupedSessions (repo)", () => {
  it("groups sessions by repoRoot and sorts groups by most recent createdAt desc", () => {
    const sessions: Session[] = [
      makeSession({ id: "a", repoRoot: "/x", createdAt: 10 }),
      makeSession({ id: "b", repoRoot: "/y", createdAt: 20 }),
      makeSession({ id: "c", repoRoot: "/x", createdAt: 30 }),
    ];
    const groups = getGroupedSessions(sessions, [], "repo");
    expect(groups.map((g) => g.key)).toEqual(["/x", "/y"]);
    expect(groups[0].sessions.map((s) => s.id)).toEqual(["a", "c"]);
    expect(groups[1].sessions.map((s) => s.id)).toEqual(["b"]);
  });

  it("uses the repo basename as the group name", () => {
    const sessions = [makeSession({ id: "a", repoRoot: "/foo/bar/my-repo" })];
    const groups = getGroupedSessions(sessions, [], "repo");
    expect(groups[0].name).toBe("my-repo");
  });
});

describe("getGroupedSessions (project)", () => {
  it("groups by projectId, names from projects list, and moves Untagged to the bottom when it is not already the most-recent group", () => {
    const projects: Project[] = [
      { id: "p1", name: "Alpha" },
      { id: "p2", name: "Beta" },
    ];
    const sessions: Session[] = [
      makeSession({ id: "a", projectId: "p1", createdAt: 5 }),
      makeSession({ id: "b", projectId: null, createdAt: 15 }),
      makeSession({ id: "c", projectId: "p2", createdAt: 10 }),
      makeSession({ id: "d", projectId: "p1", createdAt: 40 }),
    ];
    const groups = getGroupedSessions(sessions, projects, "project");
    expect(groups.map((g) => g.key)).toEqual(["p1", "p2", "__untagged__"]);
    expect(groups.map((g) => g.name)).toEqual(["Alpha", "Beta", "Untagged"]);
    expect(groups[0].sessions.map((s) => s.id)).toEqual(["a", "d"]);
    expect(groups[2].sessions.map((s) => s.id)).toEqual(["b"]);
  });

  it("falls back to 'Untagged' when project id does not resolve", () => {
    const sessions = [makeSession({ id: "a", projectId: "missing" })];
    const groups = getGroupedSessions(sessions, [], "project");
    expect(groups[0].name).toBe("Untagged");
  });
});

describe("getVisualSessionOrder", () => {
  it("flattens groups in group order, sessions in in-group order", () => {
    const sessions: Session[] = [
      makeSession({ id: "a", repoRoot: "/x", createdAt: 10 }),
      makeSession({ id: "b", repoRoot: "/y", createdAt: 20 }),
      makeSession({ id: "c", repoRoot: "/x", createdAt: 30 }),
    ];
    const order = getVisualSessionOrder(sessions, [], "repo");
    // Group /x has latest=30, /y has latest=20. /x first. Inside /x: [a, c].
    expect(order.map((s) => s.id)).toEqual(["a", "c", "b"]);
  });

  it("returns an empty array for no sessions", () => {
    expect(getVisualSessionOrder([], [], "repo")).toEqual([]);
    expect(getVisualSessionOrder([], [], "project")).toEqual([]);
  });

  it("places Untagged after named projects in the flat order when a named project is the most-recent group", () => {
    const projects: Project[] = [{ id: "p1", name: "Alpha" }];
    const sessions: Session[] = [
      makeSession({ id: "a", projectId: null, createdAt: 1 }),
      makeSession({ id: "b", projectId: "p1", createdAt: 100 }),
    ];
    const order = getVisualSessionOrder(sessions, projects, "project");
    expect(order.map((s) => s.id)).toEqual(["b", "a"]);
  });
});
