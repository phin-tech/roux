import { get } from "svelte/store";
import { sessionState } from "$lib/stores/sessions";
import { paneTrees, focusedPaneId, hasSplitPanes, getPane } from "$lib/stores/panes";

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
    return get(paneTrees).get(id) ?? null;
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
    const sessionId = this.activeSessionId();
    const paneId = this.focusedPaneId();
    if (!sessionId || !paneId) return null;
    return getPane(sessionId, paneId);
  },

  hasAttentionSession() {
    return this.sessions().some((s) => s.status === "attention");
  },
};
