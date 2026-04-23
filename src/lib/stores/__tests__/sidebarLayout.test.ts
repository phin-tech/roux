import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

// jsdom under vitest 4 ships a non-functional localStorage; provide a Map-based stub.
function installLocalStorageStub(): Storage {
  const store = new Map<string, string>();
  const stub: Storage = {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (k) => (store.has(k) ? store.get(k)! : null),
    key: (i) => Array.from(store.keys())[i] ?? null,
    removeItem: (k) => {
      store.delete(k);
    },
    setItem: (k, v) => {
      store.set(k, String(v));
    },
  };
  vi.stubGlobal("localStorage", stub);
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: stub,
  });
  return stub;
}

async function freshModule() {
  vi.resetModules();
  return await import("../sidebarLayout");
}

describe("sidebarLayout", () => {
  beforeEach(() => {
    installLocalStorageStub();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults railSide to 'right'", async () => {
    const { sidebarLayout } = await freshModule();
    expect(get(sidebarLayout).railSide).toBe("right");
  });

  it("setRailSide updates the store", async () => {
    const { sidebarLayout, setRailSide } = await freshModule();
    setRailSide("left");
    expect(get(sidebarLayout).railSide).toBe("left");
  });

  it("toggleRailSide flips between left and right", async () => {
    const { sidebarLayout, toggleRailSide } = await freshModule();
    const initial = get(sidebarLayout).railSide;
    toggleRailSide();
    expect(get(sidebarLayout).railSide).not.toBe(initial);
    toggleRailSide();
    expect(get(sidebarLayout).railSide).toBe(initial);
  });

  it("migrates legacy dockSide='left' to railSide='left'", async () => {
    window.localStorage.setItem(
      "roux.sidebar.dock",
      JSON.stringify({ dockSide: "left", width: 400, splitRatio: 0.5 }),
    );
    const { sidebarLayout } = await freshModule();
    expect(get(sidebarLayout).railSide).toBe("left");
  });

  it("migrates legacy dockSide='right' to railSide='right'", async () => {
    window.localStorage.setItem(
      "roux.sidebar.dock",
      JSON.stringify({ dockSide: "right", width: 400, splitRatio: 0.5 }),
    );
    const { sidebarLayout } = await freshModule();
    expect(get(sidebarLayout).railSide).toBe("right");
  });

  it("persists railSide to localStorage", async () => {
    const { setRailSide } = await freshModule();
    setRailSide("left");
    const raw = window.localStorage.getItem("roux.sidebar.dock");
    expect(raw).not.toBeNull();
    expect(JSON.parse(raw!).railSide).toBe("left");
  });

  it("falls back to defaults when stored JSON is malformed", async () => {
    window.localStorage.setItem("roux.sidebar.dock", "{not json");
    const { sidebarLayout } = await freshModule();
    const s = get(sidebarLayout);
    expect(Number.isFinite(s.width)).toBe(true);
    expect(Number.isFinite(s.splitRatio)).toBe(true);
    expect(s.railSide).toBe("right");
  });

  it("rejects non-finite width (NaN) and falls back to default", async () => {
    window.localStorage.setItem(
      "roux.sidebar.dock",
      JSON.stringify({ width: Number.NaN, splitRatio: 0.5, railSide: "right" }),
    );
    const { sidebarLayout } = await freshModule();
    expect(Number.isFinite(get(sidebarLayout).width)).toBe(true);
  });

  it("rejects non-finite splitRatio (Infinity) and falls back to default", async () => {
    window.localStorage.setItem(
      "roux.sidebar.dock",
      JSON.stringify({ width: 400, splitRatio: Number.POSITIVE_INFINITY, railSide: "right" }),
    );
    const { sidebarLayout } = await freshModule();
    expect(Number.isFinite(get(sidebarLayout).splitRatio)).toBe(true);
  });

  it("rejects stringified numbers (no coercion) and falls back", async () => {
    window.localStorage.setItem(
      "roux.sidebar.dock",
      JSON.stringify({ width: "400", splitRatio: "0.5", railSide: "right" }),
    );
    const { sidebarLayout } = await freshModule();
    const s = get(sidebarLayout);
    expect(Number.isFinite(s.width)).toBe(true);
    expect(Number.isFinite(s.splitRatio)).toBe(true);
  });

  it("clamps stored width above MAX_DOCK_WIDTH", async () => {
    window.localStorage.setItem(
      "roux.sidebar.dock",
      JSON.stringify({ width: 99999, splitRatio: 0.5, railSide: "right" }),
    );
    const { sidebarLayout, MAX_DOCK_WIDTH } = await freshModule();
    expect(get(sidebarLayout).width).toBeLessThanOrEqual(MAX_DOCK_WIDTH);
  });

  it("clamps stored splitRatio above max (0.8)", async () => {
    window.localStorage.setItem(
      "roux.sidebar.dock",
      JSON.stringify({ width: 400, splitRatio: 0.95, railSide: "right" }),
    );
    const { sidebarLayout } = await freshModule();
    expect(get(sidebarLayout).splitRatio).toBeLessThanOrEqual(0.8);
  });

  it("persists hidden=true across reload", async () => {
    const { hideSidebar } = await freshModule();
    hideSidebar();
    const { sidebarLayout } = await freshModule();
    expect(get(sidebarLayout).hidden).toBe(true);
  });

});
