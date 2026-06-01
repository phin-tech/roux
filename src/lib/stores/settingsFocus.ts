import { writable } from "svelte/store";

export type SettingsFocus =
  | { category: "integrations"; externalToolId?: string | null }
  | { category: string };

export const settingsFocus = writable<SettingsFocus | null>(null);

export function focusExternalToolSettings(toolId: string): void {
  settingsFocus.set({ category: "integrations", externalToolId: toolId });
}
