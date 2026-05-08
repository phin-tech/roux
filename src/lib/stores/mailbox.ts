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
 * Authoritative store of recent events, newest-first. Capped at
 * `EVENT_LIMIT` so the firehose view stays bounded; the on-disk audit
 * log keeps everything regardless.
 */
export const events = writable<MailboxEventPayload[]>([]);

/** Authoritative store of all aliases (bound + unbound). */
export const aliases = writable<AgentAlias[]>([]);

/**
 * Per-alias unread count. Refreshed whenever a relevant `mailbox-event`
 * fires; caller can also force-refresh via `refreshUnreadCount`.
 */
export const unreadByAlias = writable<Map<string, number>>(new Map());

/** Total unread count for the human-user mailbox (`me`). */
export const meUnread = derived(unreadByAlias, ($u) => $u.get("me") ?? 0);

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
        const c = await mailboxUnreadCount(a.alias);
        counts.set(a.alias, c);
      } catch (err) {
        console.warn(`unread count for ${a.alias} failed`, err);
      }
    }),
  );
  unreadByAlias.set(counts);
}

export async function refreshUnreadCount(alias: string): Promise<void> {
  try {
    const c = await mailboxUnreadCount(alias);
    unreadByAlias.update((m) => {
      const next = new Map(m);
      next.set(alias, c);
      return next;
    });
  } catch (err) {
    console.warn(`unread count for ${alias} failed`, err);
  }
}

/**
 * Apply a `MailboxEvent` to the local store. Idempotent for duplicates.
 * The store is a cache of the backend; if state diverges, callers can
 * always re-hydrate.
 */
export function applyMailboxEvent(event: MailboxEvent): void {
  switch (event.kind) {
    case "posted": {
      const e = event.event;
      events.update((list) => {
        if (list.some((x) => x.id === e.id)) return list;
        const next = [e, ...list];
        return next.length > EVENT_LIMIT ? next.slice(0, EVENT_LIMIT) : next;
      });
      // The recipient (and the human, who can see fanout into Firehose)
      // both need their unread totals refreshed.
      if (e.to) {
        void refreshUnreadCount(e.to);
      }
      break;
    }
    case "read":
    case "acked":
    case "cleared": {
      // Read-state-only mutations don't change the events array. Refresh
      // the affected recipient's unread total — Cleared is a single
      // recipient too. Cheap query against the in-memory store.
      void refreshUnreadCount(event.recipient);
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
      aliases.update((list) =>
        list.map((x) =>
          x.alias === event.canonical && x.projectId === event.projectId
            ? { ...x, sessionId: null }
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

export async function markRead(eventId: string, recipient: string): Promise<boolean> {
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
