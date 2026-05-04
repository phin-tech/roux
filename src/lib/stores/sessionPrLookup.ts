// Session ↔ GitHub PR lookup cache.
//
// Sessions carry `repoRoot` and `branch`; the backend's
// `lookup_pr_for_branch` resolves these to a `PrInfo` when an open PR
// exists. When the session has a pinned PR URL we use the URL-form
// `lookup_pr` instead, which still goes through `gh` but skips the
// branch-based search entirely.
//
// Keyed by `${repoRoot}::${branch}` (or `pin::${url}` for pinned PRs)
// because the same branch in different repos resolves to different PRs.
//
// Negative results are cached too. Miss TTL is short (60s) because the
// canonical "miss" is a freshly-pushed branch that's about to have a PR;
// the focus-refresh + branch-poll paths bypass the TTL when we have a
// good signal that a re-lookup is worthwhile.
//
// `lastError` lets the StatusBar surface a discreet warning glyph when
// `gh` fails (auth missing, network down) so the user has a clue why
// the chip isn't there.
//
// Deliberately separate from `worktreeMetadata` (whose `upsert` clears
// entries lacking worktrunk data) so users without worktrunk still get
// PR data populated.

import { writable, get, derived, type Readable } from "svelte/store";
import type { Session } from "$lib/types";
import {
  findOrCreateWatch,
  lookupPr,
  lookupPrForBranch,
  type PrInfo,
} from "$lib/tauri";
import { activeSession, sessionList } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";

const HIT_TTL_MS = 60_000;
const MISS_TTL_MS = 60_000;

export interface PrLookupEntry {
  prInfo: PrInfo | null;
  fetchedAt: number;
  inFlight: boolean;
  /** Most recent backend error, when the last lookup failed. Cleared on
   * a subsequent successful lookup. */
  lastError: string | null;
}

type LookupMap = Map<string, PrLookupEntry>;

function pinKey(url: string): string {
  return `pin::${url}`;
}

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
 * Reactive lookup for a session — honors `pinnedPrUrl` first, falling
 * back to the branch-based cache. Returns the cached `PrInfo`, `null`
 * when the lookup confirmed no PR, or `undefined` when nothing has run.
 */
