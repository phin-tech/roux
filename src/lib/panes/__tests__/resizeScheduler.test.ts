import { beforeEach, describe, expect, it, vi } from "vitest";

import { createResizeScheduler } from "../resizeScheduler";

describe("resizeScheduler", () => {
  let nextFrameId = 1;
  let scheduled = new Map<number, FrameRequestCallback>();

  beforeEach(() => {
    nextFrameId = 1;
    scheduled = new Map();
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn((cb: FrameRequestCallback) => {
        const id = nextFrameId++;
        scheduled.set(id, cb);
        return id;
      }),
    );
    vi.stubGlobal(
      "cancelAnimationFrame",
      vi.fn((id: number) => {
        scheduled.delete(id);
      }),
    );
  });

  function flushAnimationFrame() {
    const [id, cb] = [...scheduled.entries()][0] ?? [];
    if (id == null || cb == null) {
      throw new Error("no animation frame scheduled");
    }
    scheduled.delete(id);
    cb(performance.now());
  }

  it("dedupes repeated fits with unchanged dimensions for the same PTY", () => {
    const fit = vi
      .fn<() => { cols: number; rows: number } | null>()
      .mockReturnValue({ cols: 120, rows: 30 });
    const onResize = vi.fn();
    const scheduler = createResizeScheduler({
      fit,
      getPtyId: () => "pty-1",
      onResize,
    });

    scheduler.schedule();
    scheduler.schedule();
    flushAnimationFrame();

    scheduler.schedule();
    flushAnimationFrame();

    expect(fit).toHaveBeenCalledTimes(2);
    expect(onResize).toHaveBeenCalledTimes(1);
    expect(onResize).toHaveBeenCalledWith("pty-1", 120, 30);
  });

  it("re-emits resize when the PTY changes even if dimensions stay the same", () => {
    let ptyId = "pty-1";
    const fit = vi.fn().mockReturnValue({ cols: 100, rows: 24 });
    const onResize = vi.fn();
    const scheduler = createResizeScheduler({
      fit,
      getPtyId: () => ptyId,
      onResize,
    });

    scheduler.schedule();
    flushAnimationFrame();

    ptyId = "pty-2";
    scheduler.schedule();
    flushAnimationFrame();

    expect(onResize).toHaveBeenCalledTimes(2);
    expect(onResize).toHaveBeenNthCalledWith(1, "pty-1", 100, 24);
    expect(onResize).toHaveBeenNthCalledWith(2, "pty-2", 100, 24);
  });

  it("runs afterFit even when fit returns no dimensions", () => {
    const afterFit = vi.fn();
    const scheduler = createResizeScheduler({
      fit: () => null,
      getPtyId: () => "pty-1",
      onResize: vi.fn(),
    });

    scheduler.schedule({ afterFit });
    flushAnimationFrame();

    expect(afterFit).toHaveBeenCalledTimes(1);
  });

  it("cancel drops a scheduled frame and pending callback", () => {
    const afterFit = vi.fn();
    const onResize = vi.fn();
    const scheduler = createResizeScheduler({
      fit: () => ({ cols: 80, rows: 24 }),
      getPtyId: () => "pty-1",
      onResize,
    });

    scheduler.schedule({ afterFit });
    scheduler.cancel();

    expect(scheduled.size).toBe(0);
    expect(afterFit).not.toHaveBeenCalled();
    expect(onResize).not.toHaveBeenCalled();
  });
});
