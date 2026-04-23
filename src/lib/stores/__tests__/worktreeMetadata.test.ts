import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { Worktree, WorktrunkMetadata } from "$lib/types";

vi.mock("$lib/tauri", () => ({
  listWorktrees: vi.fn(),
}));

import { listWorktrees } from "$lib/tauri";
import {
  _resetWorktreeMetadataForTests,
  getWorktreeMetadataSnapshot,
  refreshWorktreeMetadata,
  refreshWorktreeMetadataForRepos,
  upsertWorktreeMetadata,
  worktreeMetadata,
  worktreeMetadataFor,
} from "../worktreeMetadata";

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

function makeWorktree(path: string, meta: WorktrunkMetadata | null): Worktree {
  return {
    path,
    branch: "branch-for-" + path,
    isMain: false,
    worktrunk: meta,
  };
}

describe("worktreeMetadata store", () => {
  beforeEach(() => {
    _resetWorktreeMetadataForTests();
    vi.mocked(listWorktrees).mockReset();
  });

  afterEach(() => {
    _resetWorktreeMetadataForTests();
  });

  it("upsert populates entries keyed by worktree path", () => {
    upsertWorktreeMetadata([
      makeWorktree("/a", makeMeta({ dirty: true })),
      makeWorktree("/b", makeMeta({ ahead: 2 })),
    ]);
    const map = get(worktreeMetadata);
    expect(map.size).toBe(2);
    expect(map.get("/a")?.dirty).toBe(true);
    expect(map.get("/b")?.ahead).toBe(2);
  });

  it("upsert with null metadata clears stale entries", () => {
    upsertWorktreeMetadata([makeWorktree("/a", makeMeta({ dirty: true }))]);
    expect(get(worktreeMetadata).has("/a")).toBe(true);

    // User uninstalls wt: listing comes back with worktrunk: null.
    upsertWorktreeMetadata([makeWorktree("/a", null)]);
    expect(get(worktreeMetadata).has("/a")).toBe(false);
  });

  it("upsert leaves unrelated entries untouched", () => {
    upsertWorktreeMetadata([
      makeWorktree("/a", makeMeta({ dirty: true })),
      makeWorktree("/b", makeMeta({ ahead: 1 })),
    ]);
    upsertWorktreeMetadata([makeWorktree("/a", makeMeta({ dirty: false }))]);
    const map = get(worktreeMetadata);
    expect(map.get("/a")?.dirty).toBe(false);
    expect(map.get("/b")?.ahead).toBe(1);
  });

  it("getWorktreeMetadataSnapshot returns the current entry or null", () => {
    expect(getWorktreeMetadataSnapshot("/a")).toBeNull();
    upsertWorktreeMetadata([makeWorktree("/a", makeMeta({ ahead: 5 }))]);
    expect(getWorktreeMetadataSnapshot("/a")?.ahead).toBe(5);
  });

  it("worktreeMetadataFor is reactive to upserts", () => {
    const readable = worktreeMetadataFor("/a");
    const seen: Array<WorktrunkMetadata | null> = [];
    const unsub = readable.subscribe((v) => seen.push(v));

    upsertWorktreeMetadata([makeWorktree("/a", makeMeta({ behind: 3 }))]);
    upsertWorktreeMetadata([makeWorktree("/a", null)]);

    unsub();
    // Initial null, then populated, then cleared.
    expect(seen[0]).toBeNull();
    expect(seen[1]?.behind).toBe(3);
    expect(seen[seen.length - 1]).toBeNull();
  });

  it("refreshWorktreeMetadata calls listWorktrees and upserts", async () => {
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree("/x", makeMeta({ isCurrent: true })),
    ]);
    await refreshWorktreeMetadata("/repo");
    expect(listWorktrees).toHaveBeenCalledWith("/repo");
    expect(getWorktreeMetadataSnapshot("/x")?.isCurrent).toBe(true);
  });

  it("refreshWorktreeMetadata swallows errors (no throw)", async () => {
    vi.mocked(listWorktrees).mockRejectedValueOnce(new Error("wt offline"));
    await expect(refreshWorktreeMetadata("/repo")).resolves.toBeUndefined();
    expect(get(worktreeMetadata).size).toBe(0);
  });

  it("refreshWorktreeMetadata ignores empty repo path", async () => {
    await refreshWorktreeMetadata("");
    expect(listWorktrees).not.toHaveBeenCalled();
  });

  it("refreshWorktreeMetadataForRepos de-duplicates the fan-out", async () => {
    vi.mocked(listWorktrees).mockResolvedValue([]);
    await refreshWorktreeMetadataForRepos(["/a", "/b", "/a", "", "/b"]);
    expect(listWorktrees).toHaveBeenCalledTimes(2);
    const calls = vi.mocked(listWorktrees).mock.calls.map((c) => c[0]).sort();
    expect(calls).toEqual(["/a", "/b"]);
  });
});
