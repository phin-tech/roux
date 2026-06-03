import { describe, expect, it } from "vitest";
import type { SpawnProfile } from "../profiles";
import { resolveAppDefaultSplitProfile } from "../splitProfileBehavior";

function profile(id: string): SpawnProfile {
  return {
    id,
    name: id,
    source: "builtin",
  };
}

describe("resolveAppDefaultSplitProfile", () => {
  it("uses the configured default profile when it exists", () => {
    const custom = profile("custom");
    const registry = new Map<string, SpawnProfile>([
      ["claude", profile("claude")],
      ["custom", custom],
    ]);

    expect(resolveAppDefaultSplitProfile(registry, "custom")).toBe(custom);
  });

  it("falls back to claude when the configured profile is missing", () => {
    const claude = profile("claude");
    const registry = new Map<string, SpawnProfile>([["claude", claude]]);

    expect(resolveAppDefaultSplitProfile(registry, "missing")).toBe(claude);
  });
});
