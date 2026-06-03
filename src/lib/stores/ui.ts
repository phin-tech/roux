import { derived, get, writable, type Readable } from "svelte/store";
import { sessionLayouts, collectVisibleLeafIds } from "$lib/panes/layout";
import {
  activeSessionId,
  sessionState,
  setActiveSession,
} from "$lib/stores/sessions";
import { showSidebar } from "$lib/stores/sidebarLayout";
import { closeMainView, openMainView } from "$lib/stores/mainView";
import { openExternalTool } from "$lib/stores/externalTools";
import type { StartupTarget } from "$lib/bindings";
import type { WorkItemStatus } from "$lib/stores/workItems";

/**
 * Global sidebar slots. The docked sidebar has a pin slot (for lightweight,
 * always-visible panels like Notes) and an active slot (for the currently
 * focused panel). When both are set to different ids they render stacked;
 * otherwise the single panel takes the full docked region.
 */
export type SidebarId =
  | "notes"
  | "watches"
  | "library"
  | "notifications"
  | "mailbox"
  | "tasks"
  | "docs"
  | "sessions"
  | "worktrunk"
  | "board";

export const PINNABLE_SIDEBARS: ReadonlySet<SidebarId> = new Set<SidebarId>([
  "sessions",
  "notes",
  "watches",
  "library",
  "tasks",
  "notifications",
  "mailbox",
  "worktrunk",
  "board",
]);

interface SidebarState {
  pinned: SidebarId | null;
  active: SidebarId | null;
}

const PIN_STORAGE_KEY = "roux.sidebar.pin";

function loadInitialPin(): SidebarId | null {
  try {
    const raw =
      typeof window !== "undefined" && window.localStorage
        ? window.localStorage.getItem(PIN_STORAGE_KEY)
        : null;
    // First launch (no persisted state) → pin Sessions so the tab list is visible by default.
    if (!raw) return "sessions";
    const parsed = JSON.parse(raw) as { pinned?: SidebarId | null };
    const p = parsed.pinned ?? null;
    return p && PINNABLE_SIDEBARS.has(p) ? p : null;
  } catch {
    return "sessions";
  }
}

const sidebarState = writable<SidebarState>({
  pinned: loadInitialPin(),
  active: null,
});

sidebarState.subscribe((s) => {
  try {
    if (typeof window === "undefined" || !window.localStorage) return;
    window.localStorage.setItem(
      PIN_STORAGE_KEY,
      JSON.stringify({ pinned: s.pinned }),
    );
  } catch {}
});

export const activeSidebar: Readable<SidebarId | null> = derived(
  sidebarState,
  ($s) => $s.active,
);

export const pinnedSidebar: Readable<SidebarId | null> = derived(
  sidebarState,
  ($s) => $s.pinned,
);

/**
 * When non-null, the notes panel targets this session id instead of the
 * app's active session. Used by the sessions-history pane so clicking
 * "View notes" on an archived row scopes the notes view to that session
 * without changing the active session. Cleared automatically when the
 * user leaves the notes sidebar.
 */
export const notesOverrideSessionId = writable<string | null>(null);

function clearNotesOverrideIfLeaving(id: SidebarId | null): void {
  if (id !== "notes") notesOverrideSessionId.set(null);
}

export function openSidebar(id: SidebarId): void {
  // Opening a panel implies the user wants to see it — bring the dock back
  // if the sidebar is currently collapsed to icons.
  showSidebar();
  sidebarState.update((s) => {
    if (s.pinned === id) return s;
    return { ...s, active: id };
  });
  clearNotesOverrideIfLeaving(id);
}

export function closeSidebar(): void {
  sidebarState.update((s) => ({ ...s, active: null }));
  notesOverrideSessionId.set(null);
}

export function toggleSidebar(id: SidebarId): void {
  const s = get(sidebarState);
  if (s.pinned === id) {
    unpinSidebar();
    return;
  }
  if (s.active === id) {
    sidebarState.set({ ...s, active: null });
    clearNotesOverrideIfLeaving(null);
    return;
  }
  // Activating a panel implies the user wants to see it.
  showSidebar();
  sidebarState.set({ ...s, active: id });
  clearNotesOverrideIfLeaving(id);
}

export type StartupSidebarPreference =
  | "restore"
  | "sessions"
  | "kanban"
  | "none";

