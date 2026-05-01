/**
 * Shared Tailwind class strings for the bits-ui `Command`-based pickers used
 * in NewSessionDialog and friends. Pulled into a module so the visual design
 * stays consistent across pickers — drift here was causing subtle look-alike
 * inconsistencies (different border radii on the input row, different shadow
 * on the dropdown). Treat these as the source of truth.
 */

export const pickerShellClass =
  "relative min-w-0 rounded-md border border-border bg-bg-deep";

export const pickerInputRowClass =
  "flex min-w-0 items-center gap-2 border-b border-border px-2 py-1.5";

export const pickerInputClass =
  "min-w-0 flex-1 bg-transparent px-1 py-1 font-mono text-[12px] text-text-primary outline-none placeholder:text-text-muted";

export const pickerListClass =
  "app-scrollbar absolute left-0 right-0 z-50 overflow-y-auto border border-border bg-bg-surface p-1 shadow-lg";

export const pickerItemClass =
  "flex cursor-pointer items-center rounded-md border border-border-subtle bg-bg-surface/50 px-2.5 py-2 text-left transition-colors hover:bg-bg-hover";

export const pickerSideButtonClass =
  "cursor-pointer rounded-md border border-border-subtle bg-bg-surface px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover hover:text-text-primary disabled:opacity-50";

/** True when focus is moving to something outside `el` (or focus is lost with no next target). */
export function focusLeavingElement(el: HTMLElement, related: EventTarget | null): boolean {
  if (related == null) return true;
  if (!(related instanceof Node)) return true;
  return !el.contains(related);
}
