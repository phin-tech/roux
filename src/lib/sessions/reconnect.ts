import type { Session } from "$lib/types";
import { addSession, removeSession } from "$lib/stores/sessions";
import { initSessionPanes, removeSessionPanes } from "$lib/stores/panes";
import { createSession, killSession } from "$lib/tauri";
import { closeAuxiliaryPanes } from "$lib/panes/actions";
import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
import { log } from "$lib/logging";

export async function reconnectSession(session: Session): Promise<Session> {
  log(`Reconnecting session ${session.id} (${session.name})`);
  await closeAuxiliaryPanes(session.id);
  await disposeClaudeTerminal(session.id);
  await killSession(session.id);

  removeSessionPanes(session.id);
  removeSession(session.id);

  const newSession = await createSession(
    session.repoRoot,
    session.name,
    session.worktreePath !== session.repoRoot ? session.worktreePath : null,
    null
  );

  log(`Reconnected session: ${newSession.id}`);
  addSession(newSession);
  initSessionPanes(newSession.id);
  return newSession;
}
