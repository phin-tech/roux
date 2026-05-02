import type { Session } from "$lib/bindings";
import { createPane, updateInstance } from "./instances";
import type { PaneStatePayload } from "./persistence";
import { sessionLayouts, type LayoutNode } from "./layout";
import { initSessionWithProfile } from "./actions";
import { setLogicalFocus } from "./focus";
import { log } from "$lib/logging";
import type { SpawnProfileRef } from "./profiles";

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

  for (const d of restored.descriptors) {
    if (d.id === primaryDescriptor.id) {
      primaryPaneId = createPrimaryPane(session.id, d.spawnProfileRef, d);
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
      markMissingRuntimeIfNeeded(d, opts.livePtyIds);
    }
  }

  for (const d of restored.descriptors) {
    if (d.type === "markdown" || d.type === "notes") continue;
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

function canAttachPty(
  ptyId: string,
  livePtyIds: ReadonlySet<string> | null | undefined,
): boolean {
  return livePtyIds?.has(ptyId) === true;
}

function markMissingRuntimeIfNeeded(
  descriptor: PaneStatePayload["descriptors"][number],
  livePtyIds: ReadonlySet<string> | null | undefined,
): void {
  if (livePtyIds == null) return;
  if (descriptor.type !== "shell") return;
  if (!descriptor.ptyId || canAttachPty(descriptor.ptyId, livePtyIds)) return;
  updateInstance(descriptor.id, {
    restoreError: "PTY is no longer running. Reconnect this pane to start a new shell.",
  });
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
    nonoProfile: descriptor.nonoProfile,
    nonoAllowDirs: descriptor.nonoAllowDirs,
    notesScope: descriptor.notesScope,
    notesViewMode: descriptor.notesViewMode,
  });
  setLogicalFocus(paneId);
  return paneId;
}
