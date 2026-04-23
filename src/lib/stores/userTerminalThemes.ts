// User-supplied terminal themes loaded from
// `~/.config/roux/themes/*.itermcolors`. Backed by the
// `list_user_terminal_themes` Tauri command. Refreshed at app startup and
// on demand via the cmd-k "Reload Terminal Themes" command.

import { writable } from "svelte/store";
import { commands, type UserTerminalTheme } from "$lib/bindings";
import { logError } from "$lib/logging";

export const userTerminalThemes = writable<UserTerminalTheme[]>([]);

export async function loadUserTerminalThemes(): Promise<UserTerminalTheme[]> {
  try {
    const themes = await commands.listUserTerminalThemes();
    userTerminalThemes.set(themes);
    return themes;
  } catch (e) {
    logError("loadUserTerminalThemes failed", e);
    return [];
  }
}
