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
  createWatch,
  onSessionExit,
  writeToSession,
  type SessionExitPayload,
} from "$lib/tauri";
import type { CreateWatchConfig } from "$lib/types";
import { getInstance, updateInstance } from "./instances";
import { focusedPaneId } from "./focus";
import { log } from "$lib/logging";
import { sessionState } from "$lib/stores/sessions";

/**
 * Create an xterm Terminal + FitAddon for a pane and store them on the
 * instance.  No-ops if the instance already has a terminal or is a
 * markdown pane.
 */
export function initTerminal(paneId: string): void {
  const instance = getInstance(paneId);
  if (!instance || instance.terminal || instance.type === "markdown") {
    log(`initTerminal(${paneId}): skipped (exists=${!!instance}, hasTerm=${!!instance?.terminal}, type=${instance?.type})`);
    return;
  }
  log(`initTerminal(${paneId}): creating terminal for type=${instance.type} ptyId=${instance.ptyId}`);

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

  terminal.registerLinkProvider({
    provideLinks(bufferLineNumber, callback) {
      const line = terminal.buffer.active.getLine(bufferLineNumber);
      if (!line) {
        callback(undefined);
        return;
      }
      const text = line.translateToString();

      const ghPattern = /https:\/\/github\.com\/([^/]+\/[^/]+)\/actions\/runs\/(\d+)/g;
      const links: { startIndex: number; length: number; repo: string; runId: number }[] = [];

      let match;
      while ((match = ghPattern.exec(text)) !== null) {
        links.push({
          startIndex: match.index,
          length: match[0].length,
          repo: match[1],
          runId: parseInt(match[2], 10),
        });
      }

      callback(
        links.map((link) => ({
          range: {
            start: { x: link.startIndex + 1, y: bufferLineNumber },
            end: { x: link.startIndex + link.length + 1, y: bufferLineNumber },
          },
          text: "Click to watch this GitHub Action",
          activate: async () => {
            const state = get(sessionState);
            const config: CreateWatchConfig = {
              name: `GH: ${link.repo} #${link.runId}`,
              kind: {
                type: "githubAction",
                repo: link.repo,
                runId: link.runId,
                workflow: null,
                branch: null,
              },
              mode: { type: "oneShot" },
              scope: state.activeSessionId
                ? { type: "session", sessionId: state.activeSessionId }
                : { type: "global" },
            };
            await createWatch(config);
          },
        }))
      );
    },
  });

  terminal.onData((data) => {
    const inst = getInstance(paneId);
    if (!inst) return;
    writeToSession(inst.ptyId, data).catch((e) => {
      log(`Write failed for ${inst.ptyId}: ${e}`);
    });
  });

  // Reconcile disableStdin with current logical focus — focus may have been
  // set before the terminal existed (e.g. initSession → setLogicalFocus
  // runs before initTerminal).
  const currentFocused = get(focusedPaneId);
  terminal.options.disableStdin = paneId !== currentFocused;

  updateInstance(paneId, { terminal, fitAddon });
}

/**
 * Wire up PTY output and (optionally) an exit handler for the pane's
 * current ptyId.  Pushes unlisteners onto the instance so they are
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
  log(`attachPtyListeners(${paneId}): ptyId=${instance.ptyId} hasChannel=${!!instance.outputChannel} hasTerm=${!!instance.terminal}`);

  // Capture the ptyId we're attaching to — if it changes between awaits,
  // a replacePty/rerun happened and we should bail.
  const targetPtyId = instance.ptyId;

  if (onExit) {
    const unlisten = await onSessionExit(targetPtyId, onExit);
    // Re-check: pane may have been disposed or ptyId replaced during await
    const current = getInstance(paneId);
    if (!current || current.ptyId !== targetPtyId) {
      unlisten();
      return;
    }
    current.unlisteners.push(unlisten);
  }

  // Re-check after exit listener await
  const inst2 = getInstance(paneId);
  if (!inst2 || inst2.ptyId !== targetPtyId) return;

  if (!inst2.outputChannel) {
    const channel = createPtyOutputChannel((bytes) => {
      // Re-read instance in case it was replaced (reconnect, rerun)
      const inst = getInstance(paneId);
      inst?.terminal?.write(bytes);
    });
    updateInstance(paneId, { outputChannel: channel });

    // Final re-check before attaching output
    const inst3 = getInstance(paneId);
    if (!inst3 || inst3.ptyId !== targetPtyId) return;
    await attachPtyOutput(targetPtyId, channel);
  } else {
    await attachPtyOutput(targetPtyId, inst2.outputChannel);
  }
}
