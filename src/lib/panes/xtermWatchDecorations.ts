import type { Terminal } from "@xterm/xterm";
import { get } from "svelte/store";

import { sessionState } from "$lib/stores/sessions";
import { createWatch } from "$lib/tauri";
import type { CreateWatchConfig } from "$lib/types";

// Match GitHub Actions run URLs, including optional trailing path
// (`/job/<n>`, `/jobs/<n>`, `/attempts/<n>`), query string, or fragment.
// The trailing `(?:[/?#]\S*)?` lets the eye-icon decoration land at the
// end of the visible URL even when the user pasted a deep link.
const GH_ACTION_PATTERN =
  /https?:\/\/(?:www\.)?github\.com\/([\w.-]+\/[\w.-]+)\/actions\/runs\/(\d+)(?:[/?#]\S*)?/g;
// Match GitHub PR URLs, including optional `/files`, `/commits`,
// `/checks`, query string, or fragment.
const GH_PR_PATTERN =
  /https?:\/\/(?:www\.)?github\.com\/([\w.-]+\/[\w.-]+)\/pull\/(\d+)(?:[/?#]\S*)?/g;

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

export interface XtermWatchDecorationsHandle {
  dispose(): void;
}

export function installXtermWatchDecorations(
  terminal: Terminal,
  options?: InstallXtermWatchDecorationOptions,
): XtermWatchDecorationsHandle {
  const decoratedTargets = new Set<string>();
  const createWatchFn = options?.createWatch ?? createWatch;
  const getActiveSessionId =
    options?.getActiveSessionId ?? (() => get(sessionState).activeSessionId);

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
          el
            .firstElementChild!.getAttribute("style")!
            .replace("opacity:0.7", "opacity:1"),
        );
      });
      el.addEventListener("mouseleave", () => {
        el.firstElementChild!.setAttribute(
          "style",
          el
            .firstElementChild!.getAttribute("style")!
            .replace("opacity:1", "opacity:0.7"),
        );
      });
      el.addEventListener("click", async (e) => {
        e.stopPropagation();
        e.preventDefault();
        await onClick();
      });
    });
  };

  // Default lookback above the cursor. Wide enough to catch logical
  // lines whose URL wraps across several visual rows at narrow widths
  // (a 100-char URL at 40 cols wraps to 3 rows).
  const SCAN_WINDOW = 16;
  // Hard cap on how far back we'll catch up in a single scan. Bursty
  // PTY output (`git log -p`, `cargo build`) can write hundreds of rows
  // between rAF callbacks; without a cap we'd traverse the entire
  // scrollback on the first frame after a burst.
  const MAX_CATCHUP_LINES = 2048;

  // Coalesce scans into a single rAF callback. Under fast PTY traffic
  // (`watch`, `ls -R`, build output) `onWriteParsed` fires hundreds of
  // times per second; without this the buffer + regex scan ran on each
  // write and pinned the render thread.
  //
  // To avoid losing decorations on bursts that scroll URLs out of the
  // viewport between rAFs, we track the highest absolute line scanned
  // so far and walk forward from there to the current cursor row each
  // time, instead of only looking at a fixed window above the cursor.
  // When a single frame's catch-up would exceed MAX_CATCHUP_LINES, the
  // unscanned remainder is recorded as `pendingBacklog` and drained on
  // subsequent rAFs — without this, lines in the gap would be skipped
  // permanently if output goes quiet right after a burst.
  let scanScheduled = false;
  let disposed = false;
  let lastScannedAbs: number | null = null;
  let pendingBacklog: { start: number; end: number } | null = null;

  const scheduleScan = () => {
    if (scanScheduled || disposed) return;
    scanScheduled = true;
    // typeof check: keeps node-only Vitest happy where rAF is undefined.
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(runScan);
    } else {
      // Fallback for environments without rAF — still defer so a burst of
      // writes coalesces.
      setTimeout(runScan, 16);
    }
  };

  const runScan = () => {
    scanScheduled = false;
    if (disposed) return;
    const buf = terminal.buffer.active;
    const bottomAbs = buf.baseY + buf.cursorY;
    // Floor: at minimum, scan SCAN_WINDOW rows above the cursor. This
    // preserves the original behavior on the first scan and in steady
    // state.
    const windowFloor = Math.max(0, bottomAbs - SCAN_WINDOW);
    // Determine the natural catch-up start: pending backlog from a prior
    // frame takes priority, otherwise extend back to lastScannedAbs+1.
    let top: number;
    if (pendingBacklog !== null) {
      top = pendingBacklog.start;
    } else if (lastScannedAbs === null) {
      top = windowFloor;
    } else {
      top = Math.min(windowFloor, lastScannedAbs + 1);
    }

    // Cap the per-frame work at MAX_CATCHUP_LINES. If the range exceeds
    // it, scan the OLDEST chunk first and record the rest as backlog —
    // newer chunks can still scroll into scrollback later, but xterm's
    // buffer keeps absolute lines reachable until they're evicted.
    const totalRange = bottomAbs - top + 1;
    let scanEnd: number;
    if (totalRange > MAX_CATCHUP_LINES) {
      scanEnd = top + MAX_CATCHUP_LINES - 1;
      pendingBacklog = { start: scanEnd + 1, end: bottomAbs };
    } else {
      scanEnd = bottomAbs;
      pendingBacklog = null;
    }
    // lastScannedAbs tracks the latest line we've actually scanned, not
    // the latest cursor position — so a backlog drain in the next rAF
    // doesn't skip rows we haven't yet processed.
    if (lastScannedAbs === null || scanEnd > lastScannedAbs) {
      lastScannedAbs = scanEnd;
    }

    for (let absStart = top; absStart <= scanEnd; absStart++) {
      const startLine = buf.getLine(absStart);
      if (!startLine) continue;
      // Only kick off processing from a logical-line start. Continuation
      // rows (`isWrapped=true`) are folded into their parent's scan.
      if (startLine.isWrapped) continue;

      // Build the logical line by concatenating consecutive `isWrapped`
      // continuations. We keep per-segment lengths so we can project a
      // logical-string offset back to (visual line, column) when placing
      // the decoration anchor.
      type Segment = { absLine: number; yOffset: number; length: number };
      let logicalText = "";
      const segments: Segment[] = [];
      let j = 0;
      while (true) {
        const absLine = absStart + j;
        const line = buf.getLine(absLine);
        if (!line) break;
        if (j > 0 && !line.isWrapped) break;
        // Pass `false` so trailing whitespace is preserved; offsets need
        // to align with the actual cell positions on the visual row.
        const text = line.translateToString(false);
        segments.push({
          absLine,
          // yOffset is relative to the cursor; xterm markers accept
          // negative values, which place the marker in scrollback.
          yOffset: absLine - bottomAbs,
          length: text.length,
        });
        logicalText += text;
        j++;
      }

      for (const target of findWatchTargetsInText(logicalText)) {
        const proj = projectOffset(segments, target.urlEnd);
        if (!proj) continue;
        const { absLine, yOffset, column } = proj;

        const targetKey = getWatchTargetKey(absLine, target);
        if (decoratedTargets.has(targetKey)) continue;
        decoratedTargets.add(targetKey);

        addWatchDecoration(
          yOffset,
          column,
          target.kind === "githubAction"
            ? "Watch this GitHub Action"
            : "Watch this PR",
          async () => {
            await createWatchFn(
              buildWatchConfigForTarget(target, getActiveSessionId()),
            );
          },
        );
      }
    }

    // Drain remaining backlog on a follow-up frame. Each pass scans
    // MAX_CATCHUP_LINES, so the worst-case lag for a deep backlog is
    // ceil(backlog / MAX_CATCHUP_LINES) frames — orders of magnitude
    // better than losing the lines outright.
    if (pendingBacklog !== null) {
      scheduleScan();
    }
  };

  const writeParsedSub = terminal.onWriteParsed(() => {
    scheduleScan();
  });

  return {
    dispose() {
      disposed = true;
      writeParsedSub.dispose();
    },
  };
}

interface ProjectedOffset {
  absLine: number;
  yOffset: number;
  column: number;
}

/**
 * Given the per-segment lengths of a logical line and an offset within
 * the concatenated logical text, return the (absolute line, viewport
 * y-offset, column) of that offset on its visual segment. When the
 * offset is past the logical line's end (rare — would require a regex
 * that overran the buffer), the trailing segment is used and the column
 * is clamped.
 */
export function projectOffset(
  segments: { absLine: number; yOffset: number; length: number }[],
  offset: number,
): ProjectedOffset | null {
  if (segments.length === 0) return null;
  let acc = 0;
  for (const seg of segments) {
    if (offset <= acc + seg.length) {
      return {
        absLine: seg.absLine,
        yOffset: seg.yOffset,
        column: offset - acc,
      };
    }
    acc += seg.length;
  }
  const last = segments[segments.length - 1];
  return { absLine: last.absLine, yOffset: last.yOffset, column: last.length };
}

function getWatchTargetKey(absLine: number, target: WatchTarget): string {
  if (target.kind === "githubAction") {
    return `${absLine}:githubAction:${target.repo}:${target.runId}`;
  }
  return `${absLine}:githubPr:${target.repo}:${target.prNumber}`;
}
