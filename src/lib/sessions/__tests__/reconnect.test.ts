import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  reconnectSessionShellPty: vi.fn(),
  killSession: vi.fn(),
  killPty: vi.fn().mockResolvedValue(undefined),
  spawnShell: vi.fn(),
  writeToSession: vi.fn().mockResolvedValue(undefined),
  loadPaneStateRaw: vi.fn().mockResolvedValue(null),
  savePaneStateRaw: vi.fn().mockResolvedValue(undefined),
  saveLivePaneStateRaw: vi.fn().mockResolvedValue(undefined),
  deletePaneStateRaw: vi.fn().mockResolvedValue(undefined),
  createPtyOutputChannel: vi.fn((_cb: unknown) => "mock-channel"),
  attachPtyOutput: vi.fn().mockResolvedValue(undefined),
  onSessionExit: vi.fn().mockResolvedValue(() => {}),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/panes/terminals", () => {
  const initTerminal = vi.fn();
  const attachPtyListeners = vi.fn().mockResolvedValue(undefined);
  return {
    initTerminal,
    attachPtyListeners,
    connectPaneTerminal: vi.fn(async (paneId: string, onExit?: unknown) => {
      initTerminal(paneId);
      return attachPtyListeners(paneId, onExit);
    }),
  };
});

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));

import { continueSession, reconnectSession, retryShellPane } from "../reconnect";
import { sessionState, addSession } from "$lib/stores/sessions";
import { initSession } from "$lib/panes/actions";
import { sessionLayouts, resetLayouts } from "$lib/panes/layout";
import { paneInstances, resetInstances, createPane, updateInstance } from "$lib/panes/instances";
import { resetFocus } from "$lib/panes/focus";
import { reconnectSessionShellPty, spawnShell, loadPaneStateRaw, writeToSession } from "$lib/tauri";
import { initTerminal, attachPtyListeners, connectPaneTerminal } from "$lib/panes/terminals";
import { resetProfileRegistry, setUserProfiles, type SpawnProfile } from "$lib/panes/profiles";
import type { Session } from "$lib/types";
import type { PaneStatePayload } from "$lib/panes/persistence";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "sess-1",
    name: "Repo",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "disconnected",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: null,
    isGitRepo: true,
    ...overrides,
  };
}

function makePayloadWithShells(sessionId: string, shells: Array<{ id: string; workingDir: string }>): PaneStatePayload {
  const mainId = `${sessionId}-main`;
  const children = [
    { kind: "leaf" as const, paneId: mainId },
    ...shells.map((s) => ({ kind: "leaf" as const, paneId: s.id })),
  ];
  return {
    schemaVersion: 4,
    layout: { kind: "split", direction: "h" as const, children },
    descriptors: [
      // The session-primary pane is a shell whose ptyId matches the
      // session id — that is how the session-owned PTY is keyed on the
      // Rust side. reconnectPrimaryPaneOnly finds it by that match.
      { id: mainId, type: "shell" as const, ptyId: sessionId },
      ...shells.map((s) => ({ id: s.id, type: "shell" as const, ptyId: "old-pty", workingDir: s.workingDir })),
    ],
  };
}

function makeProfile(overrides: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id: "claude",
    name: "Claude",
    source: "user",
    provider: "claude",
    startupCommand: "claude",
    setupCommand: undefined,
    startupBehavior: "autoRun",
    cwdOverride: undefined,
    env: {},
    icon: null,
    nonoProfile: null,
    nonoAllowDirs: [],
    ...overrides,
  };
}

