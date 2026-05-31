import type { Command } from "$lib/commands/registry";

export function commandBlockedByMainView(command: Command | undefined): boolean {
  if (!command) return false;
  return command.id.startsWith("pane.") || command.category === "Panes";
}

export function eventTargetIsInsideMainView(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("[data-main-view-root]") !== null;
}

export function eventTargetIsEditable(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const editable = target.closest("input, textarea, select, [contenteditable='true']");
  return editable !== null;
}
