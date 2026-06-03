import { writable } from "svelte/store";
import type { SettingsCategoryId } from "$lib/settings/categories";

export type SettingsFocus =
  | { category: "externalTools"; externalToolId?: string | null }
  | { category: SettingsCategoryId };

export const settingsFocus = writable<SettingsFocus | null>(null);

export function focusExternalToolSettings(toolId: string): void {
  settingsFocus.set({ category: "externalTools", externalToolId: toolId });
}
