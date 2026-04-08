import { describe, expect, it } from "vitest";
import { getDropSide } from "../dropTarget";

describe("getDropSide", () => {
  const rect = { left: 100, top: 200, width: 200, height: 120 };

  it("returns left for points closest to the left edge", () => {
    expect(getDropSide(rect, 105, 250)).toBe("left");
  });

  it("returns right for points closest to the right edge", () => {
    expect(getDropSide(rect, 295, 250)).toBe("right");
  });

  it("returns top for points closest to the top edge", () => {
    expect(getDropSide(rect, 180, 205)).toBe("top");
  });

  it("returns bottom for points closest to the bottom edge", () => {
    expect(getDropSide(rect, 180, 315)).toBe("bottom");
  });

  it("breaks corner ties consistently", () => {
    expect(getDropSide(rect, 100, 200)).toBe("left");
    expect(getDropSide(rect, 300, 320)).toBe("right");
  });

  it("stays stable for narrow panes", () => {
    const narrowRect = { left: 0, top: 0, width: 18, height: 160 };
    expect(getDropSide(narrowRect, 9, 3)).toBe("top");
    expect(getDropSide(narrowRect, 9, 157)).toBe("bottom");
    expect(getDropSide(narrowRect, 1, 80)).toBe("left");
  });
});
