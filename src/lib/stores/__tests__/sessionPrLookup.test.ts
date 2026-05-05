import { describe, it, expect, beforeEach, vi } from "vitest";
import { get } from "svelte/store";

import type { PrInfo } from "$lib/tauri";

const lookupCalls: Array<[string, string]> = [];
const pinnedLookupCalls: Array<[string | null, string]> = [];
let nextLookupResult: PrInfo | null = null;
let nextLookupReject: Error | null = null;
let resolveLookupNow = true;

const findOrCreateCalls: unknown[] = [];

vi.mock("$lib/tauri", () => ({
  lookupPrForBranch: vi.fn(async (repoPath: string, branch: string) => {
    lookupCalls.push([repoPath, branch]);
    // Polling gate so the dedupe-under-concurrency test can hold the
    // first call open while the second one tries to start.
    while (!resolveLookupNow) {
      await new Promise((r) => setTimeout(r, 1));
    }
    if (nextLookupReject) throw nextLookupReject;
    return nextLookupResult;
  }),
  lookupPr: vi.fn(async (repoPath: string | null, url: string) => {
    pinnedLookupCalls.push([repoPath, url]);
    if (nextLookupReject) throw nextLookupReject;
    if (!nextLookupResult) throw new Error("PR not found");
    return nextLookupResult;
  }),
  findOrCreateWatch: vi.fn(async (config: unknown) => {
    findOrCreateCalls.push(config);
    return { id: "stub", config };
  }),
}));

import {
  _resetSessionPrLookupForTests,
  getPrLookupSnapshot,
  installSessionPrEffect,
  lookupPrForSession,
  prLookupFor,
  prLookupErrorFor,
  prLookupForSession,
  projectPrRowsFor,
  refreshActiveSessionPr,
} from "../sessionPrLookup";
import { sessionState } from "../sessions";
import { settings } from "../settings";
import { DEFAULT_SETTINGS } from "$lib/types";

function makePr(overrides: Partial<PrInfo> = {}): PrInfo {
  return {
    number: 42,
    title: "Test PR",
    headRef: "feature/x",
    headOwner: "phin-tech",
    isCrossRepository: false,
    url: "https://github.com/phin-tech/roux/pull/42",
    repoSlug: "phin-tech/roux",
    checks: null,
    checkRuns: [],
    reviewDecision: null,
    ...overrides,
  };
}

const session = {
  repoRoot: "/tmp/repo",
  branch: "feature/x",
  isGitRepo: true,
};

