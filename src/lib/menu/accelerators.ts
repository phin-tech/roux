// Accelerator translation for native menu items.
//
// The keymap store emits shortcut strings like "cmd+shift+d" (lowercase,
// `+`-joined). Tauri's MenuItem.accelerator expects the tao/accelerator
// syntax: "CmdOrCtrl+Shift+D". We bridge the two here, and also produce the
// same canonical string from a DOM KeyboardEvent so App.svelte can dedupe
// OS-level menu shortcuts against its own keymap dispatch.

const MOD_MAP: Record<string, string> = {
  cmd: "CmdOrCtrl",
  command: "CmdOrCtrl",
  meta: "CmdOrCtrl",
  ctrl: "Control",
  control: "Control",
  shift: "Shift",
  alt: "Alt",
  option: "Alt",
  super: "Super",
};

const MOD_ORDER = ["CmdOrCtrl", "Control", "Alt", "Shift", "Super"];

const KEY_SUBSTITUTIONS: Record<string, string> = {
  " ": "Space",
  "\\": "Backslash",
  "/": "Slash",
  ",": "Comma",
  ".": "Period",
  ";": "Semicolon",
  "'": "Quote",
  "`": "Backquote",
  "-": "Minus",
  "=": "Equal",
  "[": "BracketLeft",
  "]": "BracketRight",
};

function normalizeBody(body: string): string | null {
  if (!body) return null;
  const trimmed = body.trim();
  if (!trimmed) return null;

  // Named keys from KeyboardEvent.key (Escape, ArrowLeft, Enter, F1, etc.)
  // already match Tauri's accelerator body. Pass them through as-is when
  // they're multi-character and alphabetical.
  if (trimmed.length > 1 && /^[A-Za-z][A-Za-z0-9]*$/.test(trimmed)) {
    return trimmed;
  }

  // Single digits and letters — Tauri wants the bare character, uppercased.
  if (/^[0-9]$/.test(trimmed)) return trimmed;
  if (/^[a-zA-Z]$/.test(trimmed)) return trimmed.toUpperCase();

  const sub = KEY_SUBSTITUTIONS[trimmed];
  if (sub) return sub;

  // Fall back to the raw token (e.g. "F12"). Tauri will reject unknown
  // ones; returning null would drop the accelerator entirely, so we let it
  // through and log nothing.
  return trimmed;
}

function sortMods(mods: string[]): string[] {
  return mods
    .slice()
    .sort((a, b) => MOD_ORDER.indexOf(a) - MOD_ORDER.indexOf(b));
}

/**
 * Translate a keymap-store shortcut string to Tauri's accelerator format.
 * Returns null when the input is empty, malformed, or represents a chord
 * (leader-tree prefixes like "cmd+; b d" — not expressible as an OS
 * accelerator).
 */
export function toTauriAccelerator(shortcut: string | null): string | null {
  if (!shortcut) return null;
  // Chord shortcuts (space-separated) aren't expressible as OS accelerators.
  if (shortcut.includes(" ")) return null;

  const parts = shortcut.split("+").map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0) return null;

  const mods: string[] = [];
  const bodies: string[] = [];
  for (const part of parts) {
    const lower = part.toLowerCase();
    const mod = MOD_MAP[lower];
    if (mod) {
      if (!mods.includes(mod)) mods.push(mod);
    } else {
      bodies.push(part);
    }
  }

  if (bodies.length !== 1) return null;
  const body = normalizeBody(bodies[0]);
  if (!body) return null;

  return [...sortMods(mods), body].join("+");
}

/**
 * Canonical accelerator string for a KeyboardEvent, matching the form that
 * `toTauriAccelerator` produces. Used by App.svelte to check whether an
 * incoming keydown corresponds to an accelerator the OS menu already
 * claimed — preventing double dispatch of commands like Cmd+N.
 *
 * Returns null for bare modifier presses and keys that don't resolve to a
 * non-empty body.
 */
export function eventToAccelerator(e: KeyboardEvent): string | null {
  const key = e.key;
  if (!key) return null;
  // Bare modifiers ("Meta", "Shift", etc.) aren't accelerators.
  if (["Meta", "Control", "Alt", "Shift", "Super", "Hyper", "OS"].includes(key)) {
    return null;
  }

  const mods: string[] = [];
  if (e.metaKey || e.ctrlKey) mods.push("CmdOrCtrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");

  // Modifier-less keystrokes never match menu accelerators — Tauri won't
  // register a plain-letter shortcut for a MenuItem.
  if (mods.length === 0) return null;

  const body = normalizeBody(key);
  if (!body) return null;

  return [...sortMods(mods), body].join("+");
}
