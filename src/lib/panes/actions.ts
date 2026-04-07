import { get } from "svelte/store";
import { createPane, disposePane, getInstance, type CreatePaneOpts } from "./instances";
import {
  sessionLayouts,
  initSessionLayout,
  insertLeaf,
  removeLeaf,
  firstLeafId,
  collectLeafIds,
  type SplitDirection,
} from "./layout";
import { focusedPaneId, setLogicalFocus } from "./focus";

export function initSession(sessionId: string): string {
  const mainPaneId = `${sessionId}-main`;
  // Only create if not already exists
  if (!getInstance(mainPaneId)) {
    createPane({ id: mainPaneId, type: "claude", ptyId: sessionId });
  }
  initSessionLayout(sessionId, mainPaneId);
  setLogicalFocus(mainPaneId);
  return mainPaneId;
}

export function splitPane(
  sessionId: string,
  direction: SplitDirection,
  opts: Omit<CreatePaneOpts, "id">
): string | null {
  const newPaneId = createPane(opts);

  let inserted = false;
  sessionLayouts.update((m) => {
    const tree = m.get(sessionId);
    if (!tree) return m;
    const focused = get(focusedPaneId);
    m.set(sessionId, insertLeaf(tree, focused, direction, newPaneId));
    inserted = true;
    return new Map(m);
  });

  if (!inserted) {
    disposePane(newPaneId);
    return null;
  }

  setLogicalFocus(newPaneId);
  return newPaneId;
}

export function closePane(sessionId: string, paneId: string): boolean {
  const instance = getInstance(paneId);
  if (!instance) return false;

  // Don't close the main claude pane
  if (instance.type === "claude" && instance.id === `${sessionId}-main`) {
    return false;
  }

  sessionLayouts.update((m) => {
    const tree = m.get(sessionId);
    if (!tree) return m;
    const result = removeLeaf(tree, paneId);
    if (result) m.set(sessionId, result);
    else m.delete(sessionId);
    return new Map(m);
  });

  disposePane(paneId);

  if (get(focusedPaneId) === paneId) {
    const tree = get(sessionLayouts).get(sessionId);
    setLogicalFocus(tree ? firstLeafId(tree) : null);
  }

  return true;
}

export function closeFocusedPane(sessionId: string): boolean {
  const paneId = get(focusedPaneId);
  if (!paneId) return false;
  return closePane(sessionId, paneId);
}

export function closeSessionPanes(sessionId: string) {
  const tree = get(sessionLayouts).get(sessionId);
  if (tree) {
    const ids = collectLeafIds(tree);
    for (const id of ids) disposePane(id);
  }
  sessionLayouts.update((m) => {
    m.delete(sessionId);
    return new Map(m);
  });
  // Clear focus if it was in this session
  const focused = get(focusedPaneId);
  if (focused && tree && collectLeafIds(tree).includes(focused)) {
    setLogicalFocus(null);
  }
}
