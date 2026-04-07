/**
 * Terminal initialization and PTY listener attachment.
 *
 * Separated from instances.ts so that the xterm dependency does NOT pollute
 * test-only imports of the instance store.  Callers (commands/index.ts,
 * PaneShell.svelte, etc.) import this module when they actually need a
 * real terminal.
 */

import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { get } from "svelte/store";
import { settings } from "$lib/stores/settings";
import { getXtermTheme } from "$lib/themes";
import {
  attachPtyOutput,
  createPtyOutputChannel,
  onSessionExit,
  writeToSession,
  type SessionExitPayload,
} from "$lib/tauri";
import { getInstance, updateInstance } from "./instances";
import { log } from "$lib/logging";

/**
 * Create an xterm Terminal + FitAddon for a pane and store them on the
 * instance.  No-ops if the instance already has a terminal or is a
 * markdown pane.
 */
export function initTerminal(paneId: string): void {
  const instance = getInstance(paneId);
  if (!instance || instance.terminal || instance.type === "markdown") return;

  const s = get(settings);
  const terminal = new Terminal({
    fontSize: s.fontSize,
    fontFamily: s.fontFamily,
    lineHeight: s.lineHeight,
    scrollback: s.scrollback,
    cursorStyle: s.cursorStyle as "block" | "underline" | "bar",
    cursorBlink: s.cursorBlink,
    theme: getXtermTheme(s.theme),
    disableStdin: true,
  });

  const fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);
  try {
    terminal.loadAddon(new WebglAddon());
  } catch {
    // WebGL not available — canvas fallback
  }
  terminal.loadAddon(new WebLinksAddon());

  terminal.onData((data) => {
    const inst = getInstance(paneId);
    if (!inst) return;
    writeToSession(inst.ptyId, data).catch((e) => {
      log(`Write failed for ${inst.ptyId}: ${e}`);
    });
  });

  updateInstance(paneId, { terminal, fitAddon });
}

/**
 * Wire up PTY output and (optionally) an exit handler for the pane's
 * current ptyId.  Pushes unlisteners onto the instance so they are
 * cleaned up automatically by disposePane.
 */
export async function attachPtyListeners(
  paneId: string,
  onExit?: (payload: SessionExitPayload) => void,
): Promise<void> {
  const instance = getInstance(paneId);
  if (!instance) return;

  if (onExit) {
    const unlisten = await onSessionExit(instance.ptyId, onExit);
    instance.unlisteners.push(unlisten);
  }

  if (!instance.outputChannel) {
    const channel = createPtyOutputChannel((bytes) => {
      // Re-read instance in case it was replaced (reconnect, rerun)
      const inst = getInstance(paneId);
      inst?.terminal?.write(bytes);
    });
    updateInstance(paneId, { outputChannel: channel });
    await attachPtyOutput(instance.ptyId, channel);
  } else {
    await attachPtyOutput(instance.ptyId, instance.outputChannel);
  }
}
