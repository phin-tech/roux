import { describe, expect, it } from "vitest";

import { buildMultiLineEditorContextChips } from "../multiLineEditorContext";
import type { PaneInstance } from "../instances";
import type { Session, WorktrunkMetadata } from "$lib/types";

function makePane(overrides: Partial<PaneInstance> = {}): PaneInstance {
  return {
    id: "pane-1",
    type: "shell",
    ptyId: "pty-1",
    unlisteners: [],
    workingDir: "/Users/sam/src/roux",
    ...overrides,
  };
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "session-1",
    name: "roux",
    repoRoot: "/Users/sam/src/roux",
    worktreePath: "/Users/sam/src/roux",
    branch: "feature/context-chips",
    isWorktree: true,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 1,
    isGitRepo: true,
    ...overrides,
  };
}

function makeMetadata(overrides: Partial<WorktrunkMetadata> = {}): WorktrunkMetadata {
  return {
    dirty: false,
    ahead: 0,
    behind: 0,
    locked: false,
    lockReason: null,
    prunable: false,
    prunableReason: null,
    isCurrent: false,
    isPrevious: false,
    devServerUrl: null,
    mainState: null,
    ciStatus: null,
    ciUrl: null,
    ciStale: false,
    ...overrides,
  };
}

describe("buildMultiLineEditorContextChips", () => {
  it("shows target, cwd, branch, and profile context", () => {
    expect(
      buildMultiLineEditorContextChips({
        pane: makePane(),
        session: makeSession(),
        target: "shell",
        metadata: null,
        profileName: "Plain shell",
      }),
    ).toEqual([
      expect.objectContaining({ kind: "target", label: "shell" }),
      expect.objectContaining({ kind: "cwd", label: "roux" }),
      expect.objectContaining({ kind: "branch", label: "feature/context-chips" }),
      expect.objectContaining({ kind: "profile", label: "Plain shell" }),
    ]);
  });

  it("uses the pane cwd before the session worktree path", () => {
    const chips = buildMultiLineEditorContextChips({
      pane: makePane({ workingDir: "/Users/sam/src/roux/src-tauri" }),
      session: makeSession({ worktreePath: "/Users/sam/src/roux" }),
      target: "claude",
      metadata: null,
      profileName: null,
    });

    expect(chips.find((chip) => chip.kind === "cwd")?.label).toBe("src-tauri");
    expect(chips.find((chip) => chip.kind === "target")?.label).toBe("claude");
  });

  it("falls back to the session worktree when the pane has no cwd", () => {
    const chips = buildMultiLineEditorContextChips({
      pane: makePane({ workingDir: undefined }),
      session: makeSession({ worktreePath: "/tmp/redpen" }),
      target: "shell",
      metadata: null,
      profileName: null,
    });

    expect(chips.find((chip) => chip.kind === "cwd")?.label).toBe("redpen");
  });

  it("adds compact git state when worktrunk metadata is present", () => {
    const chips = buildMultiLineEditorContextChips({
      pane: makePane(),
      session: makeSession(),
      target: "shell",
      metadata: makeMetadata({ dirty: true, ahead: 2, behind: 1, locked: true }),
      profileName: null,
    });

    expect(chips.find((chip) => chip.kind === "git-state")).toMatchObject({
      label: "dirty ↑2 ↓1 locked",
      tone: "warn",
    });
  });

  it("omits branch and git-state chips for non-git sessions", () => {
    const chips = buildMultiLineEditorContextChips({
      pane: makePane(),
      session: makeSession({ isGitRepo: false, branch: "" }),
      target: "shell",
      metadata: makeMetadata({ dirty: true, ahead: 1 }),
      profileName: null,
    });

    expect(chips.some((chip) => chip.kind === "branch")).toBe(false);
    expect(chips.some((chip) => chip.kind === "git-state")).toBe(false);
  });
});