describe("sessionPrLookup", () => {
  beforeEach(() => {
    lookupCalls.length = 0;
    pinnedLookupCalls.length = 0;
    findOrCreateCalls.length = 0;
    nextLookupResult = null;
    nextLookupReject = null;
    resolveLookupNow = true;
    _resetSessionPrLookupForTests();
    sessionState.set({ sessions: [], activeSessionId: null });
    settings.set(DEFAULT_SETTINGS);
  });

  it("returns null without calling backend for non-git or empty branch", async () => {
    expect(await lookupPrForSession({ ...session, isGitRepo: false })).toBeNull();
    expect(await lookupPrForSession({ ...session, branch: "" })).toBeNull();
    expect(await lookupPrForSession({ ...session, repoRoot: "" })).toBeNull();
    expect(lookupCalls).toEqual([]);
  });

  it("calls the backend on first lookup and caches the result", async () => {
    nextLookupResult = makePr();
    const r1 = await lookupPrForSession(session);
    expect(r1).toEqual(makePr());
    expect(lookupCalls).toEqual([["/tmp/repo", "feature/x"]]);

    // Same call within TTL → no backend hit
    const r2 = await lookupPrForSession(session);
    expect(r2).toEqual(makePr());
    expect(lookupCalls).toHaveLength(1);
  });

  it("caches negative results too (no PR for this branch)", async () => {
    nextLookupResult = null;
    const r1 = await lookupPrForSession(session);
    expect(r1).toBeNull();
    expect(lookupCalls).toHaveLength(1);

    const r2 = await lookupPrForSession(session);
    expect(r2).toBeNull();
    expect(lookupCalls).toHaveLength(1);
  });

  it("force=true bypasses the cache and re-hits the backend", async () => {
    nextLookupResult = makePr();
    await lookupPrForSession(session);
    expect(lookupCalls).toHaveLength(1);

    nextLookupResult = makePr({ number: 99 });
    const refreshed = await lookupPrForSession(session, { force: true });
    expect(refreshed?.number).toBe(99);
    expect(lookupCalls).toHaveLength(2);
  });

  it("dedupes concurrent calls for the same (repoRoot, branch)", async () => {
    // Hold the first call open until both have started.
    resolveLookupNow = false;
    nextLookupResult = makePr();

    const p1 = lookupPrForSession(session);
    const p2 = lookupPrForSession(session);

    // Yield so both calls start; then release the gate.
    await new Promise((r) => setTimeout(r, 5));
    resolveLookupNow = true;

    await Promise.all([p1, p2]);
    expect(lookupCalls).toHaveLength(1);
  });

  it("clears in-flight on backend error so the next call retries", async () => {
    nextLookupReject = new Error("gh missing");
    const r1 = await lookupPrForSession(session);
    expect(r1).toBeNull();
    expect(lookupCalls).toHaveLength(1);

    nextLookupReject = null;
    nextLookupResult = makePr();
    const r2 = await lookupPrForSession(session);
    expect(r2).toEqual(makePr());
    expect(lookupCalls).toHaveLength(2);
  });

  it("getPrLookupSnapshot reflects cached value", async () => {
    expect(getPrLookupSnapshot(session.repoRoot, session.branch)).toBeUndefined();

    nextLookupResult = makePr();
    await lookupPrForSession(session);

    expect(getPrLookupSnapshot(session.repoRoot, session.branch)).toEqual(makePr());
  });

  it("prLookupFor derived store yields undefined → PrInfo as the lookup runs", async () => {
    const store = prLookupFor(session.repoRoot, session.branch);
    expect(get(store)).toBeUndefined();

    nextLookupResult = makePr();
    await lookupPrForSession(session);
    expect(get(store)).toEqual(makePr());
  });

  it("installSessionPrEffect skips lookup when autoLookupSessionPr is false", async () => {
    settings.set({ ...DEFAULT_SETTINGS, autoLookupSessionPr: false });
    const dispose = installSessionPrEffect();

    sessionState.set({
      sessions: [
        {
          id: "s1",
          repoRoot: session.repoRoot,
          branch: session.branch,
          isGitRepo: true,
          // Other Session fields not exercised by the effect.
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        } as any,
      ],
      activeSessionId: "s1",
    });
    await new Promise((r) => setTimeout(r, 5));

    expect(lookupCalls).toEqual([]);
    dispose();
  });

  it("installSessionPrEffect creates an auto-watch only when autoWatchSessionPr is true", async () => {
    settings.set({
      ...DEFAULT_SETTINGS,
      autoLookupSessionPr: true,
      autoWatchSessionPr: false,
    });
    nextLookupResult = makePr();
    const dispose = installSessionPrEffect();

    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s1",
          repoRoot: session.repoRoot,
          branch: session.branch,
          isGitRepo: true,
        } as any,
      ],
      activeSessionId: "s1",
    });
    await new Promise((r) => setTimeout(r, 10));

    expect(lookupCalls).toHaveLength(1);
    expect(findOrCreateCalls).toEqual([]);
    dispose();
  });

  it("installSessionPrEffect re-runs lookup when branch changes for the same session id", async () => {
    settings.set({
      ...DEFAULT_SETTINGS,
      autoLookupSessionPr: true,
      autoWatchSessionPr: false,
    });
    nextLookupResult = makePr();
    const dispose = installSessionPrEffect();

    const baseSession = {
      id: "s1",
      repoRoot: session.repoRoot,
      branch: "feature/x",
      isGitRepo: true,
    };
    sessionState.set({
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      sessions: [baseSession as any],
      activeSessionId: "s1",
    });
    await new Promise((r) => setTimeout(r, 10));
    expect(lookupCalls).toEqual([["/tmp/repo", "feature/x"]]);

    // Same session id, different branch — should trigger a fresh lookup.
    sessionState.set({
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      sessions: [{ ...baseSession, branch: "feature/y" } as any],
      activeSessionId: "s1",
    });
    await new Promise((r) => setTimeout(r, 10));
    expect(lookupCalls).toEqual([
      ["/tmp/repo", "feature/x"],
      ["/tmp/repo", "feature/y"],
    ]);
    dispose();
  });

  it("pinnedPrUrl uses lookupPr and skips branch-based lookup", async () => {
    nextLookupResult = makePr({ number: 7, url: "https://github.com/o/r/pull/7" });
    const pinnedSession = {
      ...session,
      pinnedPrUrl: "https://github.com/o/r/pull/7",
    };

    const result = await lookupPrForSession(pinnedSession);
    expect(result?.number).toBe(7);
    expect(pinnedLookupCalls).toEqual([["/tmp/repo", "https://github.com/o/r/pull/7"]]);
    expect(lookupCalls).toEqual([]);
  });

  it("prLookupForSession returns the pinned PR ahead of branch-based lookup", async () => {
    nextLookupResult = makePr({ number: 7 });
    const pinnedSession = {
      repoRoot: "/tmp/repo",
      branch: "feature/x",
      pinnedPrUrl: "https://github.com/o/r/pull/7",
    };
    const reactive = prLookupForSession(pinnedSession);
    expect(get(reactive)).toBeUndefined();

    await lookupPrForSession({ ...pinnedSession, isGitRepo: true });
    expect(get(reactive)?.number).toBe(7);
  });

  it("populates lastError on failure and clears it on a subsequent success", async () => {
    nextLookupReject = new Error("gh: not authenticated");
    await lookupPrForSession(session);

    const errStore = prLookupErrorFor(session);
    expect(get(errStore)).toContain("gh: not authenticated");

    nextLookupReject = null;
    nextLookupResult = makePr();
    await lookupPrForSession(session, { force: true });
    expect(get(errStore)).toBeNull();
  });

  it("refreshActiveSessionPr force-refreshes the active session's PR", async () => {
    nextLookupResult = makePr({ number: 1 });
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s1",
          repoRoot: session.repoRoot,
          branch: session.branch,
          isGitRepo: true,
        } as any,
      ],
      activeSessionId: "s1",
    });
    await lookupPrForSession(session);
    expect(lookupCalls).toHaveLength(1);

    nextLookupResult = makePr({ number: 2 });
    const refreshed = await refreshActiveSessionPr();
    expect(refreshed?.number).toBe(2);
    expect(lookupCalls).toHaveLength(2);
  });

  it("projectPrRowsFor lists open PRs for sessions in a project", async () => {
    nextLookupResult = makePr({ number: 11 });
    await lookupPrForSession({
      repoRoot: "/tmp/repo",
      branch: "feature/x",
      isGitRepo: true,
      pinnedPrUrl: null,
    });

    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s-in",
          repoRoot: "/tmp/repo",
          branch: "feature/x",
          isGitRepo: true,
          projectId: "proj-1",
          archived: false,
        } as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s-out",
          repoRoot: "/tmp/repo",
          branch: "feature/y",
          isGitRepo: true,
          projectId: "proj-2",
          archived: false,
        } as any,
      ],
      activeSessionId: "s-in",
    });

    const rows = get(projectPrRowsFor("proj-1"));
    expect(rows).toHaveLength(1);
    expect(rows[0].session.id).toBe("s-in");
    expect(rows[0].prInfo.number).toBe(11);
    expect(rows[0].pinned).toBe(false);

    expect(get(projectPrRowsFor("proj-2"))).toHaveLength(0);
  });

  it("auto-watch is deduped per (sessionId, repo, prNumber) within a run", async () => {
    settings.set({
      ...DEFAULT_SETTINGS,
      autoLookupSessionPr: true,
      autoWatchSessionPr: true,
    });
    nextLookupResult = makePr();
    const dispose = installSessionPrEffect();

    // Activate the same session twice (no-op on the second activation).
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s1",
          repoRoot: session.repoRoot,
          branch: session.branch,
          isGitRepo: true,
        } as any,
      ],
      activeSessionId: "s1",
    });
    await new Promise((r) => setTimeout(r, 10));
    sessionState.set({
      sessions: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {
          id: "s1",
          repoRoot: session.repoRoot,
          branch: session.branch,
          isGitRepo: true,
        } as any,
      ],
      activeSessionId: "s1",
    });
    await new Promise((r) => setTimeout(r, 10));

    expect(findOrCreateCalls).toHaveLength(1);
    dispose();
  });
});
