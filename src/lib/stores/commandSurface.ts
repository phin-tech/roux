import { get, writable } from "svelte/store";
import { focusedPaneId, setLogicalFocus } from "$lib/panes/focus";
import { paneInstances } from "$lib/panes/instances";

export type CommandSurfaceMode = "palette" | "leader";

export interface CommandSurfaceState {
  open: boolean;
  mode: CommandSurfaceMode;
  returnFocusPaneId: string | null;
  leaderSequence: string[];
  leaderPromptCommandId: string | null;
  leaderPromptValue: string;
}

const INITIAL_STATE: CommandSurfaceState = {
  open: false,
  mode: "palette",
  returnFocusPaneId: null,
  leaderSequence: [],
  leaderPromptCommandId: null,
  leaderPromptValue: "",
};

export const commandSurface = writable<CommandSurfaceState>(INITIAL_STATE);

function openCommandSurface(mode: CommandSurfaceMode): void {
  const current = get(commandSurface);
  if (current.open) {
    commandSurface.set({
      ...current,
      mode,
      leaderSequence: [],
      leaderPromptCommandId: null,
      leaderPromptValue: "",
    });
    return;
  }

  commandSurface.set({
    open: true,
    mode,
    returnFocusPaneId: get(focusedPaneId),
    leaderSequence: [],
    leaderPromptCommandId: null,
    leaderPromptValue: "",
  });
}

export function openCommandPalette(): void {
  openCommandSurface("palette");
}

export function openLeaderMode(): void {
  openCommandSurface("leader");
}

export function toggleCommandSurface(mode: CommandSurfaceMode): void {
  const current = get(commandSurface);
  if (current.open && current.mode === mode) {
    closeCommandSurface();
    return;
  }
  openCommandSurface(mode);
}

export function setLeaderSequence(sequence: string[]): void {
  commandSurface.update((current) => {
    if (!current.open || current.mode !== "leader") return current;
    return {
      ...current,
      leaderSequence: sequence,
      leaderPromptCommandId: null,
      leaderPromptValue: "",
    };
  });
}

/**
 * Open the leader-prompt surface (the text input used for commands with
 * `onInput`, e.g. pane.rename). Also opens the surface in leader mode if
 * it wasn't already — keymap dispatch fires this directly from a chord,
 * which used to rely on the surface being pre-opened by `Cmd+;`.
 */
export function openLeaderPrompt(commandId: string, initialValue: string = ""): void {
  commandSurface.update((current) => ({
    ...current,
    open: true,
    mode: "leader",
    leaderPromptCommandId: commandId,
    leaderPromptValue: initialValue,
  }));
}

export function setLeaderPromptValue(value: string): void {
  commandSurface.update((current) => {
    if (!current.open || current.mode !== "leader" || !current.leaderPromptCommandId) return current;
    return {
      ...current,
      leaderPromptValue: value,
    };
  });
}

export function closeCommandSurface(): void {
  const { returnFocusPaneId } = get(commandSurface);
  commandSurface.set(INITIAL_STATE);

  if (returnFocusPaneId && get(paneInstances).has(returnFocusPaneId)) {
    setLogicalFocus(returnFocusPaneId);
    return;
  }

  if (get(focusedPaneId) === returnFocusPaneId) {
    setLogicalFocus(null);
  }
}

export function resetCommandSurface(): void {
  commandSurface.set(INITIAL_STATE);
}