describe("reconnectSession — existing behavior preserved", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    resetLayouts();
    resetInstances();
    resetFocus();
    resetProfileRegistry();
    vi.mocked(reconnectSessionShellPty).mockReset().mockResolvedValue(makeSession({ status: "idle" }));
    vi.mocked(loadPaneStateRaw).mockReset().mockResolvedValue(null);
    vi.mocked(spawnShell).mockReset().mockResolvedValue(undefined);
    vi.mocked(writeToSession).mockReset().mockResolvedValue(undefined);
    vi.mocked(initTerminal).mockReset();
    vi.mocked(attachPtyListeners).mockReset().mockResolvedValue(undefined);
    vi.mocked(connectPaneTerminal).mockReset().mockImplementation(async (paneId, onExit) => {
      vi.mocked(initTerminal)(paneId);
      return vi.mocked(attachPtyListeners)(paneId, onExit);
    });
  });

  it("reconnects the main pane when no persisted state exists", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    await reconnectSession(session);

    expect(reconnectSessionShellPty).toHaveBeenCalledWith(session.id, null, null, null);
  });

  it("invokes reconnect with extra flags without error", async () => {
    // Extra flags are now appended to the profile startup command and
    // typed into the shell rather than passed to the PTY-level command,
    // so the Tauri call shape no longer varies with flags.
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    await reconnectSession(session, ["--resume", "abc123"]);

    expect(reconnectSessionShellPty).toHaveBeenCalledWith(session.id, null, null, null);
  });

  it("continues a Claude primary profile with claude --continue", async () => {
    setUserProfiles([makeProfile()]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    await continueSession(session);

    expect(writeToSession).toHaveBeenCalledWith(session.id, "claude --continue");
    expect(writeToSession).toHaveBeenCalledWith(session.id, "\r");
  });

  it("continues a Claude primary profile by exact provider session id when available", async () => {
    setUserProfiles([makeProfile()]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      provider: "claude",
      providerSessionId: "claude-session-123",
    });

    await continueSession(session);

    expect(writeToSession).toHaveBeenCalledWith(
      session.id,
      "claude --resume claude-session-123",
    );
    expect(writeToSession).toHaveBeenCalledWith(session.id, "\r");
  });

  it("falls back to Claude continue when provider session id contains shell metacharacters", async () => {
    setUserProfiles([makeProfile()]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      provider: "claude",
      providerSessionId: "session 'quoted'; $(touch bad)",
    });

    await continueSession(session);

    // Cross-shell safety: anything outside SAFE_SHELL_ARG drops to the
    // generic continue path instead of attempting to quote — POSIX
    // single-quoting is wrong on PowerShell/cmd.
    expect(writeToSession).toHaveBeenCalledWith(session.id, "claude --continue");
    expect(writeToSession).not.toHaveBeenCalledWith(
      session.id,
      expect.stringContaining("--resume"),
    );
  });

  it("falls back to Claude continue when provider session id contains control characters", async () => {
    setUserProfiles([makeProfile()]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      provider: "claude",
      providerSessionId: "session-1\n--dangerous",
    });

    await continueSession(session);

    expect(writeToSession).toHaveBeenCalledWith(session.id, "claude --continue");
  });

  it("does not retry the profile replay when typing exact resume fails (avoid half-typed line)", async () => {
    setUserProfiles([makeProfile()]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      provider: "claude",
      providerSessionId: "claude-session-123",
    });
    vi.mocked(writeToSession)
      .mockRejectedValueOnce(new Error("dead pty during exact resume"))
      .mockResolvedValue(undefined);

    await continueSession(session);

    // Only the original attempt is made. We don't auto-fall-back to
    // `--continue` because runProfileInPane writes the command and the
    // Enter as separate PTY writes — a partial failure could leave a
    // half-typed line, and a retry would compound the mess.
    expect(vi.mocked(writeToSession).mock.calls.map(([, data]) => data)).toEqual([
      "claude --resume claude-session-123",
    ]);
  });

  it("continues a Codex primary profile with codex resume --last", async () => {
    setUserProfiles([
      makeProfile({
        id: "codex",
        name: "Codex",
        provider: "codex",
        startupCommand: "codex",
      }),
    ]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      spawnProfileRef: { kind: "registered", id: "codex" },
    });

    await continueSession(session);

    expect(writeToSession).toHaveBeenCalledWith(session.id, "codex resume --last");
    expect(writeToSession).toHaveBeenCalledWith(session.id, "\r");
  });

  it("continues a Codex primary profile by exact provider session id when available", async () => {
    setUserProfiles([
      makeProfile({
        id: "codex",
        name: "Codex",
        provider: "codex",
        startupCommand: "codex",
      }),
    ]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      spawnProfileRef: { kind: "registered", id: "codex" },
      provider: "codex",
      providerSessionId: "codex-session-123",
    });

    await continueSession(session);

    expect(writeToSession).toHaveBeenCalledWith(
      session.id,
      "codex resume codex-session-123",
    );
    expect(writeToSession).toHaveBeenCalledWith(session.id, "\r");
  });

  it("falls back to Codex resume --last when provider session id has spaces", async () => {
    setUserProfiles([
      makeProfile({
        id: "codex",
        name: "Codex",
        provider: "codex",
        startupCommand: "codex",
      }),
    ]);
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      spawnProfileRef: { kind: "registered", id: "codex" },
      provider: "codex",
      providerSessionId: "thread name with spaces",
    });

    await continueSession(session);

    // Cross-shell safety: spaces don't match SAFE_SHELL_ARG, so the
    // exact-resume path is dropped in favor of `resume --last`.
    expect(writeToSession).toHaveBeenCalledWith(session.id, "codex resume --last");
    expect(writeToSession).not.toHaveBeenCalledWith(
      session.id,
      expect.stringContaining("'thread"),
    );
  });

  it("forwards the pane profile id when reconnecting the primary shell", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);
    updateInstance(`${session.id}-main`, {
      spawnProfileRef: {
        kind: "inline",
        profile: {
          id: "custom-inline",
          name: "Custom Inline",
          source: "user",
          provider: null,
          icon: null,
          startupCommand: "echo hi",
          setupCommand: null,
          startupBehavior: "autoRun",
          cwdOverride: null,
          env: {},
          nonoProfile: null,
          nonoAllowDirs: [],
        },
      },
    });

    await reconnectSession(session);

    expect(reconnectSessionShellPty).toHaveBeenCalledWith(
      session.id,
      null,
      null,
      "custom-inline",
    );
  });

  it("preserves the layout tree when reconnecting main-pane-only", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    await reconnectSession(session);

    const afterTree = get(sessionLayouts).get(session.id);
    expect(afterTree).toBeDefined();
    const state = get(sessionState);
    expect(state.sessions.find((s) => s.id === session.id)?.status).toBe("idle");
  });

  it("double-click reconnect — second call throws already-reconnecting error", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    // Start first reconnect but don't await it yet
    const first = reconnectSession(session);
    await expect(reconnectSession(session)).rejects.toThrow("already in progress");
    await first;
  });
});

