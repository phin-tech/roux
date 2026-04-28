import { get } from "svelte/store";
import { writeToSession } from "$lib/tauri";
import { paneInstances, getAttachedPtyId } from "$lib/panes/instances";
import { focusedPaneId } from "$lib/panes/focus";
import { paneAtPoint } from "./paneAtPoint";
import { formatPathsForTerminal } from "./formatPaths";

export interface FileDropEvent {
  paths: readonly string[];
  position: { x: number; y: number };
}

function ptyForPaneId(paneId: string | null): string | null {
  if (!paneId) return null;
  const pane = get(paneInstances).get(paneId);
  if (!pane) return null;
  return getAttachedPtyId(pane);
}

export async function handleFileDrop(event: FileDropEvent): Promise<void> {
  if (event.paths.length === 0) return;

  const hitPaneId = paneAtPoint(event.position.x, event.position.y);
  const ptyId =
    ptyForPaneId(hitPaneId) ?? ptyForPaneId(get(focusedPaneId));
  if (!ptyId) return;

  await writeToSession(ptyId, formatPathsForTerminal(event.paths));
}
