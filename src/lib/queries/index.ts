import { get } from "svelte/store";
import { sessionState } from "$lib/stores/sessions";
import { sessionLayouts, hasSplitPanes } from "$lib/panes/layout";
import { focusedPaneId } from "$lib/panes/focus";
import { getInstance } from "$lib/panes/instances";

export const queries = {
  activeSession() {
    const state = get(sessionState);
    return state.sessions.find((s) => s.id === state.activeSessionId) ?? null;
  },

  sessions() {
    return get(sessionState).sessions;
  },

  activeSessionId() {
    return get(sessionState).activeSessionId;
  },

  activePaneTree() {
    const id = this.activeSessionId();
    if (!id) return null;
    return get(sessionLayouts).get(id) ?? null;
  },

  focusedPaneId() {
    return get(focusedPaneId);
  },

  canSplitPane() {
    return !!this.activeSessionId();
  },

  canClosePane() {
    const id = this.activeSessionId();
    if (!id) return false;
    const focused = this.focusedPaneId();
    if (!focused) return false;
    return focused !== id + "-main" && hasSplitPanes(id);
  },

  focusedPane() {
    const paneId = this.focusedPaneId();
    if (!paneId) return null;
    return getInstance(paneId) ?? null;
  },
};
