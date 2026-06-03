export function isMacPlatform(): boolean {
  const platform = navigator.platform || "";
  return /Mac|iPhone|iPad|iPod/i.test(platform);
}

export function hasPrimaryModifier(e: KeyboardEvent): boolean {
  return isMacPlatform() ? e.metaKey : e.ctrlKey;
}

export function shortcutDisplayPart(part: string): string {
  switch (part) {
    case "cmd":
      return isMacPlatform() ? "\u2318" : "Ctrl";
    case "shift":
      return isMacPlatform() ? "\u21e7" : "Shift";
    case "alt":
      return isMacPlatform() ? "\u2325" : "Alt";
    case "ctrl":
      return isMacPlatform() ? "\u2303" : "Ctrl";
    default:
      return part.toUpperCase();
  }
}

export function formatShortcut(shortcut: string): string {
  return shortcut
    .split("+")
    .map(shortcutDisplayPart)
    .join(isMacPlatform() ? "" : "+");
}
