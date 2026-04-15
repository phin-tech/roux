import { describe, it, expect, vi } from "vitest";
import { resolveKey, type ResolverState } from "../resolve";
import type { Bind, KeyRef, KeymapAction, KeymapTree, ParsedKeymap, Prefix } from "$lib/bindings";

// Force mac platform for the bulk of tests; modsMatch is platform-specific
// and mac is the common case. The `non_mac_primary_modifier` test overrides.
vi.mock("$lib/platform", () => ({
  isMacPlatform: () => true,
  hasPrimaryModifier: (e: KeyboardEvent) => e.metaKey,
}));

function emptyKeymap(): ParsedKeymap {
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

function physical(mods: ("cmd" | "ctrl" | "alt" | "shift")[], code: string): KeyRef {
  return { kind: "physical", mods, code };
}

function character(mods: ("cmd" | "ctrl" | "alt" | "shift")[], key: string): KeyRef {
  return { kind: "character", mods, key };
}

function bind(key: KeyRef, action: KeymapAction): Bind {
  return { key, action };
}

function tree(
  name: string,
  binds: Bind[],
  opts: Partial<Pick<KeymapTree, "sticky" | "passthrough">> = {},
): KeymapTree {
  return { name, sticky: false, passthrough: false, hud: null, binds, ...opts };
}

function prefix(key: KeyRef, tree: string): Prefix {
  return { key, tree };
}

function state(keymap: ParsedKeymap, treePath: string[] = []): ResolverState {
  return { keymap, treePath };
}

/**
 * Build a synthetic KeyboardEvent that the resolver can inspect. Vitest's
 * jsdom supports `new KeyboardEvent`, but the constructor doesn't let us
 * set `code` directly — we use a plain object with the fields the resolver
 * reads and cast to KeyboardEvent.
 */
function keydown(opts: {
  key: string;
  code?: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}): KeyboardEvent {
  return {
    key: opts.key,
    code: opts.code ?? "",
    metaKey: opts.metaKey ?? false,
    ctrlKey: opts.ctrlKey ?? false,
    altKey: opts.altKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    type: "keydown",
  } as unknown as KeyboardEvent;
}

const always = (_id: string) => true;
const never = (_id: string) => false;

describe("resolveKey — direct binds", () => {
  it("matches a physical direct bind", () => {
    const km = emptyKeymap();
    km.directBinds.push(
      bind(physical(["cmd"], "KeyK"), { kind: "command", id: "app.command-palette" }),
    );
    const r = resolveKey(keydown({ key: "k", code: "KeyK", metaKey: true }), state(km), always);
    expect(r).toEqual({
      kind: "chord",
      action: { kind: "command", id: "app.command-palette" },
      keepTreeOpen: false,
    });
  });

  it("matches Alt+KeyH regardless of e.key (macOS Option produces ˙)", () => {
    const km = emptyKeymap();
    km.directBinds.push(
      bind(physical(["alt"], "KeyH"), { kind: "command", id: "pane.focus-left" }),
    );
    const r = resolveKey(
      keydown({ key: "˙", code: "KeyH", altKey: true }),
      state(km),
      always,
    );
    expect(r.kind).toBe("chord");
  });

  it("no match returns none", () => {
    const km = emptyKeymap();
    km.directBinds.push(
      bind(physical(["cmd"], "KeyK"), { kind: "command", id: "app.command-palette" }),
    );
    const r = resolveKey(keydown({ key: "j", code: "KeyJ", metaKey: true }), state(km), always);
    expect(r).toEqual({ kind: "none" });
  });

  it("requires exact modifier match — extra shift fails", () => {
    const km = emptyKeymap();
    km.directBinds.push(
      bind(physical(["cmd"], "KeyK"), { kind: "command", id: "app.command-palette" }),
    );
    const r = resolveKey(
      keydown({ key: "K", code: "KeyK", metaKey: true, shiftKey: true }),
      state(km),
      always,
    );
    expect(r).toEqual({ kind: "none" });
  });

  it("unavailable command falls through to none", () => {
    const km = emptyKeymap();
    km.directBinds.push(
      bind(physical(["cmd"], "KeyK"), { kind: "command", id: "app.command-palette" }),
    );
    const r = resolveKey(keydown({ key: "k", code: "KeyK", metaKey: true }), state(km), never);
    expect(r).toEqual({ kind: "none" });
  });
});

describe("resolveKey — prefix → tree", () => {
  it("prefix match with no tree active enters the tree", () => {
    const km = emptyKeymap();
    km.trees.push(tree("leader", []));
    km.prefixes.push(prefix(physical(["cmd"], "Semicolon"), "leader"));
    const r = resolveKey(
      keydown({ key: ";", code: "Semicolon", metaKey: true }),
      state(km),
      always,
    );
    expect(r).toEqual({ kind: "enterTree", tree: "leader" });
  });

  it("tree bind inside active tree fires as chord", () => {
    const km = emptyKeymap();
    km.trees.push(
      tree("leader", [
        bind(character([], "h"), { kind: "command", id: "pane.focus-left" }),
      ]),
    );
    const r = resolveKey(keydown({ key: "h" }), state(km, ["leader"]), always);
    expect(r).toEqual({
      kind: "chord",
      action: { kind: "command", id: "pane.focus-left" },
      keepTreeOpen: false,
    });
  });

  it("sticky tree keeps tree open after firing a chord", () => {
    const km = emptyKeymap();
    km.trees.push(
      tree(
        "resize",
        [bind(character([], "h"), { kind: "command", id: "pane.resize-left" })],
        { sticky: true },
      ),
    );
    const r = resolveKey(keydown({ key: "h" }), state(km, ["resize"]), always);
    expect(r).toEqual({
      kind: "chord",
      action: { kind: "command", id: "pane.resize-left" },
      keepTreeOpen: true,
    });
  });

  it("prefix-within-prefix rearms (tree bind does not match)", () => {
    const km = emptyKeymap();
    km.trees.push(tree("leader", []));
    km.prefixes.push(prefix(physical(["cmd"], "Semicolon"), "leader"));
    const r = resolveKey(
      keydown({ key: ";", code: "Semicolon", metaKey: true }),
      state(km, ["leader"]),
      always,
    );
    expect(r).toEqual({ kind: "enterTree", tree: "leader" });
  });

  it("tree bind wins over prefix match of the same key", () => {
    const km = emptyKeymap();
    km.trees.push(
      tree("tmux", [
        bind(physical(["ctrl"], "KeyB"), { kind: "command", id: "session.new" }),
      ]),
    );
    km.prefixes.push(prefix(physical(["ctrl"], "KeyB"), "tmux"));
    const r = resolveKey(
      keydown({ key: "b", code: "KeyB", ctrlKey: true }),
      state(km, ["tmux"]),
      always,
    );
    expect(r).toEqual({
      kind: "chord",
      action: { kind: "command", id: "session.new" },
      keepTreeOpen: false,
    });
  });

  it("enter-tree action drills into the nested tree (append path)", () => {
    const km = emptyKeymap();
    km.trees.push(tree("leader", [bind(character([], "w"), { kind: "enterTree", tree: "panes" })]));
    km.trees.push(tree("panes", []));
    const r = resolveKey(keydown({ key: "w" }), state(km, ["leader"]), always);
    expect(r).toEqual({ kind: "drillInto", tree: "panes" });
  });
});

describe("resolveKey — Escape and passthrough", () => {
  it("Escape exits a non-sticky tree", () => {
    const km = emptyKeymap();
    km.trees.push(tree("leader", []));
    const r = resolveKey(keydown({ key: "Escape" }), state(km, ["leader"]), always);
    expect(r).toEqual({ kind: "exit" });
  });

  it("Escape exits a sticky tree with no Escape binding", () => {
    const km = emptyKeymap();
    km.trees.push(tree("resize", [], { sticky: true }));
    const r = resolveKey(keydown({ key: "Escape" }), state(km, ["resize"]), always);
    expect(r).toEqual({ kind: "exit" });
  });

  it("Escape fires explicit binding if present", () => {
    const km = emptyKeymap();
    km.trees.push(
      tree(
        "locked",
        [
          bind(physical([], "Escape"), {
            kind: "command",
            id: "keymap.exit-tree",
          }),
        ],
        { sticky: true, passthrough: true },
      ),
    );
    const r = resolveKey(
      keydown({ key: "Escape", code: "Escape" }),
      state(km, ["locked"]),
      always,
    );
    expect(r).toEqual({
      kind: "chord",
      action: { kind: "command", id: "keymap.exit-tree" },
      keepTreeOpen: true,
    });
  });

  it("passthrough tree passes unbound keys through", () => {
    const km = emptyKeymap();
    km.trees.push(tree("locked", [], { sticky: true, passthrough: true }));
    const r = resolveKey(keydown({ key: "a", code: "KeyA" }), state(km, ["locked"]), always);
    expect(r).toEqual({ kind: "passthrough" });
  });

  it("non-passthrough tree drops unbound keys (none)", () => {
    const km = emptyKeymap();
    km.trees.push(tree("leader", []));
    const r = resolveKey(keydown({ key: "a", code: "KeyA" }), state(km, ["leader"]), always);
    expect(r).toEqual({ kind: "none" });
  });

  it("bound key in passthrough tree still wins over terminal", () => {
    const km = emptyKeymap();
    km.trees.push(
      tree(
        "locked",
        [bind(physical(["ctrl"], "KeyC"), { kind: "command", id: "session.close" })],
        { sticky: true, passthrough: true },
      ),
    );
    const r = resolveKey(
      keydown({ key: "c", code: "KeyC", ctrlKey: true }),
      state(km, ["locked"]),
      always,
    );
    expect(r.kind).toBe("chord");
  });
});

describe("resolveKey — edge cases", () => {
  it("modifier-only keydown returns none", () => {
    const km = emptyKeymap();
    km.directBinds.push(
      bind(physical(["cmd"], "KeyK"), { kind: "command", id: "app.command-palette" }),
    );
    const r = resolveKey(keydown({ key: "Meta", metaKey: true }), state(km), always);
    expect(r).toEqual({ kind: "none" });
  });

  it("character bind matches shifted punctuation", () => {
    const km = emptyKeymap();
    km.trees.push(
      tree("tmux", [
        bind(character([], "%"), { kind: "command", id: "pane.split-vertical" }),
      ]),
    );
    // Shift+5 on US layouts produces %; e.key === "%"
    const r = resolveKey(
      keydown({ key: "%", code: "Digit5", shiftKey: true }),
      state(km, ["tmux"]),
      always,
    );
    expect(r.kind).toBe("chord");
  });

  it("named key binds match via e.code", () => {
    const km = emptyKeymap();
    km.directBinds.push(
      bind(physical([], "Escape"), { kind: "command", id: "keymap.exit-tree" }),
    );
    const r = resolveKey(keydown({ key: "Escape", code: "Escape" }), state(km), always);
    expect(r.kind).toBe("chord");
  });
});
