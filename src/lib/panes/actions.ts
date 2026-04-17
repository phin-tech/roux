import { get } from "svelte/store";
import {
  createPane,
  disposePane,
  getInstance,
  registerDisposeHook,
  type CreatePaneOpts,
} from "./instances";
import {
  sessionLayouts,
  initSessionLayout,
  insertLeaf,
  removeLeaf,
  firstLeafId,
  collectLeafIds,
  collectVisibleLeafIds,
  containsPaneId,
  type SplitDirection,
} from "./layout";
import { focusedPaneId, fullscreenPaneId, setLogicalFocus } from "./focus";
import { disposeAgentState } from "./agentState";
import { forgetLastStatus } from "./agentNotifications";
import type { SpawnProfileRef } from "./profiles";
import { killPty } from "$lib/tauri";

// Register cleanup hooks on disposePane so every path that disposes a
// pane (closePane, closeSessionPanes, splitPane rollback, anything
// future) also clears downstream state. Hooks live here instead of in
// instances.ts to avoid a circular dep (instances → agentState → layout
// → instances).
registerDisposeHook(disposeAgentState);
registerDisposeHook(forgetLastStatus);

export function initSession(sessionId: string): string {
  return initSessionWithProfile(sessionId, { kind: "registered", id: "claude" });
}

/**
 * Create the session's primary pane with a specific spawn profile ref.
 * Used by the new-session dialog after the user picks a profile. Call
 * `initSession` directly for the default Claude path (restore flows, etc).
 */
export function initSessionWithProfile(
  sessionId: string,
  spawnProfileRef: SpawnProfileRef,
  nono?: { nonoProfile?: string; nonoAllowDirs?: string[] },
): string {
  const mainPaneId = `${sessionId}-main`;
  if (!getInstance(mainPaneId)) {
    createPane({
      id: mainPaneId,
      type: "shell",
      ptyId: sessionId,
      spawnProfileRef,
      nonoProfile: nono?.nonoProfile,
      nonoAllowDirs: nono?.nonoAllowDirs,
    });
  }
  initSessionLayout(sessionId, mainPaneId);
  setLogicalFocus(mainPaneId);
  return mainPaneId;
}

export function splitPane(
  sessionId: string,
  direction: SplitDirection,
  opts: CreatePaneOpts
): string | null {
  const newPaneId = createPane(opts);

  let inserted = false;
  sessionLayouts.update((m) => {
    const tree = m.get(sessionId);

    // Zero-pane recovery: when the session's last pane was closed, the
    // layout entry is gone. Seed a fresh single-leaf layout with the new
    // pane as the sole leaf instead of dropping the split on the floor.
    // The spec explicitly allows zero-pane sessions as a transient state,
    // so splitting into one must re-populate it.
    if (!tree) {
      m.set(sessionId, { kind: "leaf", paneId: newPaneId });
      inserted = true;
      return new Map(m);
    }

    // Fix #4: Ensure focused pane belongs to this session's layout.
    // If focus is on a different session, fall back to the first leaf.
    let targetId = get(focusedPaneId);
    if (!targetId || !containsPaneId(tree, targetId)) {
      targetId = firstLeafId(tree);
    }

    const updated = insertLeaf(tree, targetId, direction, newPaneId);

    // Verify the new pane was actually inserted
    if (!containsPaneId(updated, newPaneId)) return m;

    m.set(sessionId, updated);
    inserted = true;
    return new Map(m);
  });

  if (!inserted) {
    disposePane(newPaneId, killPty);
    return null;
  }

  setLogicalFocus(newPaneId);
  return newPaneId;
}

export function closePane(sessionId: string, paneId: string): boolean {
  const instance = getInstance(paneId);
  if (!instance) return false;

  sessionLayouts.update((m) => {
    const tree = m.get(sessionId);
    if (!tree) return m;
    const result = removeLeaf(tree, paneId);
    if (result) m.set(sessionId, result);
    else m.delete(sessionId);
    return new Map(m);
  });

  disposePane(paneId, killPty);

  // A fullscreened pane that is now closed must release the fullscreen
  // slot — otherwise SplitPane keeps filtering every sibling out of the
  // DOM against a dead paneId and the content area goes blank.
  if (get(fullscreenPaneId) === paneId) {
    fullscreenPaneId.set(null);
  }

  if (get(focusedPaneId) === paneId) {
    const tree = get(sessionLayouts).get(sessionId);
    // Prefer the first *visible* leaf so we don't hand focus to a pane
    // hidden behind a stacked tab's `activeIndex`.
    const nextFocus = tree
      ? (collectVisibleLeafIds(tree)[0] ?? firstLeafId(tree))
      : null;
    setLogicalFocus(nextFocus);
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
    for (const id of ids) disposePane(id, killPty);
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
