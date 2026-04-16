/**
 * Terminal initialization and PTY listener attachment.
 *
 * Owns the bridge between pane metadata state and the frontend-only
 * terminal runtime registry. PaneInstance stays free of renderer internals.
 */

import { get } from "svelte/store";
import {
  attachPtyOutput,
  createPtyOutputChannel,
  onSessionExit,
  writeToSession,
  type SessionExitPayload,
} from "$lib/tauri";
import { keymapState } from "$lib/keymap/store";
import { resolveKey } from "$lib/keymap/resolve";
import { registry as commandRegistry } from "$lib/commands";
import { focusedPaneId } from "./focus";
import { getInstance } from "./instances";
import { emitPtyOutput } from "./ptyOutputBus";
import {
  ensureTerminalController,
  getPaneOutputChannel,
  getTerminalController,
  setPaneOutputChannel,
} from "./terminalRuntime";
import { log } from "$lib/logging";

/**
 * Create a terminal controller for a pane. No-ops if the controller already
 * exists or the pane is markdown-only.
 */
export function initTerminal(paneId: string): void {
  const instance = getInstance(paneId);
  const existing = getTerminalController(paneId);
  if (!instance || existing || instance.type === "markdown") {
    log(`initTerminal(${paneId}): skipped (exists=${!!instance}, hasTerm=${!!existing}, type=${instance?.type})`);
    return;
  }
  log(`initTerminal(${paneId}): creating terminal for type=${instance.type} ptyId=${instance.ptyId}`);

  const controller = ensureTerminalController(paneId, {
    allowKeyboardEvent: (event) => {
      if (event.type !== "keydown") return true;
      const km = get(keymapState);
      const resolution = resolveKey(event, km, (id) => {
        const cmd = commandRegistry.get(id);
        return !!cmd && (!cmd.available || cmd.available());
      });
      return resolution.kind === "none" || resolution.kind === "passthrough";
    },
  });
  controller.onInput((data) => {
    const inst = getInstance(paneId);
    if (!inst) return;
    writeToSession(inst.ptyId, data).catch((e) => {
      log(`Write failed for ${inst.ptyId}: ${e}`);
    });
  });

  const currentFocused = get(focusedPaneId);
  controller.setInputEnabled(paneId === currentFocused);
}

/**
 * Initialize the pane's terminal controller, then attach its current PTY.
 * This is the normal happy-path entrypoint for panes that should be both
 * renderable and wired to live PTY output.
 */
export async function connectPaneTerminal(
  paneId: string,
  onExit?: (payload: SessionExitPayload) => void,
): Promise<void> {
  initTerminal(paneId);
  await attachPtyListeners(paneId, onExit);
}

/**
 * Wire up PTY output and (optionally) an exit handler for the pane's
 * current ptyId. Pushes unlisteners onto the pane instance so they are
 * cleaned up automatically by disposePane.
 *
 * Re-checks instance existence and ptyId stability after each async
 * boundary to guard against rapid close/reconnect/rerun races.
 */
export async function attachPtyListeners(
  paneId: string,
  onExit?: (payload: SessionExitPayload) => void,
): Promise<void> {
  const instance = getInstance(paneId);
  if (!instance) {
    log(`attachPtyListeners(${paneId}): no instance found, bailing`);
    return;
  }
  log(`attachPtyListeners(${paneId}): ptyId=${instance.ptyId} hasChannel=${!!getPaneOutputChannel(paneId)} hasTerm=${!!getTerminalController(paneId)}`);

  const targetPtyId = instance.ptyId;

  if (onExit) {
    const unlisten = await onSessionExit(targetPtyId, onExit);
    const current = getInstance(paneId);
    if (!current || current.ptyId !== targetPtyId) {
      unlisten();
      return;
    }
    current.unlisteners.push(unlisten);
  }

  const inst2 = getInstance(paneId);
  if (!inst2 || inst2.ptyId !== targetPtyId) return;

  let outputChannel = getPaneOutputChannel(paneId);
  if (!outputChannel) {
    outputChannel = createPtyOutputChannel((bytes) => {
      emitPtyOutput(targetPtyId, bytes);
      getTerminalController(paneId)?.write(bytes);
    });
    setPaneOutputChannel(paneId, outputChannel);
  }

  const inst3 = getInstance(paneId);
  if (!inst3 || inst3.ptyId !== targetPtyId) return;
  await attachPtyOutput(targetPtyId, outputChannel);
}
