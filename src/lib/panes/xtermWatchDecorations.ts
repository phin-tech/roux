import type { Terminal } from "@xterm/xterm";
import { get } from "svelte/store";

import { sessionState } from "$lib/stores/sessions";
import { createWatch } from "$lib/tauri";
import type { CreateWatchConfig } from "$lib/types";

const GH_ACTION_PATTERN = /https:\/\/github\.com\/([^/]+\/[^/]+)\/actions\/runs\/(\d+)/g;
const GH_PR_PATTERN = /https:\/\/github\.com\/([^/]+\/[^/]+)\/pull\/(\d+)/g;

export type WatchTarget =
  | {
      kind: "githubAction";
      repo: string;
      runId: number;
      urlEnd: number;
    }
  | {
      kind: "githubPr";
      repo: string;
      prNumber: number;
      urlEnd: number;
    };

interface InstallXtermWatchDecorationOptions {
  createWatch?: (config: CreateWatchConfig) => Promise<unknown>;
  getActiveSessionId?: () => string | null;
}

export function findWatchTargetsInText(text: string): WatchTarget[] {
  const targets: WatchTarget[] = [];

  GH_ACTION_PATTERN.lastIndex = 0;
  let actionMatch: RegExpExecArray | null;
  while ((actionMatch = GH_ACTION_PATTERN.exec(text)) !== null) {
    targets.push({
      kind: "githubAction",
      repo: actionMatch[1],
      runId: parseInt(actionMatch[2], 10),
      urlEnd: actionMatch.index + actionMatch[0].length,
    });
  }

  GH_PR_PATTERN.lastIndex = 0;
  let prMatch: RegExpExecArray | null;
  while ((prMatch = GH_PR_PATTERN.exec(text)) !== null) {
    targets.push({
      kind: "githubPr",
      repo: prMatch[1],
      prNumber: parseInt(prMatch[2], 10),
      urlEnd: prMatch.index + prMatch[0].length,
    });
  }

  return targets.sort((a, b) => a.urlEnd - b.urlEnd);
}

export function buildWatchConfigForTarget(
  target: WatchTarget,
  activeSessionId: string | null,
): CreateWatchConfig {
  const scope = activeSessionId
    ? { type: "session" as const, sessionId: activeSessionId }
    : { type: "global" as const };

  if (target.kind === "githubAction") {
    return {
      name: `GH: ${target.repo} #${target.runId}`,
      kind: {
        type: "githubAction",
        repo: target.repo,
        runId: target.runId,
        workflow: null,
        branch: null,
      },
      mode: { type: "oneShot" },
      scope,
      notify: null,
    };
  }

  return {
    name: `PR: ${target.repo} #${target.prNumber}`,
    kind: {
      type: "githubPr",
      repo: target.repo,
      prNumber: target.prNumber,
    },
    mode: { type: "recurring", intervalSecs: 30 },
    scope,
    notify: null,
  };
}

export function installXtermWatchDecorations(
  terminal: Terminal,
  options?: InstallXtermWatchDecorationOptions,
): void {
  const decoratedTargets = new Set<string>();
  const createWatchFn = options?.createWatch ?? createWatch;
  const getActiveSessionId = options?.getActiveSessionId ?? (() => get(sessionState).activeSessionId);

  const addWatchDecoration = (
    yOffset: number,
    urlEnd: number,
    title: string,
    onClick: () => Promise<void>,
  ) => {
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
        el.firstElementChild!.setAttribute(
          "style",
          el.firstElementChild!.getAttribute("style")!.replace("opacity:0.7", "opacity:1"),
        );
      });
      el.addEventListener("mouseleave", () => {
        el.firstElementChild!.setAttribute(
          "style",
          el.firstElementChild!.getAttribute("style")!.replace("opacity:1", "opacity:0.7"),
        );
      });
      el.addEventListener("click", async (e) => {
        e.stopPropagation();
        e.preventDefault();
        await onClick();
      });
    });
  };

  terminal.onWriteParsed(() => {
    const buf = terminal.buffer.active;
    for (let i = Math.max(0, buf.cursorY - 2); i <= buf.cursorY; i++) {
      const absLine = buf.baseY + i;
      const line = buf.getLine(absLine);
      if (!line) continue;

      const text = line.translateToString();
      const yOffset = i - buf.cursorY;

      for (const target of findWatchTargetsInText(text)) {
        const targetKey = getWatchTargetKey(absLine, target);
        if (decoratedTargets.has(targetKey)) continue;
        decoratedTargets.add(targetKey);

        addWatchDecoration(
          yOffset,
          target.urlEnd,
          target.kind === "githubAction" ? "Watch this GitHub Action" : "Watch this PR",
          async () => {
            await createWatchFn(
              buildWatchConfigForTarget(target, getActiveSessionId()),
            );
          },
        );
      }
    }
  });
}

function getWatchTargetKey(absLine: number, target: WatchTarget): string {
  if (target.kind === "githubAction") {
    return `${absLine}:githubAction:${target.repo}:${target.runId}`;
  }
  return `${absLine}:githubPr:${target.repo}:${target.prNumber}`;
}