describe("reconnectSession — mid-session disconnect guard", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    resetLayouts();
    resetInstances();
    resetFocus();
    resetProfileRegistry();
    vi.mocked(reconnectSessionShellPty).mockReset().mockResolvedValue(makeSession({ status: "idle" }));
    vi.mocked(loadPaneStateRaw).mockReset().mockResolvedValue(null);
    vi.mocked(spawnShell).mockReset().mockResolvedValue(undefined);
    vi.mocked(initTerminal).mockReset();
    vi.mocked(attachPtyListeners).mockReset().mockResolvedValue(undefined);
  });

  it("skips rehydration if current layout already has splits", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    // Manually add a split to simulate active session layout
    const shellPaneId = createPane({ type: "shell", ptyId: "pty-live" });
    sessionLayouts.update((m) => {
      const next = new Map(m);
      next.set(session.id, {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: `${session.id}-main` },
          { kind: "leaf", paneId: shellPaneId },
        ],
      });
      return next;
    });

    // Even if persisted state has shells, should NOT rehydrate
    vi.mocked(loadPaneStateRaw).mockResolvedValue(
      makePayloadWithShells(session.id, [{ id: "shell-saved", workingDir: "/repo" }])
    );

    await reconnectSession(session);

    expect(spawnShell).not.toHaveBeenCalled();
    // Layout should be unchanged (still has the live shell)
    const tree = get(sessionLayouts).get(session.id);
    expect(tree?.kind).toBe("split");
  });
});

