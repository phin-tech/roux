import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/bindings", () => ({
  commands: {
    cmdDetectSmolvm: vi.fn(),
  },
}));

import { commands } from "$lib/bindings";
import {
  _resetSmolvmDetectionForTests,
  refreshSmolvmDetection,
  smolvmDetection,
} from "../smolvmDetection";

describe("smolvmDetection store", () => {
  beforeEach(() => {
    _resetSmolvmDetectionForTests();
    vi.mocked(commands.cmdDetectSmolvm).mockReset();
  });

  afterEach(() => {
    _resetSmolvmDetectionForTests();
  });

  it("starts un-probed with no binary resolved", () => {
    const s = get(smolvmDetection);
    expect(s.probed).toBe(false);
    expect(s.binaryPath).toBeNull();
    expect(s.version).toBeNull();
  });

  it("populates binaryPath + version on successful probe", async () => {
    vi.mocked(commands.cmdDetectSmolvm).mockResolvedValueOnce({
      binaryPath: "/opt/homebrew/bin/smolvm",
      version: "0.1.2",
    });
    await refreshSmolvmDetection();
    const s = get(smolvmDetection);
    expect(s.binaryPath).toBe("/opt/homebrew/bin/smolvm");
    expect(s.version).toBe("0.1.2");
    expect(s.probed).toBe(true);
  });

  it("marks probed=true with null binary when smolvm is not installed", async () => {
    vi.mocked(commands.cmdDetectSmolvm).mockResolvedValueOnce({
      binaryPath: null,
      version: null,
    });
    await refreshSmolvmDetection();
    const s = get(smolvmDetection);
    expect(s.binaryPath).toBeNull();
    expect(s.version).toBeNull();
    expect(s.probed).toBe(true);
  });

  it("treats a probe failure as not-installed and still marks probed", async () => {
    vi.mocked(commands.cmdDetectSmolvm).mockRejectedValueOnce(
      new Error("detect threw"),
    );
    await refreshSmolvmDetection();
    const s = get(smolvmDetection);
    expect(s.binaryPath).toBeNull();
    expect(s.probed).toBe(true);
  });
});
