import { get, writable } from "svelte/store";

import { listAllPtys } from "$lib/tauri";
import type { PtyInfo } from "$lib/types";
import { sessionList } from "./sessions";

const POLL_INTERVAL_MS = 5000;

export interface SessionPtyInventory {
  attachedCount: number;
  detachedCount: number;
  detachedHasUnread: boolean;
}

export const ptyInventoryBySession = writable<Map<string, SessionPtyInventory>>(new Map());

let inFlight = false;
let stopPolling: (() => void) | null = null;

export function summarizePtyInventory(
  ptys: PtyInfo[],
  knownSessionIds: Set<string>,
): Map<string, SessionPtyInventory> {
  const next = new Map<string, SessionPtyInventory>();
  for (const pty of ptys) {
    const sessionId = pty.session_id;
    if (!sessionId || !knownSessionIds.has(sessionId)) continue;

    const current = next.get(sessionId) ?? {
      attachedCount: 0,
      detachedCount: 0,
      detachedHasUnread: false,
    };

    if (pty.status.type === "RunningAttached") {
      current.attachedCount += 1;
    } else if (pty.status.type === "RunningDetached") {
      current.detachedCount += 1;
      current.detachedHasUnread ||= pty.unread_output;
    }

    next.set(sessionId, current);
  }
  return next;
}

export async function refreshPtyInventory(): Promise<void> {
  if (inFlight) return;

  const sessions = get(sessionList);
  if (sessions.length === 0) {
    ptyInventoryBySession.set(new Map());
    return;
  }

  inFlight = true;
  try {
    const ptys = await listAllPtys();
    const knownSessionIds = new Set(get(sessionList).map((session) => session.id));
    ptyInventoryBySession.set(summarizePtyInventory(ptys, knownSessionIds));
  } catch {
    // Keep the last known snapshot. The inventory badges are informational.
  } finally {
    inFlight = false;
  }
}

export function initPtyInventoryPolling(intervalMs = POLL_INTERVAL_MS): () => void {
  if (stopPolling) return stopPolling;

  const refresh = () => {
    void refreshPtyInventory();
  };
  let previousSessionKey: string | null = null;
  const unsubscribeSessions = sessionList.subscribe((sessions) => {
    const sessionKey = sessions.map((session) => session.id).join("\0");
    if (sessionKey === previousSessionKey) return;
    previousSessionKey = sessionKey;
    refresh();
  });
  const timer = setInterval(refresh, intervalMs);

  stopPolling = () => {
    unsubscribeSessions();
    clearInterval(timer);
    stopPolling = null;
  };
  return stopPolling;
}

export function _resetPtyInventoryForTests(): void {
  stopPolling?.();
  stopPolling = null;
  inFlight = false;
  ptyInventoryBySession.set(new Map());
}