export function prLookupForSession(
  session: Pick<Session, "repoRoot" | "branch" | "pinnedPrUrl"> | null | undefined,
): Readable<PrInfo | null | undefined> {
  return derived(lookupStore, ($map) => {
    if (!session) return null;
    if (session.pinnedPrUrl) {
      const pinned = $map.get(pinKey(session.pinnedPrUrl));
      return pinned ? pinned.prInfo : undefined;
    }
    if (!session.repoRoot || !session.branch) return null;
    const entry = $map.get(keyFor(session.repoRoot, session.branch));
    return entry ? entry.prInfo : undefined;
  });
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
 * Reactive view of the cached error (if any) for a session's most recent
 * lookup. Surfaces auth / network / `gh-missing` failures in the UI so
 * the user knows why the chip isn't there. Returns `null` when there is
 * either no cached entry or the last lookup succeeded.
 */
export function prLookupErrorFor(
  session: Pick<Session, "repoRoot" | "branch" | "pinnedPrUrl"> | null | undefined,
): Readable<string | null> {
  return derived(lookupStore, ($map) => {
    if (!session) return null;
    const key = session.pinnedPrUrl
      ? pinKey(session.pinnedPrUrl)
      : session.repoRoot && session.branch
        ? keyFor(session.repoRoot, session.branch)
        : null;
    if (!key) return null;
    const entry = $map.get(key);
    return entry?.lastError ?? null;
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
 * Trigger a backend lookup for the session if the cached entry is stale
 * or missing. Honors `pinnedPrUrl` first, falling back to the
 * branch-based search. `force: true` bypasses the TTL (used by the
 * manual `Refresh PR for active session` command and by the
 * window-focus refresh path).
 */
export async function lookupPrForSession(
  session: Pick<Session, "repoRoot" | "branch" | "isGitRepo" | "pinnedPrUrl">,
  opts: { force?: boolean } = {},
): Promise<PrInfo | null> {
  if (session.pinnedPrUrl) {
    return lookupForKey(
      pinKey(session.pinnedPrUrl),
      () => lookupPr(session.repoRoot ?? null, session.pinnedPrUrl as string),
      opts,
    );
  }
  const { repoRoot, branch, isGitRepo } = session;
  if (!isGitRepo || !repoRoot || !branch) return null;
  return lookupForKey(
    keyFor(repoRoot, branch),
    () => lookupPrForBranch(repoRoot, branch),
    opts,
  );
}

async function lookupForKey(
  key: string,
  fetchFn: () => Promise<PrInfo | null>,
  opts: { force?: boolean },
): Promise<PrInfo | null> {
  const existing = get(lookupStore).get(key);

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
      lastError: existing?.lastError ?? null,
    });
    return next;
  });

  try {
    const result = await fetchFn();
    lookupStore.update((m) => {
      const next = new Map(m);
      next.set(key, {
        prInfo: result,
        fetchedAt: Date.now(),
        inFlight: false,
        lastError: null,
      });
      return next;
    });
    return result;
  } catch (err) {
    // Backend errors (gh missing, auth failure, network) — clear the
    // in-flight flag, surface the message via `lastError` for the UI,
    // but don't poison the `prInfo` cache. Next call retries.
    const message = err instanceof Error ? err.message : String(err);
    lookupStore.update((m) => {
      const next = new Map(m);
      const prev = next.get(key);
      next.set(key, {
        prInfo: prev?.prInfo ?? null,
        fetchedAt: prev?.fetchedAt ?? 0,
        inFlight: false,
        lastError: message,
      });
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
 * Dedupe key intentionally includes `repoRoot`, `branch`, and
 * `pinnedPrUrl`, not just `id` — `activeSession` re-fires when those
 * mutate in place (e.g. branch rename, worktree move, user pinning a
 * PR) and we want a fresh lookup in those cases. `lookupPrForSession`'s
 * own TTL/in-flight guards keep the cost bounded for unrelated
 * session-property changes that share the same tuple.
 */
export function installSessionPrEffect(): () => void {
  let lastKey: string | null = null;
  return activeSession.subscribe((session) => {
    if (!session) {
      lastKey = null;
      return;
    }
    const key = `${session.id}|${session.repoRoot}|${session.branch}|${session.pinnedPrUrl ?? ""}`;
    if (key === lastKey) return;
    lastKey = key;

    const s = get(settings);
    if (s.autoLookupSessionPr === false) return;
    // Pinned PRs work even outside a git repo (the user explicitly told
    // us which PR to track) — the branch-based path needs the full triple.
    if (!session.pinnedPrUrl) {
      if (!session.isGitRepo || !session.repoRoot || !session.branch) return;
    }

    const sessionId = session.id;
    void lookupPrForSession(session).then((prInfo) => {
      if (!prInfo) return;
      if (get(settings).autoWatchSessionPr) {
        void maybeAutoWatch(sessionId, prInfo);
      }
    });
  });
}

/**
 * Force-refresh the active session's PR lookup. Called from the
 * window-focus listener so freshly-pushed PRs surface without waiting
 * for the negative cache TTL. Also propagates `autoWatchSessionPr` —
 * a PR discovered here (with no preceding session mutation) still
 * needs to create its session-scoped watch.
 */
export function refreshActiveSessionPr(): Promise<PrInfo | null> {
  const session = get(activeSession);
  if (!session) return Promise.resolve(null);
  if (get(settings).autoLookupSessionPr === false) return Promise.resolve(null);
  if (
    !session.pinnedPrUrl &&
    (!session.isGitRepo || !session.repoRoot || !session.branch)
  ) {
    return Promise.resolve(null);
  }
  const sessionId = session.id;
  return lookupPrForSession(session, { force: true }).then((prInfo) => {
    if (prInfo && get(settings).autoWatchSessionPr) {
      void maybeAutoWatch(sessionId, prInfo);
    }
    return prInfo;
  });
}

/**
 * One row per session with a resolved PR — feeds the project-level
 * "Open PRs in this project" panel. Pinned PRs are honored too.
 *
 * Filtering by `projectId` happens here (rather than in the panel)
 * because the same store powers the "all projects" overview; passing
 * `null` returns rows for sessions with no project.
 */
export interface ProjectPrRow {
  session: Session;
  prInfo: PrInfo;
  pinned: boolean;
}

export function projectPrRowsFor(
  projectId: string | null | undefined,
): Readable<ProjectPrRow[]> {
  return derived([sessionList, lookupStore], ([$sessions, $map]) => {
    const rows: ProjectPrRow[] = [];
    for (const s of $sessions) {
      if (s.archived) continue;
      const sessionProject = s.projectId ?? null;
      const targetProject = projectId ?? null;
      if (sessionProject !== targetProject) continue;
      const key = s.pinnedPrUrl
        ? pinKey(s.pinnedPrUrl)
        : s.repoRoot && s.branch
          ? keyFor(s.repoRoot, s.branch)
          : null;
      if (!key) continue;
      const entry = $map.get(key);
      if (!entry?.prInfo) continue;
      rows.push({ session: s, prInfo: entry.prInfo, pinned: !!s.pinnedPrUrl });
    }
    return rows;
  });
}
