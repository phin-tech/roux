import type { Session } from "$lib/bindings";
import { createPane, updateInstance } from "./instances";
import type { PaneStatePayload } from "./persistence";
import { sessionLayouts, type LayoutNode } from "./layout";
import { initSessionWithProfile } from "./actions";
import { setLogicalFocus } from "./focus";
import { log } from "$lib/logging";
import { resolveProfileRef, type SpawnProfile, type SpawnProfileRef } from "./profiles";
import { runProfileInPane } from "./profileRunner";
import { spawnShell } from "$lib/tauri";
import { renderProjectPromptForSession } from "$lib/projectPromptTemplates";

export interface RestoreSessionPanesOptions {
  initTerminal: (paneId: string) => void;
  attachPtyListeners: (paneId: string) => Promise<void>;
  attachLivePtyToPane?: (paneId: string, ptyId: string) => Promise<void>;
  livePtyIds?: ReadonlySet<string> | null;
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
    await restorePrimaryOnly(session, undefined, opts);
    return;
  }

  const restored = stripKnownStaleCommandPanes(persisted, opts.livePtyIds);
  if (!restored.layout) {
    log(`restoreSessionPanes(${session.id}): persisted state has only stale command panes; falling back to primary-only restore`);
    await restorePrimaryOnly(session, undefined, opts);
    return;
  }

  const primaryDescriptor = restored.descriptors.find((d) => d.ptyId === session.id);
  if (!primaryDescriptor) {
    log(`restoreSessionPanes(${session.id}): persisted state has no primary pane; falling back to primary-only restore`);
    await restorePrimaryOnly(session, undefined, opts);
    return;
  }

  const restoredLayout = restored.layout;
  sessionLayouts.update((m) => {
    const next = new Map(m);
    next.set(session.id, restoredLayout);
    return next;
  });

  let primaryPaneId: string | null = null;
  // Tracks non-primary shell panes whose persisted PTY was gone and that
  // we respawned fresh during this restore. The second loop uses this to
  // attach the new PTY (instead of the stale id from the descriptor) and
  // to replay the spawn profile so agents come back live without user
  // intervention — closing the cold-start gap for headless callers (MCP,
  // CLI) that resolve panes by id and expect a live PTY.
  const respawnedPanes = new Map<string, { ptyId: string; profile: SpawnProfile | null }>();

  for (const d of restored.descriptors) {
    if (d.id === primaryDescriptor.id) {
      primaryPaneId = createPrimaryPane(session.id, d.spawnProfileRef, d);
      continue;
    }

    if (shouldRespawnStaleShell(d, opts.livePtyIds)) {
      const result = await respawnStaleShell(d, session);
      if (result.kind === "ok") {
        createPane({
          id: d.id,
          type: "shell",
          ptyId: result.ptyId,
          name: d.name,
          workingDir: d.workingDir,
          spawnProfileRef: d.spawnProfileRef,
          provider: d.provider,
          providerSessionId: d.providerSessionId,
        });
        respawnedPanes.set(d.id, { ptyId: result.ptyId, profile: result.profile });
      } else {
        log(`restoreSessionPanes(${session.id}): respawn failed for ${d.id} — ${result.message}`);
        createPane({
          id: d.id,
          type: "shell",
          ptyId: "",
          name: d.name,
          workingDir: d.workingDir,
          spawnProfileRef: d.spawnProfileRef,
          provider: d.provider,
          providerSessionId: d.providerSessionId,
        });
        updateInstance(d.id, { restoreError: result.message });
      }
      continue;
    }

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
      notesScope: d.notesScope,
      notesViewMode: d.notesViewMode,
    });
  }

  for (const d of restored.descriptors) {
    if (d.type === "markdown" || d.type === "notes") continue;

    const respawn = respawnedPanes.get(d.id);
    if (respawn) {
      try {
        opts.initTerminal(d.id);
        await opts.attachPtyListeners(d.id);
      } catch (e) {
        log(`restoreSessionPanes(${session.id}): failed to attach respawned pane ${d.id}: ${e}`);
        continue;
      }
      if (respawn.profile) {
        try {
          const appendSystemPrompt = await renderProjectPromptForSession(
            session,
            respawn.profile,
          );
          await runProfileInPane(respawn.ptyId, respawn.profile, {
            ...(appendSystemPrompt.trim() ? { appendSystemPrompt } : {}),
          });
        } catch (e) {
          log(`restoreSessionPanes(${session.id}): profile replay failed for ${d.id}: ${e}`);
        }
      }
      continue;
    }

    if (!d.ptyId) continue;
    if (!canAttachPty(d.ptyId, opts.livePtyIds)) continue;
    try {
      opts.initTerminal(d.id);
      if (opts.attachLivePtyToPane && opts.livePtyIds?.has(d.ptyId)) {
        await opts.attachLivePtyToPane(d.id, d.ptyId);
      } else {
        await opts.attachPtyListeners(d.id);
      }
    } catch (e) {
      log(`restoreSessionPanes(${session.id}): failed to attach pane ${d.id}: ${e}`);
    }
  }

  if (primaryPaneId) {
    setLogicalFocus(primaryPaneId);
  }
}

