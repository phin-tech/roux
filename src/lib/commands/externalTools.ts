import { get } from "svelte/store";
import { registry, type CommandItem } from "./registry";
import type { ExternalTool } from "$lib/bindings";
import { settings } from "$lib/stores/settings";
import {
  externalToolDisabledReason,
  listEnabledExternalTools,
  openExternalTool,
} from "$lib/stores/externalTools";
import { logError } from "$lib/logging";

const DIRECT_PREFIX = "external-tools.open.";
let registeredDirect = new Set<string>();
let syncedDirectSignature: string | null = null;
let subscribed = false;

export function registerExternalToolCommands(): void {
  registry.register({
    id: "external-tools.open",
    label: "Open External Tool",
    category: "External Tools",
    getItems: externalToolItems,
    inputPlaceholder: "Pick an external tool...",
  });

  syncDirectCommands(get(settings).externalTools ?? []);
  if (!subscribed) {
    subscribed = true;
    settings.subscribe((next) => syncDirectCommands(next.externalTools ?? []));
  }
}

function externalToolItems(): CommandItem[] {
  return listEnabledExternalTools().map((tool) => ({
    id: tool.id,
    label: tool.name,
    description: tool.surface === "web" ? "Web" : "Terminal",
    icon: tool.surface === "web" ? undefined : "terminal",
    disabledReason: externalToolDisabledReason(tool),
    action: () => runTool(tool.id),
  }));
}

function syncDirectCommands(tools: ExternalTool[]): void {
  const signature = directCommandSignature(tools);
  if (signature === syncedDirectSignature) return;

  for (const id of registeredDirect) registry.unregister(id);
  registeredDirect = new Set();
  syncedDirectSignature = signature;

  for (const tool of tools) {
    if (tool.enabled === false) continue;
    const commandId = `${DIRECT_PREFIX}${tool.id}`;
    registeredDirect.add(commandId);
    registry.register({
      id: commandId,
      label: `Open ${tool.name}`,
      category: "External Tools",
      disabledReason: () => externalToolDisabledReason(tool),
      execute: () => runTool(tool.id),
    });
  }
}

function directCommandSignature(tools: ExternalTool[]): string {
  return JSON.stringify(
    tools.map((tool) => [
      tool.id,
      tool.name,
      tool.enabled,
      tool.requiresSession,
      tool.surface,
      tool.urlTemplate,
    ]),
  );
}

async function runTool(toolId: string): Promise<void> {
  try {
    await openExternalTool(toolId);
  } catch (err) {
    logError(`external tool ${toolId} launch failed`, err);
  }
}
