import { writable } from "svelte/store";
import type { DropSide } from "$lib/panes/layout";

/** The pane currently being dragged, or null. */
export const draggedPaneId = writable<string | null>(null);

/** Which pane + side is currently hovered as a drop target. */
export const dropTarget = writable<{ paneId: string; side: DropSide } | null>(null);

/** Clear all transient pane drag state. */
export function resetPaneDrag(): void {
  draggedPaneId.set(null);
  dropTarget.set(null);
}
