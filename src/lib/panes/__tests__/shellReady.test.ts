import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));

import { waitForShellReady, waitForOutput } from "../shellReady";
import { emitPtyOutput, resetPtyOutputBus } from "../ptyOutputBus";

const enc = new TextEncoder();

describe("waitForShellReady", () => {
  beforeEach(() => {
    resetPtyOutputBus();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("resolves on OSC 133;A", async () => {
    const p = waitForShellReady("pty-1", { quietMs: 200, timeoutMs: 5000 });
    emitPtyOutput("pty-1", enc.encode("hello\x1b]133;A\x07$ "));
    await expect(p).resolves.toBe("osc133");
  });

  it("resolves on output quiescence when no OSC is emitted", async () => {
    const p = waitForShellReady("pty-1", { quietMs: 200, timeoutMs: 5000 });
    emitPtyOutput("pty-1", enc.encode("welcome to zsh\n$ "));
    vi.advanceTimersByTime(199);
    // Not yet.
    let settled = false;
    p.then(() => {
      settled = true;
    });
    await Promise.resolve();
    expect(settled).toBe(false);
    vi.advanceTimersByTime(2);
    await expect(p).resolves.toBe("quiet");
  });

  it("resets the quiet timer when more bytes arrive", async () => {
    const p = waitForShellReady("pty-1", { quietMs: 200, timeoutMs: 5000 });
    emitPtyOutput("pty-1", enc.encode("loading rc..."));
    vi.advanceTimersByTime(150);
    emitPtyOutput("pty-1", enc.encode("more rc..."));
    vi.advanceTimersByTime(150);
    let settled = false;
    p.then(() => {
      settled = true;
    });
    await Promise.resolve();
    expect(settled).toBe(false);
    vi.advanceTimersByTime(60);
    await expect(p).resolves.toBe("quiet");
  });

  // Skip: replay now handled by backend PtyLogger, requires integration test
  it.skip("sees replayed OSC 133;A from before the subscription", async () => {
    emitPtyOutput("pty-1", enc.encode("prompt \x1b]133;A\x07$ "));
    const p = waitForShellReady("pty-1", { quietMs: 200, timeoutMs: 1000 });
    await expect(p).resolves.toBe("osc133");
  });

  it("falls back to timeout if nothing ever arrives", async () => {
    const p = waitForShellReady("pty-1", { quietMs: 200, timeoutMs: 1000 });
    vi.advanceTimersByTime(1001);
    await expect(p).resolves.toBe("timeout");
  });
});

describe("waitForOutput", () => {
  beforeEach(() => {
    resetPtyOutputBus();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("matches a substring across a single chunk", async () => {
    const p = waitForOutput("pty-1", "Successfully logged into");
    emitPtyOutput(
      "pty-1",
      enc.encode("Successfully logged into account 123\n"),
    );
    await expect(p).resolves.toEqual({
      kind: "matched",
      text: "Successfully logged into",
    });
  });

  it("matches a substring that straddles chunk boundaries", async () => {
    const p = waitForOutput("pty-1", "READY");
    emitPtyOutput("pty-1", enc.encode("prefix REA"));
    emitPtyOutput("pty-1", enc.encode("DY suffix"));
    await expect(p).resolves.toEqual({ kind: "matched", text: "READY" });
  });

  it("matches a regex and returns the matched text", async () => {
    const p = waitForOutput("pty-1", /account (\d+)/);
    emitPtyOutput("pty-1", enc.encode("logged into account 42\n"));
    await expect(p).resolves.toEqual({ kind: "matched", text: "account 42" });
  });

  it("times out when the pattern never appears", async () => {
    const p = waitForOutput("pty-1", "nope", { timeoutMs: 500 });
    emitPtyOutput("pty-1", enc.encode("other output"));
    vi.advanceTimersByTime(501);
    await expect(p).resolves.toEqual({ kind: "timeout" });
  });

  // Skip: replay now handled by backend PtyLogger, requires integration test
  it.skip("sees replayed bytes from before the subscription", async () => {
    emitPtyOutput("pty-1", enc.encode("history contains READY marker\n"));
    const p = waitForOutput("pty-1", "READY");
    await expect(p).resolves.toEqual({ kind: "matched", text: "READY" });
  });
});
