import { get } from "svelte/store";
import { settings } from "$lib/stores/settings";
import type { RouxSettings } from "$lib/types";

export function defaultAgentProfileId(): string {
  return effectiveDefaultAgentProfileId(get(settings));
}

export function effectiveDefaultAgentProfileId(
  current: Pick<RouxSettings, "defaultAgentProfile">,
): string {
  const id = (current.defaultAgentProfile ?? "claude").trim();
  return id || "claude";
}
