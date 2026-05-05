import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/svelte";
import { tick } from "svelte";
import type { Session, Worktree, WorktrunkMetadata } from "$lib/types";
import type { PrInfo } from "$lib/tauri";

const tauriMock = vi.hoisted(() => ({
  nextPrLookupResult: null as unknown,
}));

vi.mock("$lib/tauri", () => ({
  listWorktrees: vi.fn(),
  lookupPr: vi.fn(async () => tauriMock.nextPrLookupResult),
  lookupPrForBranch: vi.fn(async () => tauriMock.nextPrLookupResult),
  findOrCreateWatch: vi.fn(),
}));

import StatusBar from "../StatusBar.svelte";
import { sessionState } from "$lib/stores/sessions";
import {
  _resetSessionPrLookupForTests,
  lookupPrForSession,
} from "$lib/stores/sessionPrLookup";
import {
  _resetWorktreeMetadataForTests,
  upsertWorktreeMetadata,
} from "$lib/stores/worktreeMetadata";
import {
  closePrStatusDetails,
  togglePrStatusDetails,
} from "$lib/stores/prStatusDetails";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    name: "sess",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: Date.now() / 1000,
    projectId: null,
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: "s1",
    archived: false,
    endedAt: null,
    ...overrides,
  };
}

function makeMeta(overrides: Partial<WorktrunkMetadata> = {}): WorktrunkMetadata {
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
    reviewDetails: [],
    reviewDecision: null,
    ...overrides,
  };
}

function seed(path: string, meta: WorktrunkMetadata | null) {
  const wt: Worktree = {
    path,
    branch: "feat",
    isMain: false,
    worktrunk: meta,
  };
  upsertWorktreeMetadata([wt]);
}

