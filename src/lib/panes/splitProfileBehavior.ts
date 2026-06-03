import type { SpawnProfile } from "./profiles";

export function resolveAppDefaultSplitProfile(
  registry: ReadonlyMap<string, SpawnProfile>,
  defaultProfileId: string | null | undefined,
): SpawnProfile | null {
  const configuredId = defaultProfileId?.trim() || "claude";
  return registry.get(configuredId) ?? registry.get("claude") ?? null;
}
