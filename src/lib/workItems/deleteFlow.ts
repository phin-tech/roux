import { get } from "svelte/store";
import type { WorkItem } from "$lib/bindings";
import { deleteWorkItem } from "$lib/stores/workItems";
import { sessionState } from "$lib/stores/sessions";
import { closeSession } from "$lib/sessions/close";
import { killSession } from "$lib/tauri";

export type WorkItemDeleteMode = "card-only" | "card-and-stop-session";

export async function deleteWorkItemWithMode(
  item: WorkItem,
  mode: WorkItemDeleteMode,
): Promise<void> {
  if (mode === "card-and-stop-session" && item.sessionId) {
    const session = get(sessionState).sessions.find((s) => s.id === item.sessionId);
    if (session) {
      await closeSession(session, {
        force: true,
        preserveWorkItemBoundSession: false,
      });
    } else {
      await killSession(item.sessionId);
    }
  }
  await deleteWorkItem(item.id);
}
