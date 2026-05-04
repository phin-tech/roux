import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

let nextBranchByCall: Array<string | null> = [];
let nextRejectByCall: Array<Error | null> = [];
const refreshCalls: string[] = [];
let resolveNow = true;

vi.mock("$lib/tauri", () => ({
  refreshSessionBranch: vi.fn(async (sessionId: string) => {
    refreshCalls.push(sessionId);
    while (!resolveNow) {
      await new Promise((r) => setTimeout(r, 1));
    }
    const reject = nextRejectByCall.shift() ?? null;
    if (reject) throw reject;
    return nextBranchByCall.shift() ?? null;
  }),
}));

import {
  _resetSessionBranchPollerForTests,
  installSessionBranchPoller,
  refreshAllSessionBranches,
} from "../sessionBranchPoller";
import { sessionList, sessionState } from "../sessions";

describe("sessionBranchPoller", () => {
  beforeEach(() => {
    refreshCalls.length = 0;
    nextBranchByCall = [];
    nextRejectByCall = [];
    resolveNow = true;
    _resetSessionBranchPollerForTests();
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  afterEach(() => {
    _resetSessionBranchPollerForTests();
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("updates session branch in the store when the backend returns a new value", async () => {
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s1",
          repoRoot: "/repo",
          branch: "main",
          isGitRepo: true,
          archived: false,
        } as any,
      ],
      activeSessionId: "s1",
    });
    nextBranchByCall = ["feature/x"];

    await refreshAllSessionBranches();

    expect(refreshCalls).toEqual(["s1"]);
    const updated = get(sessionList).find((s) => s.id === "s1");
    expect(updated?.branch).toBe("feature/x");
  });

  it("leaves the branch unchanged when the backend returns the same value", async () => {
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s1",
          repoRoot: "/repo",
          branch: "main",
          isGitRepo: true,
          archived: false,
        } as any,
      ],
      activeSessionId: "s1",
    });
    nextBranchByCall = ["main"];

    const before = get(sessionList).find((s) => s.id === "s1");
    await refreshAllSessionBranches();
    const after = get(sessionList).find((s) => s.id === "s1");

    expect(after?.branch).toBe("main");
    // Same reference is fine here — the assertion that matters is the
    // value didn't change, not whether we re-allocated the entry.
    expect(after?.branch).toBe(before?.branch);
  });

  it("skips archived and non-git-repo sessions", async () => {
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "active",
          repoRoot: "/repo",
          branch: "main",
          isGitRepo: true,
          archived: false,
        } as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "archived",
          repoRoot: "/repo",
          branch: "main",
          isGitRepo: true,
          archived: true,
        } as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "non-git",
          repoRoot: "/repo",
          branch: "",
          isGitRepo: false,
          archived: false,
        } as any,
      ],
      activeSessionId: "active",
    });

    await refreshAllSessionBranches();

    expect(refreshCalls).toEqual(["active"]);
  });

  it("inFlight guard prevents overlapping ticks", async () => {
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s1",
          repoRoot: "/repo",
          branch: "main",
          isGitRepo: true,
          archived: false,
        } as any,
      ],
      activeSessionId: "s1",
    });

    // Hold the first tick mid-await. The second call should be a no-op
    // because of the `inFlight` guard.
    resolveNow = false;
    nextBranchByCall = ["feature/x"];

    const p1 = refreshAllSessionBranches();
    const p2 = refreshAllSessionBranches();

    // Yield so p1 enters the in-flight state, then release.
    await new Promise((r) => setTimeout(r, 5));
    resolveNow = true;
    await Promise.all([p1, p2]);

    expect(refreshCalls).toEqual(["s1"]);
  });

  it("a per-session error doesn't poison the rest of the tick", async () => {
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "boom",
          repoRoot: "/repo",
          branch: "main",
          isGitRepo: true,
          archived: false,
        } as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "ok",
          repoRoot: "/repo",
          branch: "main",
          isGitRepo: true,
          archived: false,
        } as any,
      ],
      activeSessionId: "boom",
    });
    nextRejectByCall = [new Error("git missing")];
    nextBranchByCall = ["feature/y"];

    await refreshAllSessionBranches();

    expect(refreshCalls).toEqual(["boom", "ok"]);
    const ok = get(sessionList).find((s) => s.id === "ok");
    expect(ok?.branch).toBe("feature/y");
  });

  it("installSessionBranchPoller returns a stop fn that cancels the timer", async () => {
    const stop = installSessionBranchPoller(50);
    // First tick fires immediately; wait long enough that a second would
    // fire if we didn't stop.
    await new Promise((r) => setTimeout(r, 25));
    stop();
    const callsAtStop = refreshCalls.length;
    await new Promise((r) => setTimeout(r, 120));
    expect(refreshCalls.length).toBe(callsAtStop);
  });
});
