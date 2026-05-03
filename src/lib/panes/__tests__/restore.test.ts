import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
  killPty: vi.fn().mockResolvedValue(undefined),
  detachPty: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
}));

import type { Session } from "$lib/bindings";
import { paneInstances, resetInstances } from "../instances";
import { sessionLayouts, resetLayouts } from "../layout";
import { focusedPaneId } from "../focus";
import { restoreSessionPanes } from "../restore";
import type { PaneStatePayload } from "../persistence";

function session(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    name: "Session",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: null,
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: "s1",
    archived: false,
    endedAt: null,
    blueprintId: null,
    ...overrides,
  };
}

describe("restoreSessionPanes", () => {
  const initTerminal = vi.fn();
  const attachPtyListeners = vi.fn().mockResolvedValue(undefined);

  beforeEach(() => {
    resetInstances();
    resetLayouts();
    focusedPaneId.set(null);
    initTerminal.mockClear();
    attachPtyListeners.mockClear();
  });

  it("falls back to a primary Claude pane when no persisted state exists", async () => {
    await restoreSessionPanes(session(), null, { initTerminal, attachPtyListeners });

    expect(get(sessionLayouts).get("s1")).toEqual({
      kind: "leaf",
      paneId: "s1-main",
    });
    expect(get(paneInstances).get("s1-main")?.spawnProfileRef).toEqual({
      kind: "registered",
      id: "claude",
    });
    expect(get(focusedPaneId)).toBe("s1-main");
    expect(initTerminal).toHaveBeenCalledWith("s1-main");
    expect(attachPtyListeners).toHaveBeenCalledWith("s1-main");
  });

  it("restores persisted split panes and reattaches each live PTY", async () => {
    const payload: PaneStatePayload = {
      schemaVersion: 4,
      layout: {
        kind: "split",
        direction: "h",
        sizes: [0.6, 0.4],
        children: [
          { kind: "leaf", paneId: "s1-main" },
          { kind: "leaf", paneId: "shell-pane" },
        ],
      },
      descriptors: [
        {
          id: "s1-main",
          type: "shell",
          ptyId: "s1",
          spawnProfileRef: { kind: "registered", id: "claude" },
          provider: "claude",
        },
        {
          id: "shell-pane",
          type: "shell",
          ptyId: "pty-shell",
          spawnProfileRef: { kind: "registered", id: "plain-shell" },
        },
      ],
    };

    await restoreSessionPanes(session(), payload, { initTerminal, attachPtyListeners });

    expect(get(sessionLayouts).get("s1")).toEqual(payload.layout);
    expect(get(paneInstances).get("s1-main")?.spawnProfileRef).toEqual({
      kind: "registered",
      id: "claude",
    });
    expect(get(paneInstances).get("shell-pane")?.spawnProfileRef).toEqual({
      kind: "registered",
      id: "plain-shell",
    });
    expect(get(focusedPaneId)).toBe("s1-main");
    expect(initTerminal).toHaveBeenCalledWith("s1-main");
    expect(initTerminal).toHaveBeenCalledWith("shell-pane");
    expect(attachPtyListeners).toHaveBeenCalledWith("s1-main");
    expect(attachPtyListeners).toHaveBeenCalledWith("shell-pane");
  });
});
