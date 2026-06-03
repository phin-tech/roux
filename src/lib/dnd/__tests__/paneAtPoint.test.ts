import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { paneAtPoint } from "../paneAtPoint";

describe("paneAtPoint", () => {
  const originalElementFromPoint = (
    document as Document & { elementFromPoint?: unknown }
  ).elementFromPoint;

  beforeEach(() => {
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
    if (originalElementFromPoint === undefined) {
      delete (document as unknown as Record<string, unknown>).elementFromPoint;
    } else {
      (document as unknown as Record<string, unknown>).elementFromPoint =
        originalElementFromPoint;
    }
    vi.restoreAllMocks();
  });

  function mockElementFromPoint(el: Element | null) {
    (document as unknown as Record<string, unknown>).elementFromPoint = vi
      .fn()
      .mockReturnValue(el);
  }

  it("returns the pane id when the element under the point carries data-pane-id", () => {
    const pane = document.createElement("div");
    pane.setAttribute("data-pane-id", "pane-abc");
    document.body.appendChild(pane);
    mockElementFromPoint(pane);

    expect(paneAtPoint(10, 20)).toBe("pane-abc");
  });

  it("walks up to the nearest ancestor with data-pane-id", () => {
    const pane = document.createElement("div");
    pane.setAttribute("data-pane-id", "pane-outer");
    const inner = document.createElement("span");
    pane.appendChild(inner);
    document.body.appendChild(pane);
    mockElementFromPoint(inner);

    expect(paneAtPoint(10, 20)).toBe("pane-outer");
  });

  it("returns null when no ancestor has data-pane-id", () => {
    const orphan = document.createElement("div");
    document.body.appendChild(orphan);
    mockElementFromPoint(orphan);

    expect(paneAtPoint(10, 20)).toBeNull();
  });

  it("returns null when elementFromPoint returns null", () => {
    mockElementFromPoint(null);
    expect(paneAtPoint(10, 20)).toBeNull();
  });

  it("returns null when data-pane-id is empty string", () => {
    const pane = document.createElement("div");
    pane.setAttribute("data-pane-id", "");
    document.body.appendChild(pane);
    mockElementFromPoint(pane);

    expect(paneAtPoint(10, 20)).toBeNull();
  });
});
