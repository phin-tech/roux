import { writable, derived, get } from "svelte/store";
import { commands } from "$lib/bindings";
import type { KeymapAction, KeymapTree, KeymapWarning, ParsedKeymap } from "$lib/bindings";
import { currentTree, keyMatches } from "./resolve";
import { registry } from "$lib/commands";
import { logError } from "$lib/logging";
import { notificationsPush } from "$lib/tauri";

function defaultKeymap(): ParsedKeymap {
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

interface KeymapRuntime {
  keymap: ParsedKeymap;
  /** Chain of trees entered. Tail = currently armed. Empty = no tree. */
  treePath: string[];
}

const keymapRuntime = writable<KeymapRuntime>({
  keymap: defaultKeymap(),
  treePath: [],
});

export const keymapState = { subscribe: keymapRuntime.subscribe };

/** Active tree (tail of treePath) or null if no tree is armed. */
export const activeTree = derived(keymapRuntime, ($r): KeymapTree | null =>
  currentTree($r.keymap, $r.treePath),
);

/**
 * Whether the HUD should render right now. Drives `<KeymapHud>`. Honors each
 * tree's `hud` mode (`always` / `delayed <ms>` / `never`) with fallback to
 * the document-level `hudDefault`, and ultimately `always` if neither is
 * set.
 *
 * Internally: a writable flag that enterTree/rearmTree schedule via
 * setTimeout when the mode is `delayed`, and exitTree clears.
 */
const hudVisibleWritable = writable(false);
export const hudVisible = { subscribe: hudVisibleWritable.subscribe };

let hudDelayTimer: ReturnType<typeof setTimeout> | null = null;

function cancelHudTimer(): void {
  if (hudDelayTimer !== null) {
    clearTimeout(hudDelayTimer);
    hudDelayTimer = null;
  }
}

function resolvedHudMode(runtime: KeymapRuntime): { kind: "always" } | { kind: "delayed"; ms: number } | { kind: "never" } {
  const tree = currentTree(runtime.keymap, runtime.treePath);
  if (tree?.hud) return tree.hud;
  if (runtime.keymap.hudDefault) return runtime.keymap.hudDefault;
  return { kind: "always" };
}

function applyHudModeForActiveTree(): void {
  cancelHudTimer();
  const runtime = get(keymapRuntime);
  if (runtime.treePath.length === 0) {
    hudVisibleWritable.set(false);
    return;
  }
  const mode = resolvedHudMode(runtime);
  if (mode.kind === "always") {
    hudVisibleWritable.set(true);
    return;
  }
  if (mode.kind === "never") {
    hudVisibleWritable.set(false);
    return;
  }
  // delayed: hide until the timeout elapses, then reveal IFF the tree is
  // still armed. A chord fired within the delay window exits the tree,
  // cancelling the timer via applyHudModeForActiveTree / exitTree.
  hudVisibleWritable.set(false);
  hudDelayTimer = setTimeout(() => {
    const now = get(keymapRuntime);
    if (now.treePath.length > 0) hudVisibleWritable.set(true);
    hudDelayTimer = null;
  }, mode.ms);
}

// ---------------------------------------------------------------------------
// mutations
// ---------------------------------------------------------------------------

/** Drill into a nested tree: append to `treePath`. */
export function enterTree(name: string): void {
  keymapRuntime.update((r) => {
    const target = r.keymap.trees.find((t) => t.name === name);
    if (!target) return r; // unknown tree — should have produced a load-time warning
    return { ...r, treePath: [...r.treePath, name] };
  });
  applyHudModeForActiveTree();
}

/** Arm a tree from a prefix trigger: clears any existing path and enters. */
export function rearmTree(name: string): void {
  keymapRuntime.update((r) => {
    const target = r.keymap.trees.find((t) => t.name === name);
    if (!target) return r;
    return { ...r, treePath: [name] };
  });
  applyHudModeForActiveTree();
}

export function exitTree(): void {
  cancelHudTimer();
  keymapRuntime.update((r) => ({ ...r, treePath: [] }));
  hudVisibleWritable.set(false);
}

// ---------------------------------------------------------------------------
// loading
// ---------------------------------------------------------------------------

/**
 * Load (or reload) the keymap from disk via the Rust side. On success the
 * store publishes the parsed result and any warnings surface as a
 * notification. On error the previous keymap is retained and an error
 * notification fires.
 */
export async function loadKeymap(): Promise<void> {
  const result = await commands.getKeymap();
  if (result.status === "error") {
    logError(`keymap load failed: ${result.error}`);
    void notificationsPush({
      level: "error",
      source: { type: "internal" },
      title: "Keymap failed to load",
      subtitle: null,
      body: result.error,
      sessionId: null,
      actions: [],
      dedupKey: "keymap-parse-error",
    }).catch(() => {
      // swallow; notification plumbing failing shouldn't cascade.
    });
    return;
  }
  const km = result.data;
  // Validate command IDs and tree refs against the frontend registry.
  // These checks can't run Rust-side because the registry is TS-owned.
  const validationWarnings = validateAgainstRegistry(km);
  km.warnings = [...km.warnings, ...validationWarnings];

  keymapRuntime.update(() => ({ keymap: km, treePath: [] }));
  if (km.warnings.length > 0) {
    const first = km.warnings
      .slice(0, 3)
      .map((w) => `  line ${w.line}: ${w.message}`)
      .join("\n");
    void notificationsPush({
      level: "warning",
      source: { type: "internal" },
      title: `Keymap loaded with ${km.warnings.length} warning${km.warnings.length === 1 ? "" : "s"}`,
      subtitle: null,
      body: first,
      sessionId: null,
      actions: [],
      dedupKey: "keymap-load-warnings",
    }).catch(() => {});
  }
}

function validateAgainstRegistry(km: ParsedKeymap): KeymapWarning[] {
  const out: KeymapWarning[] = [];
  // Pseudo-commands owned by the keymap module; not in the registry.
  const pseudoCommands = new Set(["keymap.exit-tree", "keymap.reload"]);
  const treeNames = new Set(km.trees.map((t) => t.name));

  function check(action: KeymapAction): string | null {
    if (action.kind === "enterTree") {
      return treeNames.has(action.tree)
        ? null
        : `references unknown tree \`${action.tree}\``;
    }
    if (pseudoCommands.has(action.id)) return null;
    return registry.get(action.id) ? null : `references unknown command \`${action.id}\``;
  }

  for (const bind of km.directBinds) {
    const err = check(bind.action);
    if (err) out.push({ message: `direct bind ${err}`, line: 0, column: 0 });
  }
  for (const tree of km.trees) {
    for (const bind of tree.binds) {
      const err = check(bind.action);
      if (err) out.push({ message: `tree "${tree.name}" bind ${err}`, line: 0, column: 0 });
    }
  }
  for (const prefix of km.prefixes) {
    if (!treeNames.has(prefix.tree)) {
      out.push({
        message: `prefix references unknown tree \`${prefix.tree}\``,
        line: 0,
        column: 0,
      });
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// shortcut display
// ---------------------------------------------------------------------------

/**
 * Return a user-facing shortcut label for a command id, derived from the
 * currently-loaded keymap. Used by `CommandPalette` to render the "press
 * this key to run" hint next to each command.
 *
 * Precedence:
 *   1. First matching direct bind → its key label.
 *   2. First `prefix → tree` chord that targets the command → `<prefix> <key>`.
 *   3. null (no shortcut registered).
 */
export function shortcutFor(commandId: string): string | null {
  const { keymap } = get(keymapRuntime);

  for (const bind of keymap.directBinds) {
    if (bind.action.kind === "command" && bind.action.id === commandId) {
      return keyLabel(bind.key);
    }
  }

  for (const prefix of keymap.prefixes) {
    const tree = keymap.trees.find((t) => t.name === prefix.tree);
    if (!tree) continue;
    for (const bind of tree.binds) {
      if (actionTargetsCommand(bind.action, commandId)) {
        return `${keyLabel(prefix.key)} ${keyLabel(bind.key)}`;
      }
    }
  }

  return null;
}

function actionTargetsCommand(action: KeymapAction, commandId: string): boolean {
  return action.kind === "command" && action.id === commandId;
}

function keyLabel(key: import("$lib/bindings").KeyRef): string {
  const mods = key.mods.map((m) => {
    switch (m) {
      case "cmd":
        return "Cmd";
      case "ctrl":
        return "Ctrl";
      case "alt":
        return "Alt";
      case "shift":
        return "Shift";
    }
  });
  const body = key.kind === "physical" ? physicalDisplay(key.code) : key.key;
  return [...mods, body].join("+");
}

function physicalDisplay(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  // Arrow / named keys render as-is; downstream code can pretty-print later.
  return code;
}

// Re-export match helper so App.svelte can check before calling resolveKey.
export { keyMatches };

// ---------------------------------------------------------------------------
// test-only
// ---------------------------------------------------------------------------

/**
 * Overwrite the store's parsed keymap without going through `loadKeymap`.
 * Resets `treePath` to empty. Intended for unit tests — do not call from
 * application code.
 */
export function __installKeymapForTest(km: ParsedKeymap): void {
  cancelHudTimer();
  keymapRuntime.set({ keymap: km, treePath: [] });
  hudVisibleWritable.set(false);
}
