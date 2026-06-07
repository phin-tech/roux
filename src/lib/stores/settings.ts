import { writable } from "svelte/store";
import type { RouxSettings } from "../types";
import { DEFAULT_SETTINGS } from "../types";
import type { KanbanSettings, StartupTarget } from "$lib/bindings";
import { normalizeTheme } from "$lib/themes";
import { setUserProfiles } from "$lib/panes/profiles";
import { normalizeKanbanSettings } from "$lib/workItems/workflow";
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
  value: RouxSettings[K],
) {
  updateSettingsDraft((s) => ({ ...s, [key]: value }));
}

export function updateSettingsDraft(
  updater: (current: RouxSettings) => RouxSettings,
): void {
  settings.update((s) => {
    const updated = updater(s);

    scheduleSettingsPersist(updated);

    return updated;
  });
}

export function setDefaultAgentProfile(profileId: string): void {
  updateSettingsDraft((s) => ({
    ...s,
    defaultAgentProfile: profileId,
  }));
}

export function setStartupTarget(target: StartupTarget): void {
  updateSettingsDraft((s) => {
    const startupExternalToolId =
      target === "externalTool" ? nextStartupExternalToolId(s) : null;
    return {
      ...s,
      startupTarget: target,
      startupExternalToolId,
      kanban: {
        ...kanbanSettings(s),
        startupSidebar: legacyKanbanStartupForTarget(target),
      },
    };
  });
}

function scheduleSettingsPersist(updated: RouxSettings): void {
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
}

function kanbanSettings(current: RouxSettings): KanbanSettings {
  return normalizeKanbanSettings(current.kanban);
}

function nextStartupExternalToolId(current: RouxSettings): string | null {
  const currentId = current.startupExternalToolId ?? null;
  const tools = (current.externalTools ?? []).filter(
    (tool) => tool.enabled !== false && !(tool.requiresSession ?? false),
  );
  return tools.some((tool) => tool.id === currentId)
    ? currentId
    : (tools[0]?.id ?? null);
}

function legacyKanbanStartupForTarget(
  target: StartupTarget,
): KanbanSettings["startupSidebar"] {
  switch (target) {
    case "sessionsSidebar":
      return "sessions";
    case "kanbanWide":
      return "kanban";
    case "none":
      return "none";
    case "restore":
    case "lastSession":
    case "externalTool":
      return "restore";
  }
}
