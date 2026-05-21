import { writable, get } from "svelte/store";
import { paneInstances } from "./instances";
import { getTerminalController } from "./terminalRuntime";
import { log } from "$lib/logging";

export const focusedPaneId = writable<string | null>(null);
export const fullscreenPaneId = writable<string | null>(null);

/**
 * Set logical focus: which pane owns keyboard input.
 * Updates disableStdin on all terminals and moves DOM focus to the target
 * terminal so keyboard input is routed correctly after programmatic
 * navigation (e.g. Alt+H/J/K/L).
 */
function activeElementSummary(): string {
  if (typeof document === "undefined") return "no-document";
  const el = document.activeElement;
  if (!(el instanceof HTMLElement)) return "none";
  const pane = el.closest("[data-pane-id]")?.getAttribute("data-pane-id") ?? "none";
  const cls = typeof el.className === "string" ? el.className : "";
  return `${el.tagName.toLowerCase()}#${el.id || "-"} pane=${pane} class=${cls.slice(0, 80)}`;
}

export function setLogicalFocus(paneId: string | null, source = "unknown") {
  const previous = get(focusedPaneId);
  const instances = get(paneInstances);
  log(
    `[pane-input] setLogicalFocus source=${source} from=${previous ?? "null"} to=${paneId ?? "null"} ` +
      `instances=${instances.size} activeBefore=${activeElementSummary()}`,
  );
  focusedPaneId.set(paneId);
  for (const [id, pane] of instances) {
    const controller = getTerminalController(id);
    const enabled = id === paneId;
    log(
      `[pane-input] route pane=${id} pty=${pane.ptyId} type=${pane.type} hasTerm=${!!controller} enabled=${enabled}`,
    );
    controller?.setInputEnabled(enabled);
  }
  if (paneId) {
    const controller = getTerminalController(paneId);
    log(`[pane-input] focus pane=${paneId} hasTerm=${!!controller}`);
    controller?.focus();
    queueMicrotask(() => {
      log(`[pane-input] focus.after pane=${paneId} activeAfter=${activeElementSummary()}`);
    });
  }
}

/**
 * Request DOM focus on a pane's terminal.
 * Only call from pointer event handlers (mousedown, click).
 */
export function requestDomFocus(paneId: string) {
  setLogicalFocus(paneId, "requestDomFocus");
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
