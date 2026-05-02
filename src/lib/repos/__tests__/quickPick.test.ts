import { describe, expect, it } from "vitest";
import {
  buildQuickPickOptions,
  formatRepoShortLabel,
  findQuickPickMatch,
} from "../quickPick";

describe("formatRepoShortLabel", () => {
  it("returns the last 2 segments by default", () => {
    expect(formatRepoShortLabel("/home/me/src/repo")).toBe("src/repo");
  });

  it("returns the path unchanged when there are fewer segments than depth", () => {
    expect(formatRepoShortLabel("/repo")).toBe("repo");
  });

  it("normalizes Windows-style backslash paths", () => {
    expect(formatRepoShortLabel("C:\\Users\\me\\src\\repo")).toBe("src/repo");
  });

  it("respects an explicit depth", () => {
    expect(formatRepoShortLabel("/a/b/c/d/repo", 3)).toBe("c/d/repo");
  });
});

describe("buildQuickPickOptions", () => {
  it("uses 2-segment labels when there are no collisions", () => {
    const opts = buildQuickPickOptions(["/work/api", "/work/web"]);
    expect(opts.map((o) => o.label)).toEqual(["work/api", "work/web"]);
  });

  it("bumps colliding 2-segment labels to 3 segments", () => {
    const opts = buildQuickPickOptions([
      "/home/alice/src/repo",
      "/home/bob/src/repo",
    ]);
    expect(opts.map((o) => o.label)).toEqual([
      "alice/src/repo",
      "bob/src/repo",
    ]);
  });

  it("falls back to the full path when 3-segment labels still collide", () => {
    // Both paths share the same 3-segment tail (`x/y/repo`), so the bump
    // alone doesn't disambiguate. The doc-comment promise — "the picker
    // never displays two visually identical rows" — requires we go all
    // the way to the full path here.
    const a = "/foo/x/y/repo";
    const b = "/bar/x/y/repo";
    const opts = buildQuickPickOptions([a, b]);
    expect(opts.map((o) => o.label)).toEqual([a, b]);
  });

  it("only falls back to the full path for the colliding rows", () => {
    // The unrelated row stays at the cheap 2-segment label even when
    // others have to fall back further.
    const a = "/foo/x/y/repo";
    const b = "/bar/x/y/repo";
    const opts = buildQuickPickOptions([a, b, "/work/standalone"]);
    expect(opts).toEqual([
      { path: a, label: a },
      { path: b, label: b },
      { path: "/work/standalone", label: "work/standalone" },
    ]);
  });

  it("returns an empty array for no input", () => {
    expect(buildQuickPickOptions([])).toEqual([]);
  });
});

describe("findQuickPickMatch", () => {
  const opts = [
    { path: "/work/api", label: "work/api" },
    { path: "/work/web", label: "work/web" },
  ];

  it("matches an exact path", () => {
    expect(findQuickPickMatch("/work/api", opts)?.path).toBe("/work/api");
  });

  it("matches an exact label case-insensitively", () => {
    expect(findQuickPickMatch("WORK/WEB", opts)?.path).toBe("/work/web");
  });

  it("falls back to substring on label or path", () => {
    expect(findQuickPickMatch("api", opts)?.path).toBe("/work/api");
  });

  it("returns null for empty / unmatched input", () => {
    expect(findQuickPickMatch("", opts)).toBeNull();
    expect(findQuickPickMatch("nope", opts)).toBeNull();
  });
});
