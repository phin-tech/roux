// Store of worktrunk-sourced metadata indexed by absolute worktree path.
//
// Populated lazily from `cmdListWorktrees` — whenever the UI already lists
// worktrees for a repo (e.g. the New Session dialog or a session-list
// refresh), it feeds the result through [`upsertWorktreeMetadata`] so
// downstream consumers (session cards, status chips) can read metadata
// without running their own Tauri calls.
//
// Keyed by worktree *path* because paths are stable per worktree across
// branch renames; `Session.worktreePath` is the join key from the session
// side.
//
// This store deliberately carries only `WorktrunkMetadata` — when `wt`
// isn't installed, entries never populate and consumers render exactly
// the same as today.

import { writable, get, derived, type Readable } from "svelte/store";
import type { Worktree, WorktrunkMetadata } from "$lib/types";
import { listWorktrees } from "$lib/tauri";

export type WorktreeMetadataMap = Map<string, WorktrunkMetadata>;

const metadataStore = writable<WorktreeMetadataMap>(new Map());

export const worktreeMetadata: Readable<WorktreeMetadataMap> = {
  subscribe: metadataStore.subscribe,
};

/**
 * Merge a fresh listing into the store. Entries that have non-null
 * `worktrunk` metadata are upserted; entries without metadata clear any
 * stale metadata we had for that path (e.g. the user uninstalled wt).
 */
export function upsertWorktreeMetadata(entries: Worktree[]): void {
  metadataStore.update((map) => {
    const next = new Map(map);
    for (const wt of entries) {
      if (wt.worktrunk) {
        next.set(wt.path, wt.worktrunk);
      } else {
        next.delete(wt.path);
      }
    }
    return next;
  });
}

/**
 * Hard reset — used by tests. Not for production callers.
 */
export function _resetWorktreeMetadataForTests(): void {
  metadataStore.set(new Map());
}

/**
 * Read the current metadata entry for a worktree path (snapshot, not reactive).
 * Returns `null` when wt hasn't surfaced metadata for that path.
 */
export function getWorktreeMetadataSnapshot(
  worktreePath: string,
): WorktrunkMetadata | null {
  return get(metadataStore).get(worktreePath) ?? null;
}

/**
 * Reactive lookup for a single worktree path. Returns a `Readable` that
 * re-emits whenever the store is updated with a new entry (or when the
 * entry is cleared).
 */
export function worktreeMetadataFor(
  worktreePath: string,
): Readable<WorktrunkMetadata | null> {
  return derived(metadataStore, ($map) => $map.get(worktreePath) ?? null);
}

/**
 * Fetch the listing for `repoPath` via Tauri and upsert the store.
 * No-op when `repoPath` is empty. Swallows errors — caller-visible
 * failures aren't the point of a metadata cache.
 */
export async function refreshWorktreeMetadata(repoPath: string): Promise<void> {
  if (!repoPath) return;
  try {
    const entries = await listWorktrees(repoPath);
    upsertWorktreeMetadata(entries);
  } catch {
    // Silently ignore — wt may not be installed, or the repo may have
    // been removed. Consumers fall back to the empty-metadata path.
  }
}

/**
 * Fan out `refreshWorktreeMetadata` across a set of repos. De-duplicates
 * so a session list with five sessions in one repo only hits the
 * backend once.
 */
export async function refreshWorktreeMetadataForRepos(
  repoPaths: Iterable<string>,
): Promise<void> {
  const unique = new Set<string>();
  for (const p of repoPaths) {
    if (p) unique.add(p);
  }
  await Promise.all(Array.from(unique).map((p) => refreshWorktreeMetadata(p)));
}
