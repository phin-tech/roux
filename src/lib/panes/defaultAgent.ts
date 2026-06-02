import { get } from "svelte/store";
import { settings } from "$lib/stores/settings";

export function defaultAgentProfileId(): string {
  const current = get(settings);
  const id = (current.defaultAgentProfile ?? current.kanban?.defaultAgentProfile ?? "claude").trim();
  return id || "claude";
}
