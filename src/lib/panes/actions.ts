import { get } from "svelte/store";
import {
  focusedPaneId,
  getPane,
  listPanes,
  removePane,
} from "$lib/stores/panes";
import { disposeShellTerminal } from "./terminalRegistry";

export interface PaneCloseDeps {
  cleanupShellPane?: (paneId: string, ptyId: string) => Promise<void> | void;
}

const defaultDeps: Required<PaneCloseDeps> = {
  cleanupShellPane: (paneId) => disposeShellTerminal(paneId),
};

export async function closePane(
  sessionId: string,
  paneId: string,
  deps: PaneCloseDeps = defaultDeps
): Promise<boolean> {
  const pane = getPane(sessionId, paneId);
  if (!pane) return false;
  if (pane.type === "claude" && pane.id === `${sessionId}-main`) {
    return false;
  }

  if (pane.type === "shell") {
    await deps.cleanupShellPane?.(pane.id, pane.ptyId);
  }

  removePane(sessionId, paneId);
  return true;
}

export async function closeFocusedPane(
  sessionId: string,
  deps: PaneCloseDeps = defaultDeps
): Promise<boolean> {
  const paneId = get(focusedPaneId);
  if (!paneId) return false;
  return closePane(sessionId, paneId, deps);
}

export async function closeAuxiliaryPanes(
  sessionId: string,
  deps: PaneCloseDeps = defaultDeps
) {
  const panes = listPanes(sessionId).filter(
    (pane) => !(pane.type === "claude" && pane.id === `${sessionId}-main`)
  );

  for (const pane of panes) {
    await closePane(sessionId, pane.id, deps);
  }
}