describe("reconnectSession — full rehydration", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    resetLayouts();
    resetInstances();
    resetFocus();
    resetProfileRegistry();
    vi.mocked(reconnectSessionShellPty).mockReset().mockResolvedValue(makeSession({ status: "idle" }));
    vi.mocked(loadPaneStateRaw).mockReset();
    vi.mocked(spawnShell).mockReset().mockResolvedValue(undefined);
    vi.mocked(initTerminal).mockReset();
    vi.mocked(attachPtyListeners).mockReset().mockResolvedValue(undefined);
  });

  it("fast-path when persisted state is primary-only leaf", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    vi.mocked(loadPaneStateRaw).mockResolvedValue({
      schemaVersion: 4,
      layout: { kind: "leaf", paneId: `${session.id}-main` },
      descriptors: [{ id: `${session.id}-main`, type: "shell", ptyId: session.id }],
    } satisfies PaneStatePayload);

    await reconnectSession(session);

    expect(spawnShell).not.toHaveBeenCalled();
    expect(connectPaneTerminal).toHaveBeenCalledWith(`${session.id}-main`);
  });

  it("spawns shells for each shell descriptor and creates pane instances", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    vi.mocked(loadPaneStateRaw).mockResolvedValue(
      makePayloadWithShells(session.id, [
        { id: "shell-a", workingDir: "/repo/a" },
        { id: "shell-b", workingDir: "/repo/b" },
      ])
    );

    await reconnectSession(session);

    expect(spawnShell).toHaveBeenCalledTimes(2);
    expect(spawnShell).toHaveBeenCalledWith(expect.any(String), "/repo/a", session.id, "shell-a", null, null, null);
    expect(spawnShell).toHaveBeenCalledWith(expect.any(String), "/repo/b", session.id, "shell-b", null, null, null);

    const instances = get(paneInstances);
    expect(instances.has("shell-a")).toBe(true);
    expect(instances.has("shell-b")).toBe(true);
    expect(instances.get("shell-a")?.type).toBe("shell");
    expect(instances.get("shell-b")?.restoreError).toBeUndefined();
  });

  it("applies the restored layout tree to sessionLayouts", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    vi.mocked(loadPaneStateRaw).mockResolvedValue(
      makePayloadWithShells(session.id, [{ id: "shell-a", workingDir: "/repo" }])
    );

    await reconnectSession(session);

    const tree = get(sessionLayouts).get(session.id);
    expect(tree?.kind).toBe("split");
    if (tree?.kind === "split") {
      const leafIds = tree.children.map((c) => (c.kind === "leaf" ? c.paneId : null));
      expect(leafIds).toContain("shell-a");
      expect(leafIds).toContain(`${session.id}-main`);
    }
  });

  it("routes restored panes through connectPaneTerminal for terminal wiring", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    vi.mocked(loadPaneStateRaw).mockResolvedValue(
      makePayloadWithShells(session.id, [
        { id: "shell-a", workingDir: "/repo" },
        { id: "shell-b", workingDir: "/repo" },
      ])
    );

    const callOrder: string[] = [];
    vi.mocked(initTerminal).mockImplementation((id) => { callOrder.push(`init:${id}`); });
    vi.mocked(attachPtyListeners).mockImplementation(async (id) => { callOrder.push(`attach:${id}`); });

    await reconnectSession(session);

    // For each pane: init must come before attach
    const idxInitA = callOrder.indexOf("init:shell-a");
    const idxAttachA = callOrder.indexOf("attach:shell-a");
    const idxInitB = callOrder.indexOf("init:shell-b");
    const idxAttachB = callOrder.indexOf("attach:shell-b");
    expect(idxInitA).toBeLessThan(idxAttachA);
    expect(idxInitB).toBeLessThan(idxAttachB);
  });

  it("creates a dead pane with restoreError when shell spawn fails", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    vi.mocked(loadPaneStateRaw).mockResolvedValue(
      makePayloadWithShells(session.id, [{ id: "shell-dead", workingDir: "/gone" }])
    );
    vi.mocked(spawnShell).mockRejectedValue(new Error("No such file or directory"));

    await reconnectSession(session);

    const instances = get(paneInstances);
    const deadPane = instances.get("shell-dead");
    expect(deadPane).toBeDefined();
    expect(deadPane?.restoreError).toContain("No such file or directory");
  });

  it("still applies the layout tree even if one shell fails to spawn", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    vi.mocked(loadPaneStateRaw).mockResolvedValue(
      makePayloadWithShells(session.id, [{ id: "shell-dead", workingDir: "/gone" }])
    );
    vi.mocked(spawnShell).mockRejectedValue(new Error("gone"));

    await reconnectSession(session);

    const tree = get(sessionLayouts).get(session.id);
    expect(tree?.kind).toBe("split");
  });

  it("preserves spawnProfileRef on restored shell panes", async () => {
    // Regression: phase 5 added spawnProfileRef persistence on save but
    // rehydratePane dropped the field on restore, so the re-run button
    // and provider-specific UI went dark after restart for every pane
    // that wasn't the session-primary.
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const mainId = `${session.id}-main`;
    vi.mocked(loadPaneStateRaw).mockResolvedValue({
      schemaVersion: 4,
      layout: {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: mainId },
          { kind: "leaf", paneId: "codex-pane" },
        ],
      },
      descriptors: [
        { id: mainId, type: "shell", ptyId: session.id },
        {
          id: "codex-pane",
          type: "shell",
          ptyId: "old-pty",
          workingDir: "/repo",
          spawnProfileRef: { kind: "registered", id: "codex" },
        },
      ],
    } satisfies PaneStatePayload);

    await reconnectSession(session);

    const codexInstance = get(paneInstances).get("codex-pane");
    expect(codexInstance?.spawnProfileRef).toEqual({
      kind: "registered",
      id: "codex",
    });
  });

  it("preserves provider session metadata on restored non-primary shell panes", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const mainId = `${session.id}-main`;
    vi.mocked(loadPaneStateRaw).mockResolvedValue({
      schemaVersion: 4,
      layout: {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: mainId },
          { kind: "leaf", paneId: "codex-pane" },
        ],
      },
      descriptors: [
        { id: mainId, type: "shell", ptyId: session.id },
        {
          id: "codex-pane",
          type: "shell",
          ptyId: "old-pty",
          workingDir: "/repo",
          spawnProfileRef: { kind: "registered", id: "codex" },
          provider: "codex",
          providerSessionId: "codex-session-123",
        },
      ],
    } satisfies PaneStatePayload);

    await reconnectSession(session);

    const codexInstance = get(paneInstances).get("codex-pane");
    expect(codexInstance?.provider).toBe("codex");
    expect(codexInstance?.providerSessionId).toBe("codex-session-123");
  });

  it("restores mixed shell, notes, and markdown panes with exact layout metadata", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const mainId = `${session.id}-main`;
    const restoredLayout = {
      kind: "split",
      direction: "h",
      sizes: [0.62, 0.38],
      children: [
        { kind: "leaf", paneId: mainId },
        {
          kind: "split",
          direction: "v",
          stacked: true,
          activeIndex: 1,
          sizes: [0.25, 0.35, 0.4],
          children: [
            { kind: "leaf", paneId: "notes-pane" },
            { kind: "leaf", paneId: "doc-pane" },
            { kind: "leaf", paneId: "shell-pane" },
          ],
        },
      ],
    } satisfies PaneStatePayload["layout"];

    vi.mocked(loadPaneStateRaw).mockResolvedValue({
      schemaVersion: 4,
      layout: restoredLayout,
      descriptors: [
        { id: mainId, type: "shell", ptyId: session.id },
        {
          id: "notes-pane",
          type: "notes",
          ptyId: "",
          name: "Session notes",
          notesScope: "repo",
          notesViewMode: "read",
        },
        {
          id: "doc-pane",
          type: "markdown",
          ptyId: "",
          name: "Plan",
          docPath: "/repo/PLAN.md",
        },
        {
          id: "shell-pane",
          type: "shell",
          ptyId: "old-shell",
          name: "server",
          workingDir: "/repo/app",
          spawnProfileRef: { kind: "registered", id: "plain-shell" },
        },
      ],
    } satisfies PaneStatePayload);

    await reconnectSession(session);

    expect(get(sessionLayouts).get(session.id)).toEqual(restoredLayout);
    const instances = get(paneInstances);
    expect(instances.get("notes-pane")).toMatchObject({
      type: "notes",
      name: "Session notes",
      notesScope: "repo",
      notesViewMode: "read",
    });
    expect(instances.get("doc-pane")).toMatchObject({
      type: "markdown",
      name: "Plan",
      docPath: "/repo/PLAN.md",
    });
    expect(instances.get("shell-pane")).toMatchObject({
      type: "shell",
      name: "server",
      workingDir: "/repo/app",
      spawnProfileRef: { kind: "registered", id: "plain-shell" },
    });
    expect(spawnShell).toHaveBeenCalledTimes(1);
    expect(spawnShell).toHaveBeenCalledWith(
      expect.any(String),
      "/repo/app",
      session.id,
      "shell-pane",
      null,
      null,
      "plain-shell",
    );
  });

  it("strips command panes from the persisted tree before rehydration", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const mainId = `${session.id}-main`;
    vi.mocked(loadPaneStateRaw).mockResolvedValue({
      schemaVersion: 4,
      layout: {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: mainId },
          { kind: "leaf", paneId: "cmd-pane" },
        ],
      },
      descriptors: [
        { id: mainId, type: "shell", ptyId: session.id },
        { id: "cmd-pane", type: "command", ptyId: "pty-cmd", command: "npm test" },
      ],
    } satisfies PaneStatePayload);

    await reconnectSession(session);

    // Command pane should NOT have been spawned or created
    const instances = get(paneInstances);
    expect(instances.has("cmd-pane")).toBe(false);
    expect(spawnShell).not.toHaveBeenCalled();
  });

  it("falls back to primary-pane-only when persisted payload fails integrity check", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const mainId = `${session.id}-main`;
    // Corrupt: leaf in tree with no matching descriptor
    vi.mocked(loadPaneStateRaw).mockResolvedValue({
      schemaVersion: 4,
      layout: {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: mainId },
          { kind: "leaf", paneId: "orphan-pane" },
        ],
      },
      descriptors: [
        { id: mainId, type: "shell", ptyId: session.id },
        // "orphan-pane" is missing from descriptors
      ],
    } satisfies PaneStatePayload);

    await reconnectSession(session);

    // Should fall back — no shell spawned, no orphan instance
    expect(spawnShell).not.toHaveBeenCalled();
    const instances = get(paneInstances);
    expect(instances.has("orphan-pane")).toBe(false);
  });

  it("falls back to primary-pane-only when persisted layout contains an invalid child", async () => {
    const session = makeSession();
    addSession(session);
    initSession(session.id);

    const mainId = `${session.id}-main`;
    vi.mocked(loadPaneStateRaw).mockResolvedValue({
      schemaVersion: 4,
      layout: {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: mainId },
          undefined,
        ],
      },
      descriptors: [
        { id: mainId, type: "shell", ptyId: session.id },
      ],
    } as unknown as PaneStatePayload);

    await reconnectSession(session);

    expect(get(sessionLayouts).get(session.id)).toEqual({ kind: "leaf", paneId: mainId });
    expect(spawnShell).not.toHaveBeenCalled();
  });
});

