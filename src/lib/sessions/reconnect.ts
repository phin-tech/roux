import type { Session } from "$lib/types";
import { updateSessionStatus } from "$lib/stores/sessions";
import { reconnectSessionPty } from "$lib/tauri";
import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
import { log } from "$lib/logging";

export async function reconnectSession(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  log(`Reconnecting session ${session.id} (${session.name})${extraFlags ? ` with flags: ${extraFlags.join(" ")}` : ""}`);

  // Dispose the old xterm terminal so a fresh one is created on re-attach
  await disposeClaudeTerminal(session.id);

  // Call the Rust command that kills old PTY + spawns new one under same ID
  const updated = await reconnectSessionPty(session.id, extraFlags);

  // Update session status in the Svelte store
  updateSessionStatus(session.id, updated.status as Session["status"]);

  log(`Session ${session.id} reconnected`);
  return updated;
}
