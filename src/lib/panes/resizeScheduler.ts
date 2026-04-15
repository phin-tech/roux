interface ScheduleOptions {
  afterFit?: () => void;
}

interface ResizeSchedulerOptions {
  fit: () => { cols: number; rows: number } | null;
  getPtyId: () => string;
  onResize: (ptyId: string, cols: number, rows: number) => void;
}

export function createResizeScheduler({
  fit,
  getPtyId,
  onResize,
}: ResizeSchedulerOptions) {
  let frameId: number | null = null;
  let pendingAfterFit: (() => void) | null = null;
  let lastResize: { ptyId: string; cols: number; rows: number } | null = null;

  function flush() {
    frameId = null;

    const afterFit = pendingAfterFit;
    pendingAfterFit = null;

    const dims = fit();
    if (!dims) {
      afterFit?.();
      return;
    }

    const ptyId = getPtyId();
    const unchanged =
      lastResize?.ptyId === ptyId &&
      lastResize.cols === dims.cols &&
      lastResize.rows === dims.rows;

    if (!unchanged) {
      lastResize = { ptyId, cols: dims.cols, rows: dims.rows };
      onResize(ptyId, dims.cols, dims.rows);
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
