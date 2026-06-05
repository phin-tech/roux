import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri", () => ({
  notificationsPush: vi.fn(),
  notificationsRemove: vi.fn().mockResolvedValue(true),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));

import {
  initWorkItemNotifications,
  stopWorkItemNotifications,
} from "../workItemNotifications";
import {
  workItems,
  workItemRuns,
  workItemDecisions,
} from "$lib/stores/workItems";
import type { WorkItem } from "$lib/bindings";
import type { WorkItemDecision, WorkItemRun } from "$lib/types/workItems";
import { sessionState } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";
import { DEFAULT_SETTINGS } from "$lib/types";
import { notificationsPush, notificationsRemove } from "$lib/tauri";

function waitTick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function makeRun(overrides: Partial<WorkItemRun> = {}): WorkItemRun {
  return {
    id: "run-1",
    workItemId: "item-1",
    kind: "implementation",
    sessionId: "sess-1",
    ptyId: null,
    provider: null,
    profileId: null,
    status: "blocked",
    worktreePath: null,
    branch: null,
    cost: null,
    createdAt: 0,
    startedAt: null,
    endedAt: null,
    updatedAt: 0,
    ...overrides,
  };
}

function makeDecision(overrides: Partial<WorkItemDecision> = {}): WorkItemDecision {
  return {
    id: "dec-1",
    runId: "run-1",
    question: "Which approach?",
    options: [],
    defaultValue: null,
    timeoutAt: null,
    status: "pending",
    resolvedValue: null,
    resolvedBy: null,
    createdAt: 0,
    resolvedAt: null,
    updatedAt: 0,
    ...overrides,
  };
}

function makeItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "item-1",
    title: "Build the thing",
    status: "doing",
    ...overrides,
  } as WorkItem;
}

describe("workItemNotifications", () => {
  beforeEach(() => {
    workItems.set([makeItem()]);
    workItemRuns.set([]);
    workItemDecisions.set([]);
    sessionState.set({ sessions: [], activeSessionId: null });
    settings.set({ ...DEFAULT_SETTINGS });
    vi.mocked(notificationsPush).mockReset();
    vi.mocked(notificationsPush).mockImplementation((req) =>
      Promise.resolve({
        id: `notif:${req.dedupKey}`,
        createdAt: 0,
        read: false,
        ...req,
      }),
    );
    vi.mocked(notificationsRemove).mockClear();
    initWorkItemNotifications();
  });

  afterEach(() => {
    stopWorkItemNotifications();
  });

  it("fires an attention notification for a new pending decision", async () => {
    workItemRuns.set([makeRun()]);
    workItemDecisions.set([makeDecision()]);
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const req = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(req.level).toBe("attention");
    expect(req.source).toEqual({ type: "internal" });
    expect(req.body).toBe("Which approach?");
    expect(req.sessionId).toBe("sess-1");
    expect(req.dedupKey).toBe("work-item-decision:dec-1");
    expect(req.actions[0].kind).toEqual({
      type: "focusSession",
      sessionId: "sess-1",
    });
  });

  it("does not re-fire when the same decision re-emits", async () => {
    workItemRuns.set([makeRun()]);
    workItemDecisions.set([makeDecision()]);
    await waitTick();
    // Unrelated store churn re-runs the derived store with the same decision.
    workItemRuns.set([makeRun({ updatedAt: 1 })]);
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
  });

  it("removes the notification when the decision resolves", async () => {
    workItemRuns.set([makeRun()]);
    workItemDecisions.set([makeDecision()]);
    await waitTick();

    workItemDecisions.set([makeDecision({ status: "resolved" })]);
    await waitTick();

    expect(notificationsRemove).toHaveBeenCalledWith("notif:work-item-decision:dec-1");
  });

  it("suppresses the notification when the bound session is active", async () => {
    sessionState.set({ sessions: [], activeSessionId: "sess-1" });
    workItemRuns.set([makeRun()]);
    workItemDecisions.set([makeDecision()]);
    await waitTick();

    expect(notificationsPush).not.toHaveBeenCalled();
  });

  it("fires a suppressed pending decision after leaving the bound session", async () => {
    sessionState.set({ sessions: [], activeSessionId: "sess-1" });
    workItemRuns.set([makeRun()]);
    workItemDecisions.set([makeDecision()]);
    await waitTick();
    expect(notificationsPush).not.toHaveBeenCalled();

    sessionState.set({ sessions: [], activeSessionId: null });
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    expect(vi.mocked(notificationsPush).mock.calls[0][0].dedupKey).toBe(
      "work-item-decision:dec-1",
    );
  });

  it("does not fire when notifications are disabled", async () => {
    settings.set({ ...DEFAULT_SETTINGS, notificationsEnabled: false });
    workItemRuns.set([makeRun()]);
    workItemDecisions.set([makeDecision()]);
    await waitTick();

    expect(notificationsPush).not.toHaveBeenCalled();
  });

  it("fires distinct notifications for two blocked items", async () => {
    workItems.set([makeItem(), makeItem({ id: "item-2" })]);
    workItemRuns.set([
      makeRun(),
      makeRun({ id: "run-2", workItemId: "item-2", sessionId: "sess-2" }),
    ]);
    workItemDecisions.set([
      makeDecision(),
      makeDecision({ id: "dec-2", runId: "run-2", question: "Other?" }),
    ]);
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(2);
    const keys = vi
      .mocked(notificationsPush)
      .mock.calls.map((c) => c[0].dedupKey);
    expect(new Set(keys)).toEqual(
      new Set(["work-item-decision:dec-1", "work-item-decision:dec-2"]),
    );
  });

  it("fires with a null session and a dismiss-only action when the run has no session", async () => {
    workItemRuns.set([makeRun({ sessionId: null })]);
    workItemDecisions.set([makeDecision()]);
    await waitTick();

    expect(notificationsPush).toHaveBeenCalledTimes(1);
    const req = vi.mocked(notificationsPush).mock.calls[0][0];
    expect(req.sessionId).toBeNull();
    expect(req.actions).toHaveLength(1);
    expect(req.actions[0].kind).toEqual({ type: "dismiss" });
  });
});
