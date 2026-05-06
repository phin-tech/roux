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
    createPane({ id: "pane-1", type: "shell", ptyId: "pty-1", name: "Work pane" });

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
    expect(request.actions[0].kind).toEqual({ type: "focusPane", paneId: "pane-1" });
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
    createPane({ id: "pane-1", type: "shell", ptyId: "pty-1", name: "Work pane" });

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
    expect(request.actions[0].kind).toEqual({ type: "focusPane", paneId: "pane-1" });
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

  it("fires once per pane — two panes finishing in the same session produces two notifications", async () => {
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
    const titles = vi.mocked(notificationsPush).mock.calls.map(([req]) => req.title);
    // Default title is provider-based when no pane name/profile is set.
    expect(titles).toContain("Claude is idle");
    expect(titles).toContain("Codex is idle");
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
