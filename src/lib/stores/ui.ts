import { derived, writable, type Readable } from "svelte/store";
import { sessionLayouts, collectVisibleLeafIds } from "$lib/panes/layout";
import { sessionState } from "$lib/stores/sessions";

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
