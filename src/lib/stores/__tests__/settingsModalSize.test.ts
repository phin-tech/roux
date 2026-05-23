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
  return await import("../settingsModalSize");
}

const KEY = "roux.settings.modalSize";

describe("settingsModalSize", () => {
  beforeEach(() => {
    installLocalStorageStub();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to a finite width and height", async () => {
    const { settingsModalSize } = await freshModule();
    const s = get(settingsModalSize);
    expect(Number.isFinite(s.width)).toBe(true);
    expect(Number.isFinite(s.height)).toBe(true);
  });

  it("setSettingsModalSize updates the store", async () => {
    const { settingsModalSize, setSettingsModalSize } = await freshModule();
    setSettingsModalSize(900, 700);
    expect(get(settingsModalSize)).toEqual({ width: 900, height: 700 });
  });

  it("persists size to localStorage", async () => {
    const { setSettingsModalSize } = await freshModule();
    setSettingsModalSize(900, 700);
    const raw = window.localStorage.getItem(KEY);
    expect(raw).not.toBeNull();
    expect(JSON.parse(raw!)).toEqual({ width: 900, height: 700 });
  });

  it("restores persisted size across reload", async () => {
    const { setSettingsModalSize } = await freshModule();
    setSettingsModalSize(910, 710);
    const { settingsModalSize } = await freshModule();
    expect(get(settingsModalSize)).toEqual({ width: 910, height: 710 });
  });

  it("clamps width above max and below min", async () => {
    const {
      setSettingsModalSize,
      settingsModalSize,
      SETTINGS_MODAL_MAX_WIDTH,
      SETTINGS_MODAL_MIN_WIDTH,
    } = await freshModule();
    setSettingsModalSize(99999, 700);
    expect(get(settingsModalSize).width).toBe(SETTINGS_MODAL_MAX_WIDTH);
    setSettingsModalSize(10, 700);
    expect(get(settingsModalSize).width).toBe(SETTINGS_MODAL_MIN_WIDTH);
  });

  it("clamps height above max and below min", async () => {
    const {
      setSettingsModalSize,
      settingsModalSize,
      SETTINGS_MODAL_MAX_HEIGHT,
      SETTINGS_MODAL_MIN_HEIGHT,
    } = await freshModule();
    setSettingsModalSize(900, 99999);
    expect(get(settingsModalSize).height).toBe(SETTINGS_MODAL_MAX_HEIGHT);
    setSettingsModalSize(900, 10);
    expect(get(settingsModalSize).height).toBe(SETTINGS_MODAL_MIN_HEIGHT);
  });

  it("rejects non-finite stored values and falls back to defaults", async () => {
    window.localStorage.setItem(
      KEY,
      JSON.stringify({ width: Number.NaN, height: Number.POSITIVE_INFINITY }),
    );
    const { settingsModalSize } = await freshModule();
    const s = get(settingsModalSize);
    expect(Number.isFinite(s.width)).toBe(true);
    expect(Number.isFinite(s.height)).toBe(true);
  });

  it("rejects stringified numbers (no coercion) and falls back", async () => {
    window.localStorage.setItem(KEY, JSON.stringify({ width: "900", height: "700" }));
    const { settingsModalSize } = await freshModule();
    const s = get(settingsModalSize);
    expect(Number.isFinite(s.width)).toBe(true);
    expect(Number.isFinite(s.height)).toBe(true);
  });

  it("falls back to defaults when stored JSON is malformed", async () => {
    window.localStorage.setItem(KEY, "{not json");
    const { settingsModalSize } = await freshModule();
    const s = get(settingsModalSize);
    expect(Number.isFinite(s.width)).toBe(true);
    expect(Number.isFinite(s.height)).toBe(true);
  });

  it("clamps an oversized persisted width on load", async () => {
    window.localStorage.setItem(KEY, JSON.stringify({ width: 99999, height: 700 }));
    const { settingsModalSize, SETTINGS_MODAL_MAX_WIDTH } = await freshModule();
    expect(get(settingsModalSize).width).toBeLessThanOrEqual(SETTINGS_MODAL_MAX_WIDTH);
  });
});
