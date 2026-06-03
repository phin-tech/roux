import { describe, expect, it } from "vitest";

import { resolveMultiLineEditorSeed } from "../multiLineEditorSeed";

describe("resolveMultiLineEditorSeed", () => {
  it("uses explicit seed text before terminal selection", () => {
    expect(
      resolveMultiLineEditorSeed("clipboard text", "selected text"),
    ).toEqual({
      text: "clipboard text",
      seeded: true,
    });
  });

  it("uses selected terminal text when no explicit seed is provided", () => {
    expect(resolveMultiLineEditorSeed(null, "git status\nnpm test")).toEqual({
      text: "git status\nnpm test",
      seeded: true,
    });
  });

  it("preserves empty explicit seed as an intentional empty editor", () => {
    expect(resolveMultiLineEditorSeed("", "selected text")).toEqual({
      text: "",
      seeded: false,
    });
  });

  it("falls back to an empty unseeded editor when nothing is selected", () => {
    expect(resolveMultiLineEditorSeed(null, "")).toEqual({
      text: "",
      seeded: false,
    });
  });
});
