import { writable, get } from "svelte/store";
import { paneInstances } from "./instances";

export const focusedPaneId = writable<string | null>(null);
export const fullscreenPaneId = writable<string | null>(null);

/**
 * Set logical focus: which pane owns keyboard input.
 * Updates disableStdin on all terminals and moves DOM focus to the target
 * terminal so keyboard input is routed correctly after programmatic
 * navigation (e.g. Alt+H/J/K/L).
 */
export function setLogicalFocus(paneId: string | null) {
  focusedPaneId.set(paneId);
  const instances = get(paneInstances);
  for (const [id, instance] of instances) {
    if (!instance.terminal) continue;
    instance.terminal.options.disableStdin = id !== paneId;
  }
  if (paneId) {
    instances.get(paneId)?.terminal?.focus();
  }
}

/**
 * Request DOM focus on a pane's terminal.
 * Only call from pointer event handlers (mousedown, click).
 */
export function requestDomFocus(paneId: string) {
  const instances = get(paneInstances);
  instances.get(paneId)?.terminal?.focus();
}

/** Toggle fullscreen for the focused pane. */
export function toggleFullscreen() {
  const focused = get(focusedPaneId);
  if (!focused) return;
  const current = get(fullscreenPaneId);
  fullscreenPaneId.set(current === focused ? null : focused);
}

/** Reset stores — for tests only. */
export function resetFocus() {
  focusedPaneId.set(null);
  fullscreenPaneId.set(null);
}
