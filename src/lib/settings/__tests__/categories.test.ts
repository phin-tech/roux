import { describe, expect, it } from "vitest";
import { normalizeSettingsCategoryId } from "../categories";

describe("settings categories", () => {
  it("keeps known categories and falls back for unknown values", () => {
    expect(normalizeSettingsCategoryId("externalTools")).toBe("externalTools");
    expect(normalizeSettingsCategoryId("missing")).toBe("general");
    expect(normalizeSettingsCategoryId(null)).toBe("general");
  });
});
