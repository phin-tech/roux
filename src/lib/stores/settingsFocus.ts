import { writable } from "svelte/store";

export type SettingsFocus =
  | { category: "externalTools"; externalToolId?: string | null }
  | { category: string };

export const settingsFocus = writable<SettingsFocus | null>(null);

export function focusExternalToolSettings(toolId: string): void {
  settingsFocus.set({ category: "externalTools", externalToolId: toolId });
}
