import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const checkMock = vi.fn();
const relaunchMock = vi.fn();

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: (...args: unknown[]) => checkMock(...args),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: (...args: unknown[]) => relaunchMock(...args),
}));

describe("checkForUpdate", () => {
  beforeEach(() => {
    checkMock.mockReset();
    relaunchMock.mockReset();
    vi.stubEnv("DEV", false);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
  });

  it("returns no-update when the plugin returns null (silent)", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkMock.mockResolvedValueOnce(null);

    const status = await checkForUpdate({ silent: true });

    expect(status).toEqual({ kind: "no-update" });
    expect(checkMock).toHaveBeenCalledTimes(1);
  });

  it("returns available with version and notes when plugin returns an Update", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkMock.mockResolvedValueOnce({
      version: "1.2.3",
      body: "Release notes here",
    });

    const status = await checkForUpdate({ silent: false });

    expect(status).toEqual({
      kind: "available",
      version: "1.2.3",
      notes: "Release notes here",
    });
  });

  it("defaults notes to empty string when body is undefined", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkMock.mockResolvedValueOnce({
      version: "2.0.0",
      body: undefined,
    });

    const status = await checkForUpdate({ silent: false });

    expect(status).toEqual({
      kind: "available",
      version: "2.0.0",
      notes: "",
    });
  });

  it("swallows network errors in silent mode and returns no-update", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { checkForUpdate } = await import("$lib/updater");
    checkMock.mockRejectedValueOnce(new Error("network connect timeout"));

    const status = await checkForUpdate({ silent: true });

    expect(status).toEqual({ kind: "no-update" });
    expect(warnSpy).toHaveBeenCalled();
  });

  it("surfaces network errors in non-silent mode", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkMock.mockRejectedValueOnce(new Error("network connect timeout"));

    const status = await checkForUpdate({ silent: false });

    expect(status).toEqual({ kind: "error", reason: "network" });
  });

  it("always surfaces signature errors even in silent mode", async () => {
    const { checkForUpdate } = await import("$lib/updater");
    checkMock.mockRejectedValueOnce(new Error("signature verification failed"));

    const status = await checkForUpdate({ silent: true });

    expect(status).toEqual({ kind: "error", reason: "signature-invalid" });
  });

  it("short-circuits in dev mode without calling the plugin", async () => {
    vi.stubEnv("DEV", true);
    const { checkForUpdate } = await import("$lib/updater");

    const status = await checkForUpdate({ silent: false });

    expect(status).toEqual({ kind: "no-update" });
    expect(checkMock).not.toHaveBeenCalled();
  });
});
