// Session ↔ GitHub PR lookup cache.
//
// Sessions carry `repoRoot` and `branch`; the backend's
// `lookup_pr_for_branch` resolves these to a `PrInfo` when an open PR
// exists. This store caches results so rapid pane / session switches
// don't fan out concurrent `gh pr list` calls.
//
// Keyed by `${repoRoot}::${branch}` because the same branch in different
// repos resolves to different PRs, and the same PR can re-appear if the
// user moves a session to a different worktree.
//
// Negative results are cached too — branches without an open PR are the
// common case (a session created on `main` for example), and we don't
// want to hammer `gh` every time the user clicks back.
//
// Deliberately separate from `worktreeMetadata` (whose `upsert` clears
// entries lacking worktrunk data) so users without worktrunk still get
// PR data populated.

import { writable, get, derived, type Readable } from "svelte/store";
import type { Session } from "$lib/types";
import { findOrCreateWatch, lookupPrForBranch, type PrInfo } from "$lib/tauri";
import { activeSession } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";

const HIT_TTL_MS = 60_000;
const MISS_TTL_MS = 5 * 60_000;

export interface PrLookupEntry {
  prInfo: PrInfo | null;
  fetchedAt: number;
  inFlight: boolean;
}

type LookupMap = Map<string, PrLookupEntry>;

const lookupStore = writable<LookupMap>(new Map());

export const sessionPrLookup: Readable<LookupMap> = {
  subscribe: lookupStore.subscribe,
};

function keyFor(repoRoot: string, branch: string): string {
  return `${repoRoot}::${branch}`;
}

function isFresh(entry: PrLookupEntry): boolean {
  const ttl = entry.prInfo ? HIT_TTL_MS : MISS_TTL_MS;
  return Date.now() - entry.fetchedAt < ttl;
}

function shouldFetch(entry: PrLookupEntry | undefined): boolean {
  if (!entry) return true;
  if (entry.inFlight) return false;
  return !isFresh(entry);
}

/**
 * Reactive lookup keyed by `(repoRoot, branch)`. Returns the cached
 * `PrInfo` (or `null` when the lookup confirmed no PR), or `undefined`
 * when the lookup hasn't run yet.
 */
export function prLookupFor(
  repoRoot: string | null | undefined,
  branch: string | null | undefined,
): Readable<PrInfo | null | undefined> {
  return derived(lookupStore, ($map) => {
    if (!repoRoot || !branch) return null;
    const entry = $map.get(keyFor(repoRoot, branch));
    return entry ? entry.prInfo : undefined;
  });
}

/**
 * Snapshot accessor (non-reactive). Returns the cached `PrInfo`, `null`
 * when we know there is no PR, or `undefined` when no lookup has run.
 */
export function getPrLookupSnapshot(
  repoRoot: string | null | undefined,
  branch: string | null | undefined,
): PrInfo | null | undefined {
  if (!repoRoot || !branch) return null;
  const entry = get(lookupStore).get(keyFor(repoRoot, branch));
  return entry ? entry.prInfo : undefined;
}

/**
 * Trigger a backend lookup for `(repoRoot, branch)` if the cached entry
 * is stale or missing. Force-true bypasses the TTL (used by the manual
 * "Refresh PR for session" command).
 */
export async function lookupPrForSession(
  session: Pick<Session, "repoRoot" | "branch" | "isGitRepo">,
  opts: { force?: boolean } = {},
): Promise<PrInfo | null> {
  const { repoRoot, branch, isGitRepo } = session;
  if (!isGitRepo || !repoRoot || !branch) return null;

  const key = keyFor(repoRoot, branch);
  const map = get(lookupStore);
  const existing = map.get(key);

  if (!opts.force && existing && !shouldFetch(existing)) {
    return existing.prInfo;
  }
  if (existing?.inFlight) return existing.prInfo;

  lookupStore.update((m) => {
    const next = new Map(m);
    next.set(key, {
      prInfo: existing?.prInfo ?? null,
      fetchedAt: existing?.fetchedAt ?? 0,
      inFlight: true,
    });
    return next;
  });

  try {
    const result = await lookupPrForBranch(repoRoot, branch);
    lookupStore.update((m) => {
      const next = new Map(m);
      next.set(key, {
        prInfo: result,
        fetchedAt: Date.now(),
        inFlight: false,
      });
      return next;
    });
    return result;
  } catch {
    // Backend errors (gh missing, auth failure, network) — clear the
    // in-flight flag but don't poison the cache. Next call retries.
    lookupStore.update((m) => {
      const next = new Map(m);
      const prev = next.get(key);
      if (prev) {
        next.set(key, { ...prev, inFlight: false });
      }
      return next;
    });
    return null;
  }
}

/**
 * Hard reset — used by tests. Not for production callers.
 */
export function _resetSessionPrLookupForTests(): void {
  lookupStore.set(new Map());
  autoWatchedKeys.clear();
}

/**
 * Track which `(sessionId, repo, prNumber)` tuples we've already auto-watched
 * in this app run. The backend `cmd_find_or_create_watch` is the source of
 * truth — this is a frontend short-circuit so we don't re-call it on every
 * cache hit.
 */
const autoWatchedKeys = new Set<string>();

function autoWatchKey(
  sessionId: string,
  repo: string,
  prNumber: number,
): string {
  return `${sessionId}::${repo}::${prNumber}`;
}

async function maybeAutoWatch(
  sessionId: string,
  prInfo: PrInfo,
): Promise<void> {
  const repo = prInfo.repoSlug;
  const key = autoWatchKey(sessionId, repo, prInfo.number);
  if (autoWatchedKeys.has(key)) return;
  autoWatchedKeys.add(key);
  try {
    await findOrCreateWatch({
      name: `PR: ${repo} #${prInfo.number}`,
      kind: { type: "githubPr", repo, prNumber: prInfo.number },
      mode: { type: "recurring", intervalSecs: 30 },
      scope: { type: "session", sessionId },
      notify: null,
    });
  } catch {
    // Backend errors (gh missing, watch service down) — drop the dedupe
    // marker so the next refresh can retry.
    autoWatchedKeys.delete(key);
  }
}

/**
 * Subscribe to `activeSession` and trigger PR lookup + (optionally) watch
 * creation on every session change. Respects the `autoLookupSessionPr` and
 * `autoWatchSessionPr` settings — when lookup is disabled, no gh call is
 * made and the rest is a no-op. Returns an unsubscribe function for tests
 * and clean shutdown.
 *
 * Dedupe key intentionally includes `repoRoot` and `branch`, not just
 * `id` — `activeSession` re-fires when those mutate in place (e.g.
 * branch rename, worktree move) and we want a fresh lookup in that
 * case. `lookupPrForSession`'s own TTL/in-flight guards keep the cost
 * bounded for unrelated session-property changes that share the same
 * triple.
 */
export function installSessionPrEffect(): () => void {
  let lastKey: string | null = null;
  return activeSession.subscribe((session) => {
    if (!session) {
      lastKey = null;
      return;
    }
    const key = `${session.id}|${session.repoRoot}|${session.branch}`;
    if (key === lastKey) return;
    lastKey = key;

    const s = get(settings);
    if (s.autoLookupSessionPr === false) return;
    if (!session.isGitRepo || !session.repoRoot || !session.branch) return;

    const sessionId = session.id;
    void lookupPrForSession(session).then((prInfo) => {
      if (!prInfo) return;
      if (get(settings).autoWatchSessionPr) {
        void maybeAutoWatch(sessionId, prInfo);
      }
    });
  });
}