async function restorePrimaryOnly(
  session: Session,
  descriptor: PaneStatePayload["descriptors"][number] | undefined,
  opts: RestoreSessionPanesOptions,
): Promise<string> {
  const mainPaneId = initPrimaryPane(session.id, descriptor?.spawnProfileRef, descriptor);
  if (canAttachPty(session.id, opts.livePtyIds)) {
    opts.initTerminal(mainPaneId);
    if (opts.attachLivePtyToPane && opts.livePtyIds?.has(session.id)) {
      await opts.attachLivePtyToPane(mainPaneId, session.id);
    } else {
      await opts.attachPtyListeners(mainPaneId);
    }
  }
  return mainPaneId;
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

function canAttachPty(
  ptyId: string,
  livePtyIds: ReadonlySet<string> | null | undefined,
): boolean {
  return livePtyIds?.has(ptyId) === true;
}

function shouldRespawnStaleShell(
  descriptor: PaneStatePayload["descriptors"][number],
  livePtyIds: ReadonlySet<string> | null | undefined,
): boolean {
  if (descriptor.type !== "shell") return false;
  if (livePtyIds == null) return false;
  if (!descriptor.ptyId) return false;
  return !livePtyIds.has(descriptor.ptyId);
}

type RespawnResult =
  | { kind: "ok"; ptyId: string; profile: SpawnProfile | null }
  | { kind: "error"; message: string };

async function respawnStaleShell(
  descriptor: PaneStatePayload["descriptors"][number],
  session: Session,
): Promise<RespawnResult> {
  const freshPtyId = crypto.randomUUID();
  const profile = resolveProfileRef(descriptor.spawnProfileRef);
  const profileId =
    descriptor.spawnProfileRef?.kind === "registered"
      ? descriptor.spawnProfileRef.id
      : descriptor.spawnProfileRef?.kind === "inline"
        ? descriptor.spawnProfileRef.profile.id
        : null;
  try {
    await spawnShell(
      freshPtyId,
      descriptor.workingDir ?? session.worktreePath,
      session.id,
      descriptor.id,
      profileId,
    );
    return { kind: "ok", ptyId: freshPtyId, profile };
  } catch (e) {
    return { kind: "error", message: String(e) };
  }
}

function stripKnownStaleCommandPanes(
  persisted: PaneStatePayload,
  livePtyIds: ReadonlySet<string> | null | undefined,
): { layout: LayoutNode | null; descriptors: PaneStatePayload["descriptors"] } {
  if (livePtyIds == null) {
    return { layout: persisted.layout, descriptors: persisted.descriptors };
  }

  const staleCommandIds = new Set(
    persisted.descriptors
      .filter((d) => d.type === "command" && (!d.ptyId || !livePtyIds.has(d.ptyId)))
      .map((d) => d.id),
  );
  if (staleCommandIds.size === 0) {
    return { layout: persisted.layout, descriptors: persisted.descriptors };
  }

  return {
    layout: stripLeaves(persisted.layout, staleCommandIds),
    descriptors: persisted.descriptors.filter((d) => !staleCommandIds.has(d.id)),
  };
}

function stripLeaves(node: LayoutNode, paneIds: Set<string>): LayoutNode | null {
  if (node.kind === "leaf") {
    return paneIds.has(node.paneId) ? null : node;
  }

  const children = node.children
    .map((child) => stripLeaves(child, paneIds))
    .filter((child): child is LayoutNode => child !== null);

  if (children.length === 0) return null;
  if (children.length === 1) return children[0];
  return { ...node, children };
}

function createPrimaryPane(
  sessionId: string,
  spawnProfileRef: SpawnProfileRef | undefined,
  descriptor: PaneStatePayload["descriptors"][number],
): string {
  const paneId = descriptor.id || `${sessionId}-main`;
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
    notesScope: descriptor.notesScope,
    notesViewMode: descriptor.notesViewMode,
  });
  setLogicalFocus(paneId);
  return paneId;
}
