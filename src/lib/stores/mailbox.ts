import { writable, derived, get } from "svelte/store";
import {
  aliasesList,
  mailboxAck as tauriMailboxAck,
  mailboxClearRead as tauriMailboxClearRead,
  mailboxListAll,
  mailboxMarkRead as tauriMailboxMarkRead,
  mailboxPost as tauriMailboxPost,
  mailboxUnreadCount,
} from "$lib/tauri";
import type {
  AgentAlias,
  AliasEvent,
  MailboxEvent,
  MailboxEventPayload,
  MailboxPostInput,
} from "$lib/tauri";

const EVENT_LIMIT = 500;

/**
 * Compound key for `unreadByAlias` and any other map that needs to
 * disambiguate same-name aliases across project scopes. `null` projectId
 * encodes the global scope; the empty-string sentinel keeps the key a
 * plain string for Map use.
 */
export function aliasKey(alias: string, projectId: string | null): string {
  return `${alias}|${projectId ?? ""}`;
}

/**
 * Authoritative store of recent events, newest-first. Capped at
 * `EVENT_LIMIT` so the firehose view stays bounded; the on-disk audit
 * log keeps everything regardless.
 */
export const events = writable<MailboxEventPayload[]>([]);

/** Authoritative store of all aliases (bound + unbound). */
export const aliases = writable<AgentAlias[]>([]);

/**
 * Per-alias unread count, keyed by `aliasKey(alias, projectId)` so the
 * same alias name in different project scopes doesn't collide. Refreshed
 * whenever a relevant `mailbox-event` fires; caller can also force-refresh
 * via `refreshUnreadCount`.
 */
export const unreadByAlias = writable<Map<string, number>>(new Map());

/** Total unread count for the human-user mailbox (`me`, global scope). */
export const meUnread = derived(
  unreadByAlias,
  ($u) => $u.get(aliasKey("me", null)) ?? 0,
);

/** Events addressed to `recipient` (oldest first to match drain semantics). */
export function eventsForRecipient(recipient: string) {
  return derived(events, ($events) =>
    $events
      .filter((e) => e.to === recipient)
      .slice()
      .reverse(),
  );
}

/** Hydrate the stores from the backend. Call once on app start. */
export async function hydrateMailbox(): Promise<void> {
  try {
    const [evs, als] = await Promise.all([
      mailboxListAll({ limit: EVENT_LIMIT }),
      aliasesList({}),
    ]);
    events.set(evs);
    aliases.set(als);
    await refreshAllUnreadCounts();
  } catch (err) {
    console.error("Failed to hydrate mailbox", err);
  }
}

async function refreshAllUnreadCounts(): Promise<void> {
  const aliasList = get(aliases);
  const counts = new Map<string, number>();
  await Promise.all(
    aliasList.map(async (a) => {
      try {
        const c = await mailboxUnreadCount(a.alias, {
          projectId: a.projectId ?? null,
          global: a.projectId == null,
        });
        counts.set(aliasKey(a.alias, a.projectId), c);
      } catch (err) {
        console.warn(`unread count for ${a.alias} failed`, err);
      }
    }),
  );
  unreadByAlias.set(counts);
}

/**
 * Refresh the unread count for a specific (alias, projectId) scope.
 * When the caller doesn't know the projectId (e.g. a `Read`/`Acked`/
 * `Cleared` Tauri event that only carries the recipient name), pass
 * `undefined` and we'll refresh all known scopes for that alias name.
 */
export async function refreshUnreadCount(
  alias: string,
  projectId: string | null | undefined,
): Promise<void> {
  if (projectId === undefined) {
    // Unknown scope — refresh every alias entry that matches this name.
    const matching = get(aliases).filter((a) => a.alias === alias);
    if (matching.length === 0) {
      // No alias entry yet (e.g. mailbox event for an alias we haven't
      // hydrated). Refresh the global scope as a best-effort default.
      await refreshUnreadCount(alias, null);
      return;
    }
    await Promise.all(
      matching.map((a) => refreshUnreadCount(alias, a.projectId)),
    );
    return;
  }
  try {
    const c = await mailboxUnreadCount(alias, {
      projectId,
      global: projectId == null,
    });
    unreadByAlias.update((m) => {
      const next = new Map(m);
      next.set(aliasKey(alias, projectId), c);
      return next;
    });
  } catch (err) {
    console.warn(`unread count for ${alias} failed`, err);
  }
}

/**
 * Bumped on every `applyMailboxEvent` call so per-recipient views can
 * react to backend mutations (mark-read / ack / clear) by refetching
 * their backend-driven listings. The `events` store doesn't change for
 * read-state-only mutations, so components that need to refresh on
 * those events subscribe to this tick instead.
 */
export const mailboxMutationTick = writable(0);

/**
 * Apply a `MailboxEvent` to the local store. Idempotent for duplicates.
 * The store is a cache of the backend; if state diverges, callers can
 * always re-hydrate.
 */
