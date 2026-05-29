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
import { refreshWorktrunkDetection } from "$lib/stores/worktrunkDetection";

export const settings = writable<RouxSettings>(DEFAULT_SETTINGS);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
// Track the last persisted override so we only re-probe when it
// actually changes — otherwise every unrelated setting tweak would
// force a subprocess spawn.
let lastWorktrunkBinaryPath: string | null | undefined = undefined;

export async function initSettings(): Promise<RouxSettings> {
  const raw = await getSettings();
  const loaded = { ...raw, theme: normalizeTheme(raw.theme) };
  settings.set(loaded);
  setUserProfiles(loaded.spawnProfiles);
  lastWorktrunkBinaryPath = loaded.worktrunkBinaryPath;

  // Listen for changes from backend
  await onSettingsChanged((updated) => {
    const next = { ...updated, theme: normalizeTheme(updated.theme) };
    settings.set(next);
    setUserProfiles(next.spawnProfiles);
    if (next.worktrunkBinaryPath !== lastWorktrunkBinaryPath) {
      lastWorktrunkBinaryPath = next.worktrunkBinaryPath;
      void refreshWorktrunkDetection();
    }
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
      void updateSettingsApi(updated).then(() => {
        // Changes to the wt binary override invalidate the cached
        // detection state (ActivityRail/WorktrunkPanel/NewSessionDialog
        // all key off it), so reprobe so UI reflects reality without a
        // restart.
        if (updated.worktrunkBinaryPath !== lastWorktrunkBinaryPath) {
          lastWorktrunkBinaryPath = updated.worktrunkBinaryPath;
          void refreshWorktrunkDetection();
        }
      });
    }, 500);

    return updated;
  });
}
