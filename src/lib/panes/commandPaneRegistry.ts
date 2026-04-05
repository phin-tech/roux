export interface CommandPaneHandle {
  paneId: string;
  command: string;
  getStatus: () => "running" | "succeeded" | "failed";
  triggerRerun: () => void;
}

const registry = new Map<string, CommandPaneHandle>();

export function registerCommandPane(handle: CommandPaneHandle) {
  registry.set(handle.paneId, handle);
}

export function unregisterCommandPane(paneId: string) {
  registry.delete(paneId);
}

export function listCommandPanes(): CommandPaneHandle[] {
  return [...registry.values()];
}
