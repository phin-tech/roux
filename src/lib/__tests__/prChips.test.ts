import { describe, expect, it } from "vitest";

import { checksChipFor, reviewChipFor } from "../prChips";

describe("checksChipFor", () => {
  it("returns null when there is no rollup or rollup is empty", () => {
    expect(checksChipFor(null)).toBeNull();
    expect(checksChipFor(undefined)).toBeNull();
    expect(
      checksChipFor({
        state: "none",
        passing: 0,
        failing: 0,
        pending: 0,
        total: 0,
      }),
    ).toBeNull();
  });

  it("renders a green chip when all checks are passing", () => {
    const chip = checksChipFor({
      state: "passing",
      passing: 4,
      failing: 0,
      pending: 0,
      total: 4,
    });
    expect(chip?.color).toBe("text-green");
    expect(chip?.label).toContain("4/4");
    expect(chip?.spin).toBe(false);
  });

  it("renders a red chip when any check is failing", () => {
    const chip = checksChipFor({
      state: "failing",
      passing: 2,
      failing: 1,
      pending: 0,
      total: 3,
    });
    expect(chip?.color).toBe("text-red");
    expect(chip?.label).toContain("1/3");
  });

  it("renders a spinning yellow chip while checks are pending", () => {
    const chip = checksChipFor({
      state: "pending",
      passing: 1,
      failing: 0,
      pending: 2,
      total: 3,
    });
    expect(chip?.color).toBe("text-yellow");
    expect(chip?.spin).toBe(true);
  });
});

describe("reviewChipFor", () => {
  it("returns null when there is no review decision", () => {
    expect(reviewChipFor(null)).toBeNull();
    expect(reviewChipFor("")).toBeNull();
  });

  it("maps APPROVED to a green chip", () => {
    const chip = reviewChipFor("APPROVED");
    expect(chip?.color).toBe("text-green");
    expect(chip?.label).toBe("Approved");
  });

  it("maps CHANGES_REQUESTED to a red chip", () => {
    const chip = reviewChipFor("CHANGES_REQUESTED");
    expect(chip?.color).toBe("text-red");
    expect(chip?.label).toBe("Changes requested");
  });

  it("renders REVIEW_REQUIRED muted (not red)", () => {
    const chip = reviewChipFor("REVIEW_REQUIRED");
    expect(chip?.color).toBe("text-text-muted");
  });

  it("ignores unknown decisions", () => {
    expect(reviewChipFor("WHATEVER")).toBeNull();
  });
});
