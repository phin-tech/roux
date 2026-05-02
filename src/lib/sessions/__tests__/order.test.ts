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

  it("includes projects that have blueprints but no live sessions", () => {
    // Without this, a freshly-created project never renders in the sidebar
    // (no group → no header → no dimmed blueprint rows), so the user can't
    // spawn from it or edit it from the sidebar.
    const projects: Project[] = [
      {
        id: "p-empty",
        name: "Empty",
        sessionBlueprints: [
          {
            id: "bp1",
            name: "shell",
            repoRoot: "/r",
            spawnProfile: "claude",
            nonoAllowDirs: [],
          },
        ],
      },
    ];
    const groups = getGroupedSessions([], projects, "project");
    expect(groups.map((g) => g.key)).toEqual(["p-empty"]);
    expect(groups[0].sessions).toEqual([]);
    expect(groups[0].name).toBe("Empty");
  });

  it("seeds an empty group for every project, even ones with no blueprints", () => {
    // With auto-spawn-on-create, a project may have neither blueprints nor
    // live sessions — sessions were spawned without "save as template" and
    // then closed. Seeding the group keeps the project addressable in the
    // sidebar; the consumer's auto-collapse-on-first-sight handles noise.
    const projects: Project[] = [{ id: "p-nothing", name: "Nothing" }];
    const groups = getGroupedSessions([], projects, "project");
    expect(groups.map((g) => g.key)).toEqual(["p-nothing"]);
    expect(groups[0].sessions).toEqual([]);
  });

  it("sorts a blueprint-only project below groups that have live sessions", () => {
    const projects: Project[] = [
      { id: "p-active", name: "Active" },
      {
        id: "p-empty",
        name: "Empty",
        sessionBlueprints: [
          {
            id: "bp1",
            name: "shell",
            repoRoot: "/r",
            spawnProfile: "claude",
            nonoAllowDirs: [],
          },
        ],
      },
    ];
    const sessions = [makeSession({ id: "a", projectId: "p-active", createdAt: 50 })];
    const groups = getGroupedSessions(sessions, projects, "project");
    expect(groups.map((g) => g.key)).toEqual(["p-active", "p-empty"]);
  });
});

describe("getGroupedSessions (session)", () => {
  it("returns a single flat group sorted by createdAt desc", () => {
    const sessions: Session[] = [
      makeSession({ id: "a", projectId: "p1", createdAt: 10 }),
      makeSession({ id: "b", projectId: null, createdAt: 30 }),
      makeSession({ id: "c", projectId: "p2", createdAt: 20 }),
    ];
    const groups = getGroupedSessions(sessions, [], "session");
    expect(groups).toHaveLength(1);
    expect(groups[0].key).toBe("__all__");
    expect(groups[0].name).toBe("Sessions");
    expect(groups[0].sessions.map((s) => s.id)).toEqual(["b", "c", "a"]);
  });

  it("ignores projects entirely — even projects with blueprints add no groups", () => {
    const projects: Project[] = [
      {
        id: "p-empty",
        name: "Empty",
        sessionBlueprints: [
          {
            id: "bp1",
            name: "shell",
            repoRoot: "/r",
            spawnProfile: "claude",
            nonoAllowDirs: [],
          },
        ],
      },
    ];
    const groups = getGroupedSessions([], projects, "session");
    expect(groups).toEqual([]);
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
