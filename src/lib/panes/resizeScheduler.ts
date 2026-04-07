import type { FitAddon } from "@xterm/addon-fit";

interface ScheduleOptions {
  afterFit?: () => void;
}

interface ResizeSchedulerOptions {
  getFitAddon: () => FitAddon | null;
  getPtyId: () => string;
  onResize: (ptyId: string, cols: number, rows: number) => void;
}

export function createResizeScheduler({
  getFitAddon,
  getPtyId,
  onResize,
}: ResizeSchedulerOptions) {
  let frameId: number | null = null;
  let pendingAfterFit: (() => void) | null = null;
  let lastResize: { ptyId: string; cols: number; rows: number } | null = null;

  function flush() {
    frameId = null;

    const fitAddon = getFitAddon();
    const afterFit = pendingAfterFit;
    pendingAfterFit = null;

    if (!fitAddon) {
      afterFit?.();
      return;
    }

    fitAddon.fit();

    const dims = fitAddon.proposeDimensions();
    const ptyId = getPtyId();
    if (dims) {
      const unchanged =
        lastResize?.ptyId === ptyId &&
        lastResize.cols === dims.cols &&
        lastResize.rows === dims.rows;

      if (!unchanged) {
        lastResize = { ptyId, cols: dims.cols, rows: dims.rows };
        onResize(ptyId, dims.cols, dims.rows);
      }
    }

    afterFit?.();
  }

  return {
    schedule(options?: ScheduleOptions) {
      if (options?.afterFit) {
        pendingAfterFit = options.afterFit;
      }
      if (frameId !== null) return;
      frameId = requestAnimationFrame(flush);
    },
    cancel() {
      if (frameId !== null) {
        cancelAnimationFrame(frameId);
        frameId = null;
      }
      pendingAfterFit = null;
    },
  };
}
