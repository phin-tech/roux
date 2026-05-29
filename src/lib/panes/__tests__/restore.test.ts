import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const spawnShellMock = vi.fn().mockResolvedValue(undefined);

vi.mock("$lib/tauri", () => ({
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
  killPty: vi.fn().mockResolvedValue(undefined),
  detachPty: vi.fn().mockResolvedValue(undefined),
  spawnShell: (...args: unknown[]) => spawnShellMock(...args),
}));

const runProfileInPaneMock = vi.fn().mockResolvedValue(undefined);
vi.mock("$lib/panes/profileRunner", () => ({
  runProfileInPane: (...args: unknown[]) => runProfileInPaneMock(...args),
}));

const resolveProfileRefMock = vi.fn();
vi.mock("$lib/panes/profiles", async () => {
  const actual = await vi.importActual<typeof import("$lib/panes/profiles")>("$lib/panes/profiles");
  return {
    ...actual,
    resolveProfileRef: (...args: unknown[]) => resolveProfileRefMock(...args),
  };
});

vi.mock("$lib/projectPromptTemplates", () => ({
  renderProjectPromptForSession: vi.fn().mockResolvedValue(""),
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
  const attachLivePtyToPane = vi.fn().mockResolvedValue(undefined);

  beforeEach(() => {
    resetInstances();
    resetLayouts();
    focusedPaneId.set(null);
    initTerminal.mockClear();
    attachPtyListeners.mockClear();
    attachLivePtyToPane.mockClear();
    spawnShellMock.mockReset().mockResolvedValue(undefined);
    runProfileInPaneMock.mockReset().mockResolvedValue(undefined);
    resolveProfileRefMock.mockReset().mockReturnValue(null);
  });

  it("falls back to a primary Claude pane when no persisted state exists", async () => {
    await restoreSessionPanes(session(), null, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: new Set(["s1"]),
    });

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

  it("does not attach the fallback primary pane without live PTY evidence", async () => {
    await restoreSessionPanes(session({ status: "disconnected" }), null, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: new Set(),
    });

    expect(get(sessionLayouts).get("s1")).toEqual({
      kind: "leaf",
      paneId: "s1-main",
    });
    expect(get(focusedPaneId)).toBe("s1-main");
    expect(initTerminal).not.toHaveBeenCalled();
    expect(attachPtyListeners).not.toHaveBeenCalled();
  });

  it("restores persisted split panes and reattaches each live PTY", async () => {
    const payload: PaneStatePayload = {
      schemaVersion: 5,
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

    await restoreSessionPanes(session(), payload, {
      initTerminal,
      attachPtyListeners,
      attachLivePtyToPane,
      livePtyIds: new Set(["s1", "pty-shell"]),
    });

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
    expect(attachLivePtyToPane).toHaveBeenCalledWith("s1-main", "s1");
    expect(attachLivePtyToPane).toHaveBeenCalledWith("shell-pane", "pty-shell");
    expect(attachPtyListeners).not.toHaveBeenCalled();
  });

  it("auto-respawns a fresh PTY for non-primary shell panes whose persisted PTY is gone", async () => {
    const payload: PaneStatePayload = {
      schemaVersion: 5,
      layout: {
        kind: "split",
        direction: "h",
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
        },
        {
          id: "shell-pane",
          type: "shell",
          ptyId: "stale-pty",
          workingDir: "/repo/sub",
          spawnProfileRef: { kind: "registered", id: "plain-shell" },
        },
      ],
    };

    await restoreSessionPanes(session(), payload, {
      initTerminal,
      attachPtyListeners,
      attachLivePtyToPane,
      livePtyIds: new Set(["s1"]),
    });

    expect(spawnShellMock).toHaveBeenCalledTimes(1);
    const [freshPtyId, workingDir, sessionId, paneId] =
      spawnShellMock.mock.calls[0];
    expect(typeof freshPtyId).toBe("string");
    expect(freshPtyId).not.toBe("stale-pty");
    expect(workingDir).toBe("/repo/sub");
    expect(sessionId).toBe("s1");
    expect(paneId).toBe("shell-pane");

    const respawned = get(paneInstances).get("shell-pane");
    expect(respawned?.ptyId).toBe(freshPtyId);
    expect(respawned?.restoreError).toBeUndefined();
    expect(initTerminal).toHaveBeenCalledWith("shell-pane");
    expect(attachPtyListeners).toHaveBeenCalledWith("shell-pane");
  });

  it("replays the spawn profile after auto-respawn so agents come back live", async () => {
    const profile = {
      id: "claude",
      name: "Claude",
      startupCommand: "claude",
    };
    resolveProfileRefMock.mockReturnValue(profile);

    const payload: PaneStatePayload = {
      schemaVersion: 5,
      layout: {
        kind: "split",
        direction: "h",
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
        },
        {
          id: "shell-pane",
          type: "shell",
          ptyId: "stale-pty",
          workingDir: "/repo",
          spawnProfileRef: { kind: "registered", id: "claude" },
        },
      ],
    };

    await restoreSessionPanes(session(), payload, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: new Set(["s1"]),
    });

    expect(runProfileInPaneMock).toHaveBeenCalledTimes(1);
    const [ptyIdArg, profileArg] = runProfileInPaneMock.mock.calls[0];
    const respawned = get(paneInstances).get("shell-pane");
    expect(ptyIdArg).toBe(respawned?.ptyId);
    expect(profileArg).toBe(profile);
  });

  it("marks the pane retryable when auto-respawn fails", async () => {
    spawnShellMock.mockRejectedValueOnce(new Error("No such file or directory"));

    const payload: PaneStatePayload = {
      schemaVersion: 5,
      layout: {
        kind: "split",
        direction: "h",
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
        },
        {
          id: "shell-pane",
          type: "shell",
          ptyId: "stale-pty",
          workingDir: "/missing",
          spawnProfileRef: { kind: "registered", id: "plain-shell" },
        },
      ],
    };

    await restoreSessionPanes(session(), payload, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: new Set(["s1"]),
    });

    const dead = get(paneInstances).get("shell-pane");
    expect(dead?.ptyId).toBe("");
    expect(dead?.restoreError).toContain("No such file or directory");
    expect(initTerminal).not.toHaveBeenCalledWith("shell-pane");
    expect(attachPtyListeners).not.toHaveBeenCalledWith("shell-pane");
    expect(runProfileInPaneMock).not.toHaveBeenCalled();
  });

  it("does not auto-respawn when live PTY inventory is unknown", async () => {
    const payload: PaneStatePayload = {
      schemaVersion: 5,
      layout: {
        kind: "split",
        direction: "h",
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
        },
        {
          id: "shell-pane",
          type: "shell",
          ptyId: "maybe-live",
          workingDir: "/repo",
          spawnProfileRef: { kind: "registered", id: "plain-shell" },
        },
      ],
    };

    await restoreSessionPanes(session(), payload, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: null,
    });

    expect(spawnShellMock).not.toHaveBeenCalled();
    expect(runProfileInPaneMock).not.toHaveBeenCalled();
    expect(get(paneInstances).get("shell-pane")?.ptyId).toBe("maybe-live");
  });

  it("restores panes but does not attach PTYs when live inventory is unknown", async () => {
    const payload: PaneStatePayload = {
      schemaVersion: 5,
      layout: {
        kind: "split",
        direction: "h",
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
        },
        {
          id: "shell-pane",
          type: "shell",
          ptyId: "maybe-live-pty",
          spawnProfileRef: { kind: "registered", id: "plain-shell" },
        },
      ],
    };

    await restoreSessionPanes(session({ status: "disconnected" }), payload, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: null,
    });

    expect(get(paneInstances).get("shell-pane")?.restoreError).toBeUndefined();
    expect(get(focusedPaneId)).toBe("s1-main");
    expect(initTerminal).not.toHaveBeenCalled();
    expect(attachPtyListeners).not.toHaveBeenCalled();
  });

  it("strips known-stale command panes from restored layouts", async () => {
    const payload: PaneStatePayload = {
      schemaVersion: 5,
      layout: {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "s1-main" },
          { kind: "leaf", paneId: "cmd-pane" },
        ],
      },
      descriptors: [
        {
          id: "s1-main",
          type: "shell",
          ptyId: "s1",
          spawnProfileRef: { kind: "registered", id: "claude" },
        },
        {
          id: "cmd-pane",
          type: "command",
          ptyId: "stale-command-pty",
          command: "npm test",
        },
      ],
    };

    await restoreSessionPanes(session({ status: "disconnected" }), payload, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: new Set(["s1"]),
    });

    expect(get(sessionLayouts).get("s1")).toEqual({
      kind: "leaf",
      paneId: "s1-main",
    });
    expect(get(paneInstances).has("cmd-pane")).toBe(false);
    expect(initTerminal).toHaveBeenCalledWith("s1-main");
    expect(initTerminal).not.toHaveBeenCalledWith("cmd-pane");
    expect(attachPtyListeners).toHaveBeenCalledWith("s1-main");
    expect(attachPtyListeners).not.toHaveBeenCalledWith("cmd-pane");
  });

  it("falls back to a primary pane when persisted state has no session primary descriptor", async () => {
    const payload: PaneStatePayload = {
      schemaVersion: 5,
      layout: {
        kind: "leaf",
        paneId: "stale-shell",
      },
      descriptors: [
        {
          id: "stale-shell",
          type: "shell",
          ptyId: "stale-pty",
          spawnProfileRef: { kind: "registered", id: "plain-shell" },
        },
      ],
    };

    await restoreSessionPanes(session({ status: "disconnected" }), payload, {
      initTerminal,
      attachPtyListeners,
      livePtyIds: new Set(),
    });

    expect(get(sessionLayouts).get("s1")).toEqual({
      kind: "leaf",
      paneId: "s1-main",
    });
    expect(get(paneInstances).get("s1-main")?.ptyId).toBe("s1");
    expect(get(paneInstances).has("stale-shell")).toBe(false);
    expect(get(focusedPaneId)).toBe("s1-main");
    expect(initTerminal).not.toHaveBeenCalled();
    expect(attachPtyListeners).not.toHaveBeenCalled();
  });
});
