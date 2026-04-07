import type { Session } from "$lib/types";
import { updateSessionStatus } from "$lib/stores/sessions";
import { reconnectSessionPty } from "$lib/tauri";
import { replacePty } from "$lib/panes/instances";
import { log } from "$lib/logging";

const reconnecting = new Set<string>();

export async function reconnectSession(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  if (reconnecting.has(session.id)) {
    throw new Error(`Reconnect already in progress for ${session.id}`);
  }
  reconnecting.add(session.id);
  try {
    log(`Reconnecting session ${session.id} (${session.name})${extraFlags ? ` with flags: ${extraFlags.join(" ")}` : ""}`);

    const mainPaneId = `${session.id}-main`;

    // Swap the PTY on the main pane — tears down old listeners, keeps terminal
    replacePty(mainPaneId, session.id);

    // Call the Rust command that kills old PTY + spawns new one under same ID
    const updated = await reconnectSessionPty(session.id, extraFlags);

    // Re-attach PTY listeners to the main pane
    const { attachPtyListeners } = await import("$lib/panes/terminals");
    await attachPtyListeners(mainPaneId);

    // Update session status in the Svelte store
    updateSessionStatus(session.id, updated.status as Session["status"]);

    log(`Session ${session.id} reconnected`);
    return updated;
  } finally {
    reconnecting.delete(session.id);
  }
}
