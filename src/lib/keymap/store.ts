import { writable, derived, get } from "svelte/store";
import { commands } from "$lib/bindings";
import type { KeymapAction, KeymapTree, ParsedKeymap } from "$lib/bindings";
import { currentTree, keyMatches } from "./resolve";
import { logError } from "$lib/logging";
import { notificationsPush } from "$lib/tauri";

function defaultKeymap(): ParsedKeymap {
  return {
    presetRef: null,
    hudDefault: { kind: "always" },
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

/** Whether the HUD should be visible right now. Drives `<KeymapHud>`. */
export const hudVisible = derived(keymapRuntime, ($r) => $r.treePath.length > 0);

// ---------------------------------------------------------------------------
// mutations
// ---------------------------------------------------------------------------

export function enterTree(name: string): void {
  keymapRuntime.update((r) => {
    const target = r.keymap.trees.find((t) => t.name === name);
    if (!target) return r; // unknown tree — should have produced a load-time warning
    return { ...r, treePath: [...r.treePath, name] };
  });
}

export function exitTree(): void {
  keymapRuntime.update((r) => ({ ...r, treePath: [] }));
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
