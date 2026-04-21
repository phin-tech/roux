import { derived, get, writable, type Readable } from "svelte/store";
import { sessionLayouts, collectVisibleLeafIds } from "$lib/panes/layout";
import { sessionState } from "$lib/stores/sessions";

/**
 * Global sidebar slot. The app renders at most one side panel at a time —
 * Settings, Notes, Watches, Notifications. Any new panel registers here.
 * State is ephemeral (not persisted).
 */
export type SidebarId = "settings" | "notes" | "watches" | "notifications" | "sessions";

export const activeSidebar = writable<SidebarId | null>(null);

/**
 * When non-null, the notes panel targets this session id instead of the
 * app's active session. Used by the sessions-history pane so clicking
 * "View notes" on an archived row scopes the notes view to that session
 * without changing the active session. Cleared automatically when the
 * user leaves the notes sidebar.
 */
export const notesOverrideSessionId = writable<string | null>(null);

export function openSidebar(id: SidebarId): void {
  activeSidebar.set(id);
  if (id !== "notes") notesOverrideSessionId.set(null);
}

export function closeSidebar(): void {
  activeSidebar.set(null);
  notesOverrideSessionId.set(null);
}

export function toggleSidebar(id: SidebarId): void {
  const next = get(activeSidebar) === id ? null : id;
  activeSidebar.set(next);
  if (next !== "notes") notesOverrideSessionId.set(null);
}

export function openNotesForSession(sessionId: string): void {
  notesOverrideSessionId.set(sessionId);
  activeSidebar.set("notes");
}

const HOLD_DELAY_MS = 200;

interface HintController {
  store: Readable<boolean>;
  arm: (delayMs?: number) => void;
  hide: () => void;
}

function createHintController(): HintController {
  const store = writable(false);
  let holdTimer: ReturnType<typeof setTimeout> | null = null;

  function arm(delayMs: number = HOLD_DELAY_MS): void {
    if (holdTimer !== null) return;
    holdTimer = setTimeout(() => {
      holdTimer = null;
      store.set(true);
    }, delayMs);
  }

  function hide(): void {
    if (holdTimer !== null) {
      clearTimeout(holdTimer);
      holdTimer = null;
    }
    store.set(false);
  }

  return { store, arm, hide };
}

const sessionHints = createHintController();
const paneHints = createHintController();

// Session number overlay — shown on sidebar cards when Cmd is held.
export const showSessionHints = sessionHints.store;
export const armSessionHints = sessionHints.arm;
export const hideSessionHints = sessionHints.hide;

// Pane number overlay — shown on visible panes when Alt is held.
export const showPaneHints = paneHints.store;
export const armPaneHints = paneHints.arm;
export const hidePaneHints = paneHints.hide;

/**
 * Map from paneId → slot number (1..10) for the currently-active session,
 * walked in visible DFS order over the pane tree. Used by both the overlay
 * badges and the Alt+digit key handler, so they cannot drift.
 */
export const paneSlotById = derived(
  [sessionLayouts, sessionState],
  ([$layouts, $state]) => {
    const map = new Map<string, number>();
    const activeId = $state.activeSessionId;
    if (!activeId) return map;
    const tree = $layouts.get(activeId);
    if (!tree) return map;
    const ids = collectVisibleLeafIds(tree);
    for (let i = 0; i < ids.length && i < 10; i++) {
      map.set(ids[i], i + 1);
    }
    return map;
  },
);
