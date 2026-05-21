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
import { getAttachedPtyId, getInstance } from "./instances";
import { emitPtyOutput } from "./ptyOutputBus";
import {
  ensureTerminalController,
  getPaneOutputChannel,
  getTerminalController,
  setPaneOutputChannel,
} from "./terminalRuntime";
import { log } from "$lib/logging";

function inputPreview(data: string): string {
  const escaped = data
    .replace(/\r/g, "\\r")
    .replace(/\n/g, "\\n")
    .replace(/\t/g, "\\t")
    .replace(/\u001b/g, "\\e");
  return escaped.length > 24 ? `${escaped.slice(0, 24)}...` : escaped;
}

function outputPreview(bytes: Uint8Array): string {
  const slice = bytes.slice(0, 24);
  const chars = Array.from(slice, (byte) => {
    if (byte === 13) return "\\r";
    if (byte === 10) return "\\n";
    if (byte === 9) return "\\t";
    if (byte === 27) return "\\e";
    if (byte < 32 || byte > 126) return `\\x${byte.toString(16).padStart(2, "0")}`;
    return String.fromCharCode(byte);
  }).join("");
  return bytes.length > slice.length ? `${chars}...` : chars;
}

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
        if (id === "pane.open-multiline-editor") {
          const pane = getInstance(paneId);
          return !!cmd && !!pane && (pane.type === "shell" || pane.type === "command") && !!getAttachedPtyId(pane);
        }
        return !!cmd && (!cmd.available || cmd.available());
      });
      // App.svelte's window-capture handler preventDefaults for two
      // very different reasons:
      //   (a) Escape focus-blur fix when there's a focused terminal —
      //       we still want xterm to forward Escape to the PTY.
      //   (b) Anything its keymap dispatch path swallowed: chord /
      //       enterTree / drillInto / exit, or `none` while a tree is
      //       armed (resolve.ts §1e). xterm must not also process or
      //       double-fire those.
      // A blanket `defaultPrevented → false` swallows (a); a guard that
      // only checks `chord` leaks (b)'s tree-armed `none` keys to the
      // PTY mid-chord. Distinguish by resolution: only `passthrough` and
      // `none` with no armed tree are App-untouched and may proceed.
      const treeArmed = km.treePath.length > 0;
      const appUntouched =
        resolution.kind === "passthrough" ||
        (resolution.kind === "none" && !treeArmed);
      if (event.defaultPrevented && !appUntouched) return false;
      if (
        resolution.kind === "chord" &&
        resolution.action.kind === "command" &&
        resolution.action.id === "pane.open-multiline-editor"
      ) {
        // Returning false from xterm's allowKeyboardEvent stops xterm from
        // processing the key but does not preventDefault on the underlying
        // DOM event. Stop it here so the keypress can't reach a later
        // bubble-phase handler or the browser default action.
        event.preventDefault();
        event.stopPropagation();
        const cmd = commandRegistry.get(resolution.action.id);
        focusedPaneId.set(paneId);
        if (cmd?.execute) void cmd.execute();
        return false;
      }
      return resolution.kind === "none" || resolution.kind === "passthrough";
    },
  });
  controller.onInput((data) => {
    const inst = getInstance(paneId);
    if (!inst) {
      log(`[pane-input] onData pane=${paneId} dropped=no-instance bytes=${data.length} data=${inputPreview(data)}`);
      return;
    }
    log(
      `[pane-input] onData pane=${paneId} pty=${inst.ptyId} bytes=${data.length} data=${inputPreview(data)}`,
    );
    writeToSession(inst.ptyId, data)
      .then(() => {
        log(`[pane-input] writeToSession.ok pane=${paneId} pty=${inst.ptyId} bytes=${data.length}`);
      })
      .catch((e) => {
        log(`[pane-input] writeToSession.err pane=${paneId} pty=${inst.ptyId} error=${e}`);
        log(`Write failed for ${inst.ptyId}: ${e}`);
      });
  });

  const currentFocused = get(focusedPaneId);
  log(
    `[pane-input] initTerminal.inputState pane=${paneId} pty=${instance.ptyId} focused=${currentFocused ?? "null"} ` +
      `enabled=${paneId === currentFocused}`,
  );
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
      log(
        `[pane-input] output pane=${paneId} pty=${targetPtyId} bytes=${bytes.length} ` +
          `data=${outputPreview(bytes)} hasTerm=${!!getTerminalController(paneId)}`,
      );
      emitPtyOutput(targetPtyId, bytes);
      getTerminalController(paneId)?.write(bytes);
    });
    setPaneOutputChannel(paneId, outputChannel);
  }

  const inst3 = getInstance(paneId);
  if (!inst3 || inst3.ptyId !== targetPtyId) return;
  await attachPtyOutput(targetPtyId, outputChannel);
}
