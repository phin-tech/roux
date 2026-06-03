import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri", () => ({
  notificationsPush: vi.fn().mockResolvedValue(undefined),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));

import {
  initAgentNotifications,
  stopAgentNotifications,
  forgetLastStatus,
} from "../agentNotifications";
import { updateAgentState, resetAgentStates } from "../agentState";
import { resetInstances, createPane } from "../instances";
import { resetProfileRegistry } from "../profiles";
import { resetLayouts, sessionLayouts } from "../layout";
import { focusedPaneId, resetFocus } from "../focus";
import { sessionState } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";
import { DEFAULT_SETTINGS } from "$lib/types";
import { notificationsPush } from "$lib/tauri";

function waitTick(): Promise<void> {
  // notificationsPush fires via a Promise.then chain inside the subscriber.
  // Flush the microtask queue so the push call resolves before assertions.
  return new Promise((resolve) => setTimeout(resolve, 0));
}

describe("agentNotifications", () => {
  beforeEach(() => {
    resetInstances();
    resetAgentStates();
    resetProfileRegistry();
    resetLayouts();
    resetFocus();
    sessionState.set({ sessions: [], activeSessionId: null });
    settings.set({ ...DEFAULT_SETTINGS });
    vi.mocked(notificationsPush).mockClear();
    initAgentNotifications();
  });

  afterEach(() => {
    stopAgentNotifications();
  });

  it("does not fire when a pane's first state is idle", async () => {
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();
    expect(notificationsPush).not.toHaveBeenCalled();
  });

  it("does not fire for generating → generating ticks", async () => {
    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    await waitTick();
    expect(notificationsPush).not.toHaveBeenCalled();
  });

  it("fires exactly once on a generating → idle transition", async () => {
    createPane({
      id: "pane-1",
      type: "shell",
      ptyId: "pty-1",
      name: "Work pane",
    });

    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const request = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(request.title).toContain("Work pane");
    expect(request.level).toBe("success");
    expect(request.source).toEqual({ type: "hook", provider: "claude" });
    expect(request.actions[0].kind).toEqual({
      type: "focusPane",
      paneId: "pane-1",
    });
  });

  it("does not fire completion for blocked → idle", async () => {
    updateAgentState("pane-1", {
      provider: "claude",
      status: "blocked",
      permissionInfo: { toolName: "Edit" },
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).not.toHaveBeenCalled();
  });

  it("fires a deduped error notification when an agent enters error", async () => {
    createPane({
      id: "pane-1",
      type: "shell",
      ptyId: "pty-1",
      name: "Work pane",
    });

    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "error",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "error",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const request = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(request.level).toBe("error");
    expect(request.source).toEqual({ type: "hook", provider: "claude" });
    expect(request.title).toContain("Work pane");
    expect(request.actions[0].kind).toEqual({
      type: "focusPane",
      paneId: "pane-1",
    });
    expect(request.dedupKey).toBe("error:pane:pane-1");
  });

  it("includes Claude Stop transcript summaries when present", async () => {
    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      completionSummary: {
        query: "update the notifications",
        response: "notifications now include summaries",
      },
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const request = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(request.body).toBe(
      "Prompt: update the notifications\nResponse: notifications now include summaries",
    );
  });

  it("fires once per pane when no session layout owns either pane", async () => {
    // Without sessionLayouts, both panes fall back to per-pane dedup keys.
    updateAgentState("pane-a", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-b", {
      provider: "codex",
      status: "generating",
      source: "hook",
    });

    updateAgentState("pane-a", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    updateAgentState("pane-b", {
      provider: "codex",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(2);
    const titles = vi
      .mocked(notificationsPush)
      .mock.calls.map(([req]) => req.title);
    // Default title is provider-based when no pane name/profile is set.
    expect(titles).toContain("Claude finished");
    expect(titles).toContain("Codex finished");
    const dedupKeys = vi
      .mocked(notificationsPush)
      .mock.calls.map(([req]) => req.dedupKey);
    expect(dedupKeys).toContain("completion:pane:pane-a");
    expect(dedupKeys).toContain("completion:pane:pane-b");
  });

  it("collapses two panes in the same session into one completion:session dedup key", async () => {
    sessionLayouts.set(
      new Map([
        [
          "s1",
          {
            kind: "split",
            direction: "h",
            children: [
              { kind: "leaf", paneId: "pane-a" },
              { kind: "leaf", paneId: "pane-b" },
            ],
          },
        ],
      ]),
    );

    updateAgentState("pane-a", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-b", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-a", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    updateAgentState("pane-b", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(2);
    const requests = vi
      .mocked(notificationsPush)
      .mock.calls.map(([req]) => req);
    expect(requests.every((req) => req.sessionId === "s1")).toBe(true);
    expect(
      requests.every((req) => req.dedupKey === "completion:session:s1"),
    ).toBe(true);
  });

  it("does not fire when the setting is disabled", async () => {
    settings.set({
      ...DEFAULT_SETTINGS,
      agentCompletionNotificationsEnabled: false,
    });

    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).not.toHaveBeenCalled();
  });

  it("still fires error notifications when completion notifications are disabled", async () => {
    settings.set({
      ...DEFAULT_SETTINGS,
      agentCompletionNotificationsEnabled: false,
    });
    createPane({
      id: "pane-1",
      type: "shell",
      ptyId: "pty-1",
      name: "Work pane",
    });

    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "error",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const request = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(request.level).toBe("error");
  });

  it("does not fire when the pane is currently visible (active session + focused)", async () => {
    sessionLayouts.set(new Map([["s1", { kind: "leaf", paneId: "pane-1" }]]));
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    focusedPaneId.set("pane-1");

    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).not.toHaveBeenCalled();
  });

  it("fires when the pane is in a non-active session", async () => {
    sessionLayouts.set(new Map([["s1", { kind: "leaf", paneId: "pane-1" }]]));
    sessionState.set({ sessions: [], activeSessionId: "s2" });
    focusedPaneId.set("pane-1");

    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const request = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(request.sessionId).toBe("s1");
    expect(request.dedupKey).toBe("completion:session:s1");
  });

  it("fires when the pane is in the active session but not focused", async () => {
    sessionLayouts.set(
      new Map([
        [
          "s1",
          {
            kind: "split",
            direction: "h",
            children: [
              { kind: "leaf", paneId: "pane-1" },
              { kind: "leaf", paneId: "pane-2" },
            ],
          },
        ],
      ]),
    );
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    focusedPaneId.set("pane-2");

    updateAgentState("pane-1", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-1", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const request = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(request.sessionId).toBe("s1");
    expect(request.dedupKey).toBe("completion:session:s1");
  });

  it("forgetLastStatus lets a reused pane id re-fire the first transition", async () => {
    updateAgentState("pane-reuse", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-reuse", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();
    expect(notificationsPush).toHaveBeenCalledTimes(1);

    // Simulate pane disposal + a brand-new pane taking the same id.
    forgetLastStatus("pane-reuse");

    updateAgentState("pane-reuse", {
      provider: "claude",
      status: "generating",
      source: "hook",
    });
    updateAgentState("pane-reuse", {
      provider: "claude",
      status: "idle",
      source: "hook",
    });
    await waitTick();
    expect(notificationsPush).toHaveBeenCalledTimes(2);
  });
});
