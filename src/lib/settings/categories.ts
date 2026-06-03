export const SETTINGS_CATEGORY_IDS = [
  "general",
  "sessions",
  "terminal",
  "agents",
  "kanban",
  "externalTools",
  "notes",
  "integrations",
  "notifications",
  "keyboard",
  "experiments",
  "advanced",
] as const;

export type SettingsCategoryId = (typeof SETTINGS_CATEGORY_IDS)[number];

const CATEGORY_ID_SET: ReadonlySet<string> = new Set(SETTINGS_CATEGORY_IDS);

export function isSettingsCategoryId(
  value: string | null | undefined,
): value is SettingsCategoryId {
  return typeof value === "string" && CATEGORY_ID_SET.has(value);
}

export function normalizeSettingsCategoryId(
  value: string | null | undefined,
): SettingsCategoryId {
  return isSettingsCategoryId(value) ? value : "general";
}
