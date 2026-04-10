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
import { openUrl } from "@tauri-apps/plugin-opener";
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
    allowProposedApi: true,
  });

  const fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);
  try {
    terminal.loadAddon(new WebglAddon());
  } catch {
    // WebGL not available — canvas fallback
  }
  terminal.loadAddon(new WebLinksAddon((_event, uri) => {
    openUrl(uri);
  }));

  // Add watch buttons next to GitHub URLs as they appear
  const ghRunPattern = /https:\/\/github\.com\/([^/]+\/[^/]+)\/actions\/runs\/(\d+)/g;
  const ghPrPattern = /https:\/\/github\.com\/([^/]+\/[^/]+)\/pull\/(\d+)/g;
  const decoratedLines = new Set<string>(); // "lineNum:pattern" keys

  function addWatchDecoration(
    yOffset: number,
    urlEnd: number,
    title: string,
    onClick: () => Promise<void>,
  ) {
    const marker = terminal.registerMarker(yOffset);
    if (!marker) return;
    const decoration = terminal.registerDecoration({
      marker,
      anchor: "left",
      x: urlEnd + 1,
      width: 3,
    });
    if (!decoration) return;
    decoration.onRender((el) => {
      if (el.dataset.initialized) return;
      el.dataset.initialized = "true";
      el.innerHTML = `<svg title="${title}" style="cursor:pointer;opacity:0.7;" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>`;
      el.style.display = "inline-flex";
      el.style.alignItems = "center";
      el.style.height = "100%";
      el.style.zIndex = "10";
      el.addEventListener("mouseenter", () => {
        el.firstElementChild!.setAttribute("style", el.firstElementChild!.getAttribute("style")!.replace("opacity:0.7", "opacity:1"));
      });
      el.addEventListener("mouseleave", () => {
        el.firstElementChild!.setAttribute("style", el.firstElementChild!.getAttribute("style")!.replace("opacity:1", "opacity:0.7"));
      });
      el.addEventListener("click", async (e) => {
        e.stopPropagation();
        e.preventDefault();
        await onClick();
      });
    });
  }

  terminal.onWriteParsed(() => {
    const buf = terminal.buffer.active;
    for (let i = Math.max(0, buf.cursorY - 2); i <= buf.cursorY; i++) {
      const absLine = buf.baseY + i;
      const line = buf.getLine(absLine);
      if (!line) continue;
      const text = line.translateToString();
      const yOffset = i - buf.cursorY;

      // GitHub Actions run URLs
      const runKey = `${absLine}:run`;
      if (!decoratedLines.has(runKey)) {
        ghRunPattern.lastIndex = 0;
        let match;
        while ((match = ghRunPattern.exec(text)) !== null) {
          decoratedLines.add(runKey);
          const repo = match[1];
          const runId = parseInt(match[2], 10);
          addWatchDecoration(yOffset, match.index + match[0].length, "Watch this GitHub Action", async () => {
            const state = get(sessionState);
            const config: CreateWatchConfig = {
              name: `GH: ${repo} #${runId}`,
              kind: { type: "githubAction", repo, runId, workflow: null, branch: null },
              mode: { type: "oneShot" },
              scope: state.activeSessionId
                ? { type: "session", sessionId: state.activeSessionId }
                : { type: "global" },
              notify: null,
            };
            await createWatch(config);
          });
        }
      }

      // GitHub PR URLs
      const prKey = `${absLine}:pr`;
      if (!decoratedLines.has(prKey)) {
        ghPrPattern.lastIndex = 0;
        let match;
        while ((match = ghPrPattern.exec(text)) !== null) {
          decoratedLines.add(prKey);
          const repo = match[1];
          const prNumber = parseInt(match[2], 10);
          addWatchDecoration(yOffset, match.index + match[0].length, "Watch this PR", async () => {
            const state = get(sessionState);
            const config: CreateWatchConfig = {
              name: `PR: ${repo} #${prNumber}`,
              kind: { type: "githubPr", repo, prNumber },
              mode: { type: "recurring", intervalSecs: 30 },
              scope: state.activeSessionId
                ? { type: "session", sessionId: state.activeSessionId }
                : { type: "global" },
              notify: null,
            };
            await createWatch(config);
          });
        }
      }
    }
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
