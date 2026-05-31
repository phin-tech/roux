import type { Command } from "$lib/commands/registry";
import { eventTargetIsEditable } from "$lib/keymap/editableTarget";

export { eventTargetIsEditable };

export function commandBlockedByMainView(command: Command | undefined): boolean {
  if (!command) return false;
  return command.id.startsWith("pane.") || command.category === "Panes";
}

export function eventTargetIsInsideMainView(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("[data-main-view-root]") !== null;
}

export function eventTargetIsMainViewKeyboardOwner(target: EventTarget | null): boolean {
  return (
    eventTargetIsInsideMainView(target) ||
    target === document.body ||
    target === document.documentElement
  );
}

export function mainViewTargetShouldBypassAppKeymap(target: EventTarget | null): boolean {
  return eventTargetIsMainViewKeyboardOwner(target) && eventTargetIsEditable(target);
}