export function applyStartupSidebarPreference(
  preference: StartupSidebarPreference,
): void {
  if (preference === "restore") return;
  if (preference === "none") {
    sidebarState.set({ pinned: null, active: null });
    clearNotesOverrideIfLeaving(null);
    return;
  }
  const pinned: SidebarId = preference === "kanban" ? "board" : "sessions";
  showSidebar();
  sidebarState.set({ pinned, active: null });
  clearNotesOverrideIfLeaving(pinned);
}

export async function applyStartupTargetPreference(
  preference: StartupTarget | undefined,
  externalToolId?: string | null,
): Promise<void> {
  switch (preference ?? "restore") {
    case "restore":
      return;
    case "none":
      closeMainView();
      sidebarState.set({ pinned: null, active: null });
      clearNotesOverrideIfLeaving(null);
      return;
    case "sessionsSidebar":
      applyStartupSidebarPreference("sessions");
      return;
    case "kanbanWide":
      sidebarState.set({ pinned: null, active: null });
      clearNotesOverrideIfLeaving(null);
      openMainView({ kind: "board" });
      return;
    case "lastSession": {
      const newest = [...get(sessionState).sessions].sort(
        (a, b) => b.createdAt - a.createdAt,
      )[0];
      if (newest) {
        setActiveSession(newest.id);
        return;
      }
      applyStartupSidebarPreference("sessions");
      return;
    }
    case "externalTool":
      if (externalToolId) await openExternalTool(externalToolId);
      return;
  }
}

export function pinSidebar(id: SidebarId): void {
  if (!PINNABLE_SIDEBARS.has(id)) return;
  showSidebar();
  sidebarState.update((s) => ({
    pinned: id,
    active: s.active === id ? null : s.active,
  }));
}

export function unpinSidebar(): void {
  // Unpin = "stop forcing a split, collapse back to single view."
  // The previously-pinned panel is the user's anchor — it returns to the active
  // slot and the transient sibling drops away.
  //
  // Exception: when active is a non-pinnable takeover panel (Settings, Docs),
  // leave it alone. Otherwise unpinning Notes while Docs is open would
  // close Docs — jarring and not what the user asked for.
  sidebarState.update((s) => {
    const activeIsTakeover =
      s.active !== null && !PINNABLE_SIDEBARS.has(s.active);
    if (activeIsTakeover) {
      return { ...s, pinned: null };
    }
    return {
      pinned: null,
      active: s.pinned ?? s.active,
    };
  });
}

/**
 * Clear only the pinned slot without touching the active slot. Used by a
 * panel's own close (×) button — the user is asking to dismiss THIS panel,
 * not to collapse the split (which is `unpinSidebar`'s anchor-promotion).
 */
export function closePinned(): void {
  sidebarState.update((s) => ({ ...s, pinned: null }));
}

export function isPinned(id: SidebarId): boolean {
  return get(sidebarState).pinned === id;
}

export function openNotesForSession(sessionId: string): void {
  showSidebar();
  notesOverrideSessionId.set(sessionId);
  sidebarState.update((s) => ({ ...s, active: "notes" }));
}

/**
 * Id of the work item currently open in the card editor, or null when the
 * editor is closed. Kept here (not in the work-items data store) because it is
 * pure view state shared between the board surfaces and the editor modal.
 */
export const editingWorkItemId = writable<string | null>(null);

export interface NewWorkItemEditorRequest {
  status: WorkItemStatus;
  title?: string;
}

export const newWorkItemEditor = writable<NewWorkItemEditorRequest | null>(
  null,
);

export function openWorkItemEditor(id: string): void {
  newWorkItemEditor.set(null);
  editingWorkItemId.set(id);
}

export function openNewWorkItemEditor(request: NewWorkItemEditorRequest): void {
  editingWorkItemId.set(null);
  newWorkItemEditor.set(request);
}

export function closeWorkItemEditor(): void {
  editingWorkItemId.set(null);
  newWorkItemEditor.set(null);
}

export interface WorkItemSessionStartRequest {
  itemId: string;
  title: string;
}

export const workItemSessionStart =
  writable<WorkItemSessionStartRequest | null>(null);

export function openWorkItemSessionStart(
  request: WorkItemSessionStartRequest,
): void {
  workItemSessionStart.set(request);
}

export function closeWorkItemSessionStart(): void {
  workItemSessionStart.set(null);
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
  [sessionLayouts, activeSessionId],
  ([$layouts, $activeSessionId]) => {
    const map = new Map<string, number>();
    const activeId = $activeSessionId;
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
