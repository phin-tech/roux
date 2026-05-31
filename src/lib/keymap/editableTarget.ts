export function eventTargetIsEditable(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;

  if (target.closest("input, textarea, select, .cm-editor")) return true;

  for (let current: Element | null = target; current; current = current.parentElement) {
    if (!(current instanceof HTMLElement)) continue;
    if (current.isContentEditable) return true;

    const value = current.getAttribute("contenteditable");
    if (value == null) continue;

    const normalized = value.trim().toLowerCase();
    if (normalized === "false") return false;
    if (normalized === "" || normalized === "true" || normalized === "plaintext-only") return true;
  }

  return false;
}
