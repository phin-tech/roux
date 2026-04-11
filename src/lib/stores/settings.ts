import { writable } from "svelte/store";
import type { RouxSettings } from "../types";
import { DEFAULT_SETTINGS } from "../types";
import { normalizeTheme } from "$lib/themes";
import { setUserProfiles } from "$lib/panes/profiles";
import {
  getSettings,
  updateSettings as updateSettingsApi,
  onSettingsChanged,
} from "../tauri";

export const settings = writable<RouxSettings>(DEFAULT_SETTINGS);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

export async function initSettings(): Promise<RouxSettings> {
  const raw = await getSettings();
  const loaded = { ...raw, theme: normalizeTheme(raw.theme) };
  settings.set(loaded);
  setUserProfiles(loaded.spawnProfiles);

  // Listen for changes from backend
  await onSettingsChanged((updated) => {
    const next = { ...updated, theme: normalizeTheme(updated.theme) };
    settings.set(next);
    setUserProfiles(next.spawnProfiles);
  });

  return loaded;
}

export function updateSetting<K extends keyof RouxSettings>(
  key: K,
  value: RouxSettings[K]
) {
  settings.update((s) => {
    const updated = { ...s, [key]: value };

    // Debounced save to backend
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      updateSettingsApi(updated);
    }, 500);

    return updated;
  });
}
