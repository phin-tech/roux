import type { InitialPtySize } from "$lib/tauri";

/**
 * Estimate the cell dimensions (cols × rows) of the pane a new session will
 * land in, using the current window size, configured font metrics, and the
 * known chrome heights (topbar, statusbar) and sidebar width.
 *
 * The estimate does not need to be perfect. Its job is to start the PTY
 * close enough to the real pane size that the follow-up `fitAddon.fit()`
 * either produces identical dimensions (no SIGWINCH, no `zle reset-prompt`)
 * or a 1–2 cell adjustment whose reset-prompt is fast enough that no user
 * input has accumulated to be overwritten.
 *
 * The old default was a hardcoded 80×24, which caused visible prompt
 * overwrites in most modern terminal layouts — any wider pane would
 * trigger a full re-render on resize, and async zsh prompt frameworks
 * would redraw on top of typed characters during that window.
 */

/** Header/TabsBar/etc. above the pane area. Roughly measured from the DOM. */
const TOPBAR_HEIGHT_PX = 64;
/** Statusbar / footer below the pane area. */
const STATUSBAR_HEIGHT_PX = 24;
/** Left sidebar width when visible. Zero when collapsed. */
const DEFAULT_SIDEBAR_PX = 240;

/**
 * Monospace char-width / font-size ratio. Most programming fonts (Mono,
 * Fira Code, JetBrains Mono, Berkeley Mono, SF Mono) fall in 0.55–0.62;
 * 0.60 is a decent middle estimate and errs toward slightly wider cells,
 * which means we'd under-estimate cols by a cell or two rather than
 * over-estimate (and trigger a shrink SIGWINCH, which is the worse race).
 */
const CHAR_WIDTH_RATIO = 0.6;

export interface EstimatePaneSizeOptions {
  fontSize: number;
  lineHeight: number; // 1.0-ish multiplier
  sidebarVisible?: boolean;
}

export function estimatePaneSize(
  opts: EstimatePaneSizeOptions,
): InitialPtySize | null {
  if (typeof window === "undefined") return null;

  const charWidthPx = opts.fontSize * CHAR_WIDTH_RATIO;
  const lineHeightPx = opts.fontSize * opts.lineHeight;
  if (charWidthPx <= 0 || lineHeightPx <= 0) return null;

  const sidebarPx = opts.sidebarVisible === false ? 0 : DEFAULT_SIDEBAR_PX;
  const availWidthPx = Math.max(0, window.innerWidth - sidebarPx);
  const availHeightPx = Math.max(
    0,
    window.innerHeight - TOPBAR_HEIGHT_PX - STATUSBAR_HEIGHT_PX,
  );

  const cols = Math.max(20, Math.floor(availWidthPx / charWidthPx));
  const rows = Math.max(5, Math.floor(availHeightPx / lineHeightPx));

  return { cols, rows };
}
