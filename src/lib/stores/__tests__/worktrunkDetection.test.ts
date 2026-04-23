import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/bindings", () => ({
  commands: {
    cmdDetectWorktrunk: vi.fn(),
  },
}));

import { commands } from "$lib/bindings";
import {
  _resetWorktrunkDetectionForTests,
  refreshWorktrunkDetection,
  worktrunkDetection,
} from "../worktrunkDetection";

describe("worktrunkDetection store", () => {
  beforeEach(() => {
    _resetWorktrunkDetectionForTests();
    vi.mocked(commands.cmdDetectWorktrunk).mockReset();
  });

  afterEach(() => {
    _resetWorktrunkDetectionForTests();
  });

  it("starts un-probed with no binary resolved", () => {
    const s = get(worktrunkDetection);
    expect(s.probed).toBe(false);
    expect(s.binaryPath).toBeNull();
    expect(s.version).toBeNull();
  });

  it("populates binaryPath + version on successful probe", async () => {
    vi.mocked(commands.cmdDetectWorktrunk).mockResolvedValueOnce({
      binaryPath: "/opt/homebrew/bin/wt",
      version: "0.44.0",
      hasConfig: false,
    });
    await refreshWorktrunkDetection();
    const s = get(worktrunkDetection);
    expect(s.binaryPath).toBe("/opt/homebrew/bin/wt");
    expect(s.version).toBe("0.44.0");
    expect(s.probed).toBe(true);
  });

  it("marks probed=true with null binary when wt is not installed", async () => {
    vi.mocked(commands.cmdDetectWorktrunk).mockResolvedValueOnce({
      binaryPath: null,
      version: null,
      hasConfig: false,
    });
    await refreshWorktrunkDetection();
    const s = get(worktrunkDetection);
    expect(s.binaryPath).toBeNull();
    expect(s.version).toBeNull();
    expect(s.probed).toBe(true);
  });

  it("treats a probe failure as not-installed and still marks probed", async () => {
    vi.mocked(commands.cmdDetectWorktrunk).mockRejectedValueOnce(
      new Error("detect threw"),
    );
    await refreshWorktrunkDetection();
    const s = get(worktrunkDetection);
    expect(s.binaryPath).toBeNull();
    expect(s.probed).toBe(true);
  });
});