describe("retryShellPane", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    resetLayouts();
    resetInstances();
    resetFocus();
    resetProfileRegistry();
    vi.mocked(spawnShell).mockReset().mockResolvedValue(undefined);
    vi.mocked(initTerminal).mockReset();
    vi.mocked(attachPtyListeners).mockReset().mockResolvedValue(undefined);
  });

  it("spawns a fresh shell and clears restoreError on success", async () => {
    const paneId = createPane({
      type: "shell",
      ptyId: "",
      workingDir: "/repo",
    });
    // Mark as dead
    const { updateInstance } = await import("$lib/panes/instances");
    updateInstance(paneId, { restoreError: "old error" });

    await retryShellPane(paneId, "sess-1");

    expect(spawnShell).toHaveBeenCalledWith(expect.any(String), "/repo", "sess-1", paneId, null, null, null);
    const inst = get(paneInstances).get(paneId);
    expect(inst?.restoreError).toBeUndefined();
  });

  it("sets restoreError again when retry also fails", async () => {
    const paneId = createPane({ type: "shell", ptyId: "", workingDir: "/still-gone" });
    const { updateInstance } = await import("$lib/panes/instances");
    updateInstance(paneId, { restoreError: "old error" });

    vi.mocked(spawnShell).mockRejectedValue(new Error("still gone"));
    await retryShellPane(paneId, "sess-1");

    const inst = get(paneInstances).get(paneId);
    expect(inst?.restoreError).toContain("still gone");
  });

  it("is a no-op for panes without restoreError", async () => {
    const paneId = createPane({ type: "shell", ptyId: "live-pty", workingDir: "/repo" });

    await retryShellPane(paneId, "sess-1");

    expect(spawnShell).not.toHaveBeenCalled();
  });
});
