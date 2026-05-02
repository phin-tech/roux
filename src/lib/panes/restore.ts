import type { Session } from "$lib/bindings";
import { createPane, updateInstance } from "./instances";
import type { PaneStatePayload } from "./persistence";
import { sessionLayouts } from "./layout";
import { initSessionWithProfile } from "./actions";
import { log } from "$lib/logging";
import type { SpawnProfileRef } from "./profiles";

export interface RestoreSessionPanesOptions {
  initTerminal: (paneId: string) => void;
  attachPtyListeners: (paneId: string) => Promise<void>;
}

/**
 * Rebuild pane runtime state after a webview reload.
 *
 * The Rust side keeps PTYs alive across Ctrl+Shift+R, but all Svelte stores
 * and terminal controllers are gone. Recreate persisted pane instances and
 * reattach output channels without replaying profile startup commands.
 */
export async function restoreSessionPanes(
  session: Session,
  persisted: PaneStatePayload | null,
  opts: RestoreSessionPanesOptions,
): Promise<void> {
  if (!persisted) {
    const mainPaneId = initPrimaryPane(session.id, undefined);
    opts.initTerminal(mainPaneId);
    await opts.attachPtyListeners(mainPaneId);
    return;
  }

  sessionLayouts.update((m) => {
    const next = new Map(m);
    next.set(session.id, persisted.layout);
    return next;
  });

  for (const d of persisted.descriptors) {
    if (d.id === `${session.id}-main` && d.ptyId === session.id) {
      createPrimaryPane(session.id, d.spawnProfileRef, d);
    } else {
      createPane({
        id: d.id,
        type: d.type,
        ptyId: d.ptyId,
        name: d.name,
        workingDir: d.workingDir,
        command: d.command,
        docPath: d.docPath,
        spawnProfileRef: d.spawnProfileRef,
        provider: d.provider,
        providerSessionId: d.providerSessionId,
        nonoProfile: d.nonoProfile,
        nonoAllowDirs: d.nonoAllowDirs,
        notesScope: d.notesScope,
        notesViewMode: d.notesViewMode,
      });
    }
  }

  for (const d of persisted.descriptors) {
    if (d.type === "markdown" || d.type === "notes") continue;
    if (!d.ptyId) continue;
    try {
      opts.initTerminal(d.id);
      await opts.attachPtyListeners(d.id);
    } catch (e) {
      log(`restoreSessionPanes(${session.id}): failed to attach pane ${d.id}: ${e}`);
    }
  }
}

function initPrimaryPane(
  sessionId: string,
  spawnProfileRef: SpawnProfileRef | undefined,
  descriptor?: PaneStatePayload["descriptors"][number],
): string {
  const profileRef: SpawnProfileRef =
    spawnProfileRef ?? { kind: "registered", id: "claude" };
  const paneId = initSessionWithProfile(sessionId, profileRef, {
    provider: descriptor?.provider,
    providerSessionId: descriptor?.providerSessionId,
    nonoProfile: descriptor?.nonoProfile,
    nonoAllowDirs: descriptor?.nonoAllowDirs,
  });
  if (descriptor) {
    updateInstance(paneId, {
      name: descriptor.name,
      workingDir: descriptor.workingDir,
      command: descriptor.command,
      docPath: descriptor.docPath,
      provider: descriptor.provider,
      providerSessionId: descriptor.providerSessionId,
      notesScope: descriptor.notesScope,
      notesViewMode: descriptor.notesViewMode,
    });
  }
  return paneId;
}

function createPrimaryPane(
  sessionId: string,
  spawnProfileRef: SpawnProfileRef | undefined,
  descriptor: PaneStatePayload["descriptors"][number],
): string {
  const paneId = `${sessionId}-main`;
  createPane({
    id: paneId,
    type: descriptor.type,
    ptyId: descriptor.ptyId,
    name: descriptor.name,
    workingDir: descriptor.workingDir,
    command: descriptor.command,
    docPath: descriptor.docPath,
    spawnProfileRef: spawnProfileRef ?? { kind: "registered", id: "claude" },
    provider: descriptor.provider,
    providerSessionId: descriptor.providerSessionId,
    nonoProfile: descriptor.nonoProfile,
    nonoAllowDirs: descriptor.nonoAllowDirs,
    notesScope: descriptor.notesScope,
    notesViewMode: descriptor.notesViewMode,
  });
  return paneId;
}