export function applyMailboxEvent(event: MailboxEvent): void {
  mailboxMutationTick.update((n) => n + 1);
  switch (event.kind) {
    case "posted": {
      const e = event.event;
      events.update((list) => {
        if (list.some((x) => x.id === e.id)) return list;
        const next = [e, ...list];
        return next.length > EVENT_LIMIT ? next.slice(0, EVENT_LIMIT) : next;
      });
      // The recipient (and the human, who can see fanout into Firehose)
      // both need their unread totals refreshed. The event carries
      // `projectId`, so the refresh targets exactly the right scope.
      if (e.to) {
        void refreshUnreadCount(e.to, e.projectId);
      }
      break;
    }
    case "read":
    case "acked":
    case "cleared":
    case "dismissed": {
      // Read-state-only mutations don't change the events array. The
      // Tauri payload doesn't carry projectId — refresh every known
      // scope for that recipient (fan-out is small in practice).
      void refreshUnreadCount(event.recipient, undefined);
      break;
    }
    case "retracted": {
      // Mark the event row retracted in-place so the firehose / sent
      // view can render it as such; recipient inbox queries hit the
      // backend (which already filters retracted) so they refresh
      // through the existing mailboxMutationTick effect.
      events.update((list) =>
        list.map((e) =>
          e.id === event.eventId
            ? { ...e, retractedAt: e.retractedAt ?? Date.now() }
            : e,
        ),
      );
      // Bump every alias's unread count — the retracted event might
      // have been counted somewhere we can't pinpoint without scanning.
      void refreshAllUnreadCounts();
      break;
    }
    case "topicDelivered": {
      // Bump the subscriber's unread count.
      void refreshUnreadCount(event.recipient, undefined);
      break;
    }
  }
}

/** Apply an `AliasEvent` to the alias store. */
export function applyAliasEvent(event: AliasEvent): void {
  switch (event.kind) {
    case "set": {
      const a = event.alias;
      aliases.update((list) => {
        const filtered = list.filter(
          (x) => !(x.alias === a.alias && x.projectId === a.projectId),
        );
        return [...filtered, a];
      });
      break;
    }
    case "unset": {
      // Clear ALL binding fields (sessionId, paneId, autoClaimed) — not
      // just sessionId. The Phase 1.5 model binds aliases to panes, so
      // leaving paneId set would keep the @alias chip on the pane and
      // the Deliver button enabled even after the backend unbound the
      // alias.
      aliases.update((list) =>
        list.map((x) =>
          x.alias === event.canonical && x.projectId === event.projectId
            ? { ...x, sessionId: null, paneId: null, autoClaimed: false }
            : x,
        ),
      );
      break;
    }
  }
}

// ── Mutation helpers (call the backend; the local store is updated via the
// event channel emitted from Rust). Returning the awaited result lets
// optimistic-UI callers settle.

export async function postMailboxMessage(
  input: MailboxPostInput,
): Promise<MailboxEventPayload> {
  return tauriMailboxPost(input);
}

export async function markRead(
  eventId: string,
  recipient: string,
): Promise<boolean> {
  return tauriMailboxMarkRead(eventId, recipient);
}

export async function ackEvent(
  eventId: string,
  recipient: string,
  result: string | null = null,
): Promise<boolean> {
  return tauriMailboxAck(eventId, recipient, result);
}

export async function clearReadFor(recipient: string): Promise<number> {
  return tauriMailboxClearRead(recipient);
}

/** Synchronous snapshot for action handlers. */
export function getEventSnapshot(id: string): MailboxEventPayload | undefined {
  return get(events).find((e) => e.id === id);
}

// ── Threading ──────────────────────────────────────────────────────────────

/**
 * One thread in the inbox view. A thread groups events sharing the same
 * `correlationId`; events without a correlation are 1-event threads
 * (visually identical to a flat row).
 *
 * Resolution rules:
 * - The root is the event whose `id === correlationId` (the original
 *   message that seeded the thread). If that event isn't in the
 *   provided slice (clipped, dismissed, retracted) the earliest event
 *   in the group becomes the visual root.
 * - Replies are sorted oldest → newest so the conversation reads
 *   top-to-bottom.
 */
export interface MailboxThread {
  /** Stable id for keyed rendering. Equals `correlationId` when set,
   *  else the singleton's event id. */
  id: string;
  root: MailboxEventPayload;
  replies: MailboxEventPayload[];
}

/**
 * Group a flat event list into threads by `correlationId`. Threads are
 * ordered by their root's `createdAt` ascending, matching the flat
 * drain order ("oldest first") the inbox already used. Pure function;
 * exported so it can be unit-tested without a Tauri runtime.
 */
export function groupIntoThreads(
  events: MailboxEventPayload[],
): MailboxThread[] {
  const buckets = new Map<string, MailboxEventPayload[]>();
  for (const e of events) {
    const key = e.correlationId ?? e.id;
    const bucket = buckets.get(key);
    if (bucket) bucket.push(e);
    else buckets.set(key, [e]);
  }

  const threads: MailboxThread[] = [];
  for (const [key, group] of buckets) {
    group.sort((a, b) => a.createdAt - b.createdAt);
    // Prefer the event whose id matches the correlationId — that's
    // the original. Fall back to the earliest visible event when the
    // root isn't in this slice.
    const rootIdx = group.findIndex((e) => e.id === key);
    const root = rootIdx >= 0 ? group[rootIdx] : group[0];
    const replies = group.filter((e) => e.id !== root.id);
    threads.push({ id: key, root, replies });
  }

  // Stable thread order: oldest root first. Ties broken by id so the
  // result is deterministic across hot reloads.
  threads.sort((a, b) => {
    const byTime = a.root.createdAt - b.root.createdAt;
    if (byTime !== 0) return byTime;
    return a.id.localeCompare(b.id);
  });
  return threads;
}
