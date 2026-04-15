import type { Bind, KeyRef, KeymapAction, KeymapTree, Modifier, ParsedKeymap, Prefix } from "$lib/bindings";
import { isMacPlatform } from "$lib/platform";

export type Resolution =
  | { kind: "none" }
  /** Prefix matched at top level, or rearm while a tree is active. Replaces treePath. */
  | { kind: "enterTree"; tree: string }
  /** Nested enter-tree action inside an already-armed tree. Appends to treePath. */
  | { kind: "drillInto"; tree: string }
  | { kind: "chord"; action: KeymapAction; keepTreeOpen: boolean }
  | { kind: "passthrough" }
  | { kind: "exit" };

export interface ResolverState {
  keymap: ParsedKeymap;
  /** Chain of trees entered, tail = currently armed. Empty = no tree. */
  treePath: string[];
}

/**
 * Match a `KeyboardEvent` against the keymap, given the current runtime
 * state. Pure — no side effects, no DOM access beyond `event` itself.
 *
 * Precedence (see design spec):
 *   1. If a tree is active:
 *      a. Tree bind wins (even if the key is also a prefix).
 *      b. Otherwise a matching prefix rearms (prefix-within-prefix).
 *      c. Otherwise Escape exits.
 *      d. Otherwise passthrough (if the tree is passthrough) or none.
 *   2. No tree active:
 *      a. Prefix match → enterTree.
 *      b. Direct bind match → chord.
 *      c. Otherwise none.
 *
 * Bindings whose command's `isCommandAvailable(id)` returns false resolve to
 * `none` so the key falls through instead of firing a greyed-out action.
 */
export function resolveKey(
  event: KeyboardEvent,
  state: ResolverState,
  isCommandAvailable: (id: string) => boolean,
): Resolution {
  const { keymap, treePath } = state;
  const activeTree = currentTree(keymap, treePath);

  // Modifier-only keydowns never fire binds.
  if (isModifierOnly(event)) return { kind: "none" };

  if (activeTree) {
    // 1a. Tree bind match.
    const matched = findBind(activeTree.binds, event);
    if (matched && actionIsAvailable(matched.action, isCommandAvailable)) {
      // enter-tree action: promote to nested tree, appending to the path
      // so leader → leader-panes preserves the breadcrumb.
      if (matched.action.kind === "enterTree") {
        return { kind: "drillInto", tree: matched.action.tree };
      }
      return {
        kind: "chord",
        action: matched.action,
        keepTreeOpen: activeTree.sticky ?? false,
      };
    }

    // 1b. Prefix rearm (prefix-within-prefix).
    const prefix = findPrefix(keymap.prefixes, event);
    if (prefix) {
      return { kind: "enterTree", tree: prefix.tree };
    }

    // 1c. Escape exits (universal safety net).
    if (event.key === "Escape") {
      return { kind: "exit" };
    }

    // 1d. Passthrough trees pass unbound keys to the terminal.
    if (activeTree.passthrough) {
      return { kind: "passthrough" };
    }

    // 1e. Unbound key in a non-passthrough tree is dropped; tree stays armed.
    return { kind: "none" };
  }

  // 2a. No tree active — check prefixes.
  const prefix = findPrefix(keymap.prefixes, event);
  if (prefix) {
    return { kind: "enterTree", tree: prefix.tree };
  }

  // 2b. Direct bind.
  const direct = findBind(keymap.directBinds, event);
  if (direct && actionIsAvailable(direct.action, isCommandAvailable)) {
    return {
      kind: "chord",
      action: direct.action,
      keepTreeOpen: false,
    };
  }

  return { kind: "none" };
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

export function currentTree(
  keymap: ParsedKeymap,
  treePath: string[],
): KeymapTree | null {
  if (treePath.length === 0) return null;
  const name = treePath[treePath.length - 1];
  return keymap.trees.find((t) => t.name === name) ?? null;
}

function actionIsAvailable(
  action: KeymapAction,
  isCommandAvailable: (id: string) => boolean,
): boolean {
  if (action.kind === "enterTree") return true;
  return isCommandAvailable(action.id);
}

function findBind(binds: Bind[], event: KeyboardEvent): Bind | null {
  for (const bind of binds) {
    if (keyMatches(bind.key, event)) return bind;
  }
  return null;
}

function findPrefix(prefixes: Prefix[], event: KeyboardEvent): Prefix | null {
  for (const p of prefixes) {
    if (keyMatches(p.key, event)) return p;
  }
  return null;
}

export function keyMatches(key: KeyRef, event: KeyboardEvent): boolean {
  if (key.kind === "physical") {
    if (!modsMatch(key.mods, event, true)) return false;
    // Physical binds on named keys also match `e.key` (named keys have no
    // separate shifted variant and browsers report Escape/Tab/Arrow* on both
    // `e.code` and `e.key`).
    return event.code === key.code || event.key === key.code;
  }
  // Character binds: the logical character (e.key) already encodes Shift,
  // so we don't compare shiftKey. `%` matches regardless of whether the
  // user pressed Shift+5 or used a keyboard where % is unshifted.
  if (!modsMatch(key.mods, event, false)) return false;
  return event.key === key.key;
}

function modsMatch(
  mods: Modifier[],
  event: KeyboardEvent,
  checkShift: boolean,
): boolean {
  // `cmd` is platform-dispatched: Meta on macOS, Ctrl elsewhere. On non-mac
  // `cmd` and `ctrl` collapse to the same physical key (Ctrl), so either
  // modifier name in the keymap maps to the same event flag.
  let wantsPrimary = false;          // Cmd on mac, Ctrl on non-mac
  let wantsCtrlSecondary = false;    // mac-only: Ctrl-the-other-key
  let wantsAlt = false;
  let wantsShift = false;

  for (const m of mods) {
    switch (m) {
      case "cmd":
        wantsPrimary = true;
        break;
      case "ctrl":
        if (isMacPlatform()) wantsCtrlSecondary = true;
        else wantsPrimary = true;
        break;
      case "alt":
        wantsAlt = true;
        break;
      case "shift":
        wantsShift = true;
        break;
    }
  }

  const primary = isMacPlatform() ? event.metaKey : event.ctrlKey;
  const ctrlSecondary = isMacPlatform() ? event.ctrlKey : false;

  if (primary !== wantsPrimary) return false;
  if (ctrlSecondary !== wantsCtrlSecondary) return false;
  if (event.altKey !== wantsAlt) return false;
  if (checkShift && event.shiftKey !== wantsShift) return false;
  return true;
}

function isModifierOnly(event: KeyboardEvent): boolean {
  return (
    event.key === "Meta" ||
    event.key === "Control" ||
    event.key === "Alt" ||
    event.key === "Shift"
  );
}
