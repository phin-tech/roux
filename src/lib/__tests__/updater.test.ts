import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const checkForUpdateMock = vi.fn();
const installUpdateMock = vi.fn();
const listenMock = vi.fn();
const relaunchMock = vi.fn();

vi.mock("$lib/bindings", () => ({
  commands: {
    checkForUpdate: (...args: unknown[]) => checkForUpdateMock(...args),
    installUpdate: (...args: unknown[]) => installUpdateMock(...args),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: (...args: unknown[]) => relaunchMock(...args),
}));

describe("checkForUpdate", () => {
  beforeEach(() => {
    checkForUpdateMock.mockReset();
    installUpdateMock.mockReset();
    listenMock.mockReset();
    relaunchMock.mockReset();
    vi.stubEnv("DEV", false);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
  });

  it("passes the channel through to the Rust command", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({ status: "ok", data: null });

    await checkForUpdate({ silent: true, channel: "preRelease" });

    expect(checkForUpdateMock).toHaveBeenCalledWith("preRelease");
  });

  it("returns no-update when the command returns null data", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({ status: "ok", data: null });

    const status = await checkForUpdate({ silent: true, channel: "stable" });

    expect(status).toEqual({ kind: "no-update" });
  });

  it("returns available with version and notes when an update is found", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({
      status: "ok",
      data: { version: "1.2.3", notes: "Release notes here" },
    });

    const status = await checkForUpdate({ silent: false, channel: "stable" });

    expect(status).toEqual({
      kind: "available",
      version: "1.2.3",
      notes: "Release notes here",
    });
  });

  it("defaults notes to empty string when the backend omits them", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({
      status: "ok",
      data: { version: "2.0.0", notes: "" },
    });

    const status = await checkForUpdate({ silent: false, channel: "stable" });

    expect(status).toEqual({ kind: "available", version: "2.0.0", notes: "" });
  });

  it("swallows backend network errors in silent mode", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({
      status: "error",
      error: { kind: "network" },
    });

    const status = await checkForUpdate({ silent: true, channel: "stable" });

    expect(status).toEqual({ kind: "no-update" });
    expect(warnSpy).toHaveBeenCalled();
  });

  it("surfaces backend network errors in non-silent mode", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({
      status: "error",
      error: { kind: "network" },
    });

    const status = await checkForUpdate({ silent: false, channel: "stable" });

    expect(status).toEqual({ kind: "error", reason: "network" });
  });

  it("always surfaces signature errors even in silent mode", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({
      status: "error",
      error: { kind: "signature-invalid" },
    });

    const status = await checkForUpdate({ silent: true, channel: "stable" });

    expect(status).toEqual({ kind: "error", reason: "signature-invalid" });
  });

  it("maps not-found to no-update (empty channel is not an error)", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({
      status: "error",
      error: { kind: "not-found" },
    });

    const status = await checkForUpdate({ silent: false, channel: "preRelease" });

    expect(status).toEqual({ kind: "no-update" });
  });

  it("classifies internal errors as unknown", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockResolvedValueOnce({
      status: "error",
      error: { kind: "internal", message: "boom" },
    });

    const status = await checkForUpdate({ silent: false, channel: "stable" });

    expect(status).toEqual({ kind: "error", reason: "unknown" });
  });

  it("surfaces transport failures as a classified error", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkForUpdateMock.mockRejectedValueOnce(new Error("network connect timeout"));

    const status = await checkForUpdate({ silent: false, channel: "stable" });

    expect(status).toEqual({ kind: "error", reason: "network" });
  });

  it("short-circuits in dev mode without calling the command", async () => {
    vi.stubEnv("DEV", true);
    const { checkForUpdate } = await import("$lib/updater");

    const status = await checkForUpdate({ silent: false, channel: "stable" });

    expect(status).toEqual({ kind: "no-update" });
    expect(checkForUpdateMock).not.toHaveBeenCalled();
  });
});
