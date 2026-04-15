import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { get } from "svelte/store";
import {
  enterTree,
  rearmTree,
  exitTree,
  hudVisible,
  keymapState,
} from "../store";
import type { HudMode, KeymapTree, ParsedKeymap } from "$lib/bindings";

// Replace the Tauri get_keymap command (not exercised in these tests) and
// the command registry so validation doesn't spew warnings.
vi.mock("$lib/bindings", async () => {
  return {
    commands: {
      getKeymap: async () => ({ status: "ok", data: emptyParsed() }),
    },
  };
});
vi.mock("$lib/commands", () => ({
  registry: {
    get: (_id: string) => ({ id: _id }),
  },
}));
vi.mock("$lib/tauri", () => ({
  notificationsPush: vi.fn(),
}));
vi.mock("$lib/logging", () => ({
  logError: vi.fn(),
}));

function emptyParsed(): ParsedKeymap {
  return {
    presetRef: null,
    hudDefault: null,
    directBinds: [],
    unbinds: [],
    trees: [],
    prefixes: [],
    warnings: [],
  };
}

function tree(
  name: string,
  hud: HudMode | null = null,
): KeymapTree {
  return { name, sticky: false, passthrough: false, hud, binds: [] };
}

/**
 * Swap in a parsed keymap directly, bypassing `loadKeymap`. Used by tests
 * that want to exercise enterTree/exitTree with a controlled shape.
 */
function seedKeymap(km: ParsedKeymap): void {
  // The store module isn't exporting its writable, so we go through the
  // public mutators. Reset, then overwrite via a fresh enter-from-scratch.
  exitTree();
  // Hack: mutate the keymap via a reload-like path. The store only exposes
  // treePath mutation; to install a full keymap we need to reach into the
  // module. For these tests we simulate by calling enterTree on the
  // current (default) store after monkey-patching it — but that requires
  // the writable. So for now, we exercise the store indirectly via a
  // dedicated test helper exported below.
  __installKeymapForTest(km);
}

// The store exports a test-only install hook, defined below in store.ts.
import { __installKeymapForTest } from "../store";

describe("HUD modes", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    exitTree();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("always mode reveals HUD immediately", () => {
    seedKeymap({
      ...emptyParsed(),
      hudDefault: { kind: "always" },
      trees: [tree("leader")],
    });
    enterTree("leader");
    expect(get(hudVisible)).toBe(true);
  });

  it("never mode keeps HUD hidden", () => {
    seedKeymap({
      ...emptyParsed(),
      hudDefault: { kind: "never" },
      trees: [tree("leader")],
    });
    enterTree("leader");
    expect(get(hudVisible)).toBe(false);
  });

  it("delayed mode hides initially, reveals after timeout", () => {
    seedKeymap({
      ...emptyParsed(),
      hudDefault: { kind: "delayed", ms: 500 },
      trees: [tree("leader")],
    });
    enterTree("leader");
    expect(get(hudVisible)).toBe(false);
    vi.advanceTimersByTime(499);
    expect(get(hudVisible)).toBe(false);
    vi.advanceTimersByTime(1);
    expect(get(hudVisible)).toBe(true);
  });

  it("delayed mode does not reveal if tree exits before timeout", () => {
    seedKeymap({
      ...emptyParsed(),
      hudDefault: { kind: "delayed", ms: 500 },
      trees: [tree("leader")],
    });
    enterTree("leader");
    vi.advanceTimersByTime(100);
    exitTree();
    vi.advanceTimersByTime(500);
    expect(get(hudVisible)).toBe(false);
  });

  it("tree-level hud overrides document default", () => {
    seedKeymap({
      ...emptyParsed(),
      hudDefault: { kind: "always" },
      trees: [tree("quiet", { kind: "never" })],
    });
    enterTree("quiet");
    expect(get(hudVisible)).toBe(false);
  });

  it("rearm cancels previous HUD timer and re-applies for new tree", () => {
    seedKeymap({
      ...emptyParsed(),
      hudDefault: { kind: "always" },
      trees: [
        tree("a", { kind: "delayed", ms: 500 }),
        tree("b"),
      ],
    });
    enterTree("a");
    expect(get(hudVisible)).toBe(false);
    // Rearm to `b` which inherits `always` default
    rearmTree("b");
    expect(get(hudVisible)).toBe(true);
    // Advancing past the old timer must NOT flip visible back (old tree was replaced)
    vi.advanceTimersByTime(1000);
    expect(get(hudVisible)).toBe(true);
  });
});

describe("keymap state", () => {
  beforeEach(() => {
    exitTree();
  });

  it("enterTree appends to treePath", () => {
    seedKeymap({
      ...emptyParsed(),
      trees: [tree("a"), tree("b")],
    });
    enterTree("a");
    enterTree("b");
    expect(get(keymapState).treePath).toEqual(["a", "b"]);
  });

  it("rearmTree replaces treePath", () => {
    seedKeymap({
      ...emptyParsed(),
      trees: [tree("a"), tree("b")],
    });
    enterTree("a");
    enterTree("b");
    rearmTree("a");
    expect(get(keymapState).treePath).toEqual(["a"]);
  });

  it("exitTree clears treePath", () => {
    seedKeymap({
      ...emptyParsed(),
      trees: [tree("a")],
    });
    enterTree("a");
    exitTree();
    expect(get(keymapState).treePath).toEqual([]);
  });

  it("enterTree on unknown tree is a no-op", () => {
    seedKeymap({
      ...emptyParsed(),
      trees: [tree("a")],
    });
    enterTree("nonexistent");
    expect(get(keymapState).treePath).toEqual([]);
  });
});