describe("StatusBar worktrunk integration", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _resetSessionPrLookupForTests();
    tauriMock.nextPrLookupResult = null;
    closePrStatusDetails();
  });

  afterEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _resetSessionPrLookupForTests();
    tauriMock.nextPrLookupResult = null;
    closePrStatusDetails();
  });

  it("renders no PR link when no session is active", () => {
    const { queryByTestId } = render(StatusBar);
    expect(queryByTestId("status-bar-pr-link")).toBeNull();
  });

  it("renders a PR link when the active session's worktree has ciUrl", () => {
    const s = makeSession({ worktreePath: "/wt/feat-a" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-a",
      makeMeta({
        ciStatus: "passed",
        ciUrl: "https://github.com/org/repo/pull/42",
      }),
    );
    const { getByTestId } = render(StatusBar);
    const a = getByTestId("status-bar-pr-link") as HTMLAnchorElement;
    expect(a.textContent).toContain("PR #42");
    expect(a.querySelector("svg")).not.toBeNull();
    expect(a.getAttribute("href")).toBe(
      "https://github.com/org/repo/pull/42",
    );
    expect(a.className).toContain("text-green");
  });

  it("renders a red PR link on failed CI", () => {
    const s = makeSession({ worktreePath: "/wt/feat-b" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-b",
      makeMeta({
        ciStatus: "failed",
        ciUrl: "https://github.com/org/repo/pull/101",
      }),
    );
    const { getByTestId } = render(StatusBar);
    const a = getByTestId("status-bar-pr-link");
    expect(a.className).toContain("text-red");
    expect(a.querySelector("svg")).not.toBeNull();
    expect(a.textContent).toContain("PR #101");
  });

  it("extracts MR number for gitlab-style merge_requests URLs", () => {
    const s = makeSession({ worktreePath: "/wt/feat-gl" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-gl",
      makeMeta({
        ciStatus: "running",
        ciUrl: "https://gitlab.com/org/repo/-/merge_requests/7",
      }),
    );
    const { getByTestId } = render(StatusBar);
    const a = getByTestId("status-bar-pr-link");
    expect(a.textContent).toContain("PR #7");
  });

  it("falls back to a non-link ci chip when ciStatus is set but ciUrl is null", () => {
    const s = makeSession({ worktreePath: "/wt/feat-c" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-c",
      makeMeta({ ciStatus: "running", ciUrl: null }),
    );
    const { getByTestId, queryByTestId } = render(StatusBar);
    expect(queryByTestId("status-bar-pr-link")).toBeNull();
    const chip = getByTestId("status-bar-ci-chip");
    expect(chip.querySelector("svg")).not.toBeNull();
    expect(chip.textContent).toContain("ci");
    expect(chip.className).toContain("text-yellow");
  });

  it("dims the link with opacity-60 when CI is stale", () => {
    const s = makeSession({ worktreePath: "/wt/feat-d" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-d",
      makeMeta({
        ciStatus: "passed",
        ciUrl: "https://github.com/org/repo/pull/1",
        ciStale: true,
      }),
    );
    const { getByTestId } = render(StatusBar);
    const a = getByTestId("status-bar-pr-link");
    expect(a.className).toContain("opacity-60");
    expect(a.getAttribute("title")).toContain("stale");
  });

  it("renders nothing when the active session has no worktrunk metadata", () => {
    const s = makeSession({ worktreePath: "/wt/unknown" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    const { queryByTestId } = render(StatusBar);
    expect(queryByTestId("status-bar-pr-link")).toBeNull();
    expect(queryByTestId("status-bar-ci-chip")).toBeNull();
  });

  it("hides the ci chip when status == 'no-ci'", () => {
    const s = makeSession({ worktreePath: "/wt/feat-e" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed("/wt/feat-e", makeMeta({ ciStatus: "no-ci" }));
    const { queryByTestId } = render(StatusBar);
    expect(queryByTestId("status-bar-pr-link")).toBeNull();
    expect(queryByTestId("status-bar-ci-chip")).toBeNull();
  });

  it("renders a hover popover with individual PR check statuses", async () => {
    const s = makeSession({
      repoRoot: "/repo",
      worktreePath: "/wt/feat-checks",
      branch: "feature/x",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-checks",
      makeMeta({
        ciStatus: "running",
        ciUrl: "https://github.com/phin-tech/roux/pull/42",
      }),
    );
    tauriMock.nextPrLookupResult = makePr({
      checks: {
        state: "failing",
        passing: 1,
        failing: 1,
        pending: 1,
        total: 3,
      },
      checkRuns: [
        { name: "cargo test", status: "passing", url: null },
        { name: "npm check", status: "failing", url: null },
        { name: "publish preview", status: "pending", url: null },
      ],
    });
    await lookupPrForSession(s, { force: true });

    const { getByTestId, queryByTestId } = render(StatusBar);
    const link = getByTestId("status-bar-pr-link");
    const popover = getByTestId("status-bar-pr-popover");

    expect(link.className).toContain("text-red");
    expect(link.getAttribute("aria-describedby")).toBe("status-bar-pr-popover");
    expect(link.querySelectorAll("svg")).toHaveLength(1);
    expect(queryByTestId("status-bar-pr-checks")).toBeNull();
    expect(popover.className).not.toContain("pointer-events-none");
    expect(popover.className).toContain("group-focus-within:block");
    expect(popover.textContent).toContain("cargo test");
    expect(popover.textContent).toContain("passing");
    expect(popover.textContent).toContain("npm check");
    expect(popover.textContent).toContain("failing");
    expect(popover.textContent).toContain("publish preview");
    expect(popover.textContent).toContain("pending");
  });

  it("renders a hover popover with individual PR review statuses", async () => {
    const s = makeSession({
      repoRoot: "/repo",
      worktreePath: "/wt/feat-reviews",
      branch: "feature/x",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-reviews",
      makeMeta({
        ciStatus: "passed",
        ciUrl: "https://github.com/phin-tech/roux/pull/42",
      }),
    );
    tauriMock.nextPrLookupResult = makePr({
      reviewDecision: "CHANGES_REQUESTED",
      reviewDetails: [
        { reviewer: "alice", state: "APPROVED", url: null },
        { reviewer: "bob", state: "CHANGES_REQUESTED", url: null },
      ],
    });
    await lookupPrForSession(s, { force: true });

    const { getByTestId, queryByTestId } = render(StatusBar);
    const popover = getByTestId("status-bar-pr-popover");

    expect(queryByTestId("status-bar-pr-review")).toBeNull();
    expect(popover.textContent).toContain("Approvals");
    expect(popover.textContent).toContain("1/2");
    expect(popover.textContent).toContain("alice");
    expect(popover.textContent).toContain("approved");
    expect(popover.textContent).toContain("bob");
    expect(popover.textContent).toContain("changes requested");
  });

  it("can force the PR details popover open without hover", async () => {
    const s = makeSession({
      repoRoot: "/repo",
      worktreePath: "/wt/feat-toggle",
      branch: "feature/x",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    seed(
      "/wt/feat-toggle",
      makeMeta({
        ciStatus: "running",
        ciUrl: "https://github.com/phin-tech/roux/pull/42",
      }),
    );
    tauriMock.nextPrLookupResult = makePr({
      checks: {
        state: "pending",
        passing: 0,
        failing: 0,
        pending: 1,
        total: 1,
      },
      checkRuns: [{ name: "npm check", status: "pending", url: null }],
    });
    await lookupPrForSession(s, { force: true });

    const { getByTestId } = render(StatusBar);
    const popover = getByTestId("status-bar-pr-popover");
    expect(popover.className).toContain("hidden");

    togglePrStatusDetails();
    await tick();

    expect(popover.className).toContain("block");
    expect(popover.className).not.toContain("hidden");
  });
});
