import { describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS, type Project, type Session, type SessionBlueprint } from "$lib/types";
import type { SpawnProfile } from "$lib/panes/profiles";
import {
  buildProjectPromptContext,
  buildProjectPromptPreviewContext,
} from "$lib/projectPromptTemplates";

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: "proj-1",
    name: "Template Vars",
    repoRoots: ["/Users/sam/src/roux"],
    contextPaths: ["/Users/sam/spec.md"],
    sessionBlueprints: [],
    projectPrompt: "",
    ...overrides,
  };
}

function session(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    name: "api",
    repoRoot: "/Users/sam/src/roux",
    worktreePath: "/Users/sam/src/roux-api",
    branch: "feature/api",
    isWorktree: true,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: "proj-1",
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: "s1",
    archived: false,
    endedAt: null,
    blueprintId: "bp-api",
    pinnedPrUrl: null,
    smolMachineName: null,
    ...overrides,
  };
}

function blueprint(overrides: Partial<SessionBlueprint> = {}): SessionBlueprint {
  return {
    id: "bp-api",
    name: "api",
    repoRoot: "/Users/sam/src/roux",
    branch: "feature/api",
    worktreePath: null,
    spawnProfile: "claude",
    base: null,
    fetchFirst: false,
    nonoProfile: null,
    nonoAllowDirs: [],
    ...overrides,
  };
}

function profile(overrides: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id: "claude",
    name: "Claude",
    setupCommand: null,
    startupCommand: "claude",
    startupBehavior: null,
    env: null,
    cwdOverride: null,
    icon: null,
    provider: "claude",
    nonoProfile: null,
    nonoAllowDirs: null,
    source: "builtin",
    ...overrides,
  };
}

describe("project prompt template context", () => {
  it("builds session, model, path, and same-project session variables", () => {
    const ctx = buildProjectPromptContext({
      project: project(),
      session: session(),
      profile: profile(),
      settings: { ...DEFAULT_SETTINGS, defaultModel: "claude-opus-4-6" },
      sessions: [
        session(),
        session({
          id: "s2",
          name: "web",
          worktreePath: "/Users/sam/src/roux-web",
          branch: "feature/web",
          blueprintId: "bp-web",
        }),
        session({
          id: "s3",
          name: "other-project",
          projectId: "proj-2",
        }),
        session({
          id: "s4",
          name: "archived",
          archived: true,
        }),
      ],
    });

    expect(ctx.session.worktree_name).toBe("roux-api");
    expect(ctx.paths.sessions_folder).toBe("/Users/sam/src/roux-api");
    expect(ctx.model).toEqual({ name: "claude-opus-4-6", family: "claude" });
    expect(ctx.other_sessions).toHaveLength(1);
    expect(ctx.other_sessions[0]).toMatchObject({
      id: "s2",
      name: "web",
      branch: "feature/web",
      worktree_name: "roux-web",
    });
  });

  it("builds preview context from the selected blueprint", () => {
    const ctx = buildProjectPromptPreviewContext({
      project: {
        id: "proj-1",
        name: "Template Vars",
        repoRoots: ["/Users/sam/src/roux"],
        contextPaths: [],
      },
      blueprint: blueprint({
        worktreePath: "/Users/sam/src/roux-preview",
      }),
      profile: profile({ id: "codex", name: "Codex", provider: "codex" }),
      settings: { ...DEFAULT_SETTINGS, defaultModel: null },
      sessions: [session({ id: "s2", name: "existing" })],
    });

    expect(ctx.session).toMatchObject({
      id: "preview",
      name: "api",
      worktree_path: "/Users/sam/src/roux-preview",
      worktree_name: "roux-preview",
      branch: "feature/api",
      is_worktree: true,
      blueprint_id: "bp-api",
    });
    expect(ctx.model).toEqual({ name: null, family: "codex" });
    expect(ctx.other_sessions.map((s) => s.id)).toEqual(["s2"]);
  });

  it("treats branch-only blueprint previews as worktree sessions", () => {
    const ctx = buildProjectPromptPreviewContext({
      project: {
        id: "proj-1",
        name: "Template Vars",
        repoRoots: ["/Users/sam/src/roux"],
        contextPaths: [],
      },
      blueprint: blueprint({ worktreePath: null }),
      profile: profile(),
      settings: DEFAULT_SETTINGS,
      sessions: [],
    });

    expect(ctx.session.worktree_path).toBe("/Users/sam/src/roux");
    expect(ctx.session.is_worktree).toBe(true);
  });
});
