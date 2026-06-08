import type { Session } from "$lib/bindings";
import { createPane, updateInstance } from "./instances";
import type { PaneStatePayload } from "./persistence";
import { sessionLayouts, type LayoutNode } from "./layout";
import { initSessionWithProfile } from "./actions";
import { setLogicalFocus } from "./focus";
import { log } from "$lib/logging";
import type { SpawnProfileRef } from "./profiles";
import { decidePaneRestore } from "./restoreDecision";

export interface RestoreSessionPanesOptions {
  initTerminal: (paneId: string) => void;
  attachPtyListeners: (paneId: string) => Promise<void>;
  attachLivePtyToPane?: (paneId: string, ptyId: string) => Promise<void>;
  livePtyIds?: ReadonlySet<string> | null;
  primaryPtyId?: string | null;
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
    log(
      `restoreSessionPanes(${session.id}): persisted state has only stale command panes; falling back to primary-only restore`,
    );
    await restorePrimaryOnly(session, undefined, opts);
    return;
  }

  const primaryPtyId = opts.primaryPtyId ?? session.id;
  const primaryDescriptor =
    restored.descriptors.find((d) => d.ptyId === primaryPtyId) ??
    restored.descriptors.find((d) => d.ptyId === session.id);
  if (!primaryDescriptor) {
    log(
      `restoreSessionPanes(${session.id}): persisted state has no primary pane; falling back to primary-only restore`,
    );
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
    const descriptor =
      d.id === primaryDescriptor.id && primaryPtyId !== session.id
        ? { ...d, ptyId: primaryPtyId }
        : d;
    const decision = decidePaneRestore({
      descriptor,
      sessionId: session.id,
      livePtyIds: opts.livePtyIds,
    });
    if (decision.kind === "strip") continue;

    if (d.id === primaryDescriptor.id) {
      primaryPaneId = createPrimaryPane(session.id, d.spawnProfileRef, d);
      updateInstance(primaryPaneId, {
        ptyId: decision.panePtyId,
        terminalState: decision.terminalState,
      });
      continue;
    }

    createPane({
      id: d.id,
      type: d.type,
      ptyId: decision.panePtyId,
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
    updateInstance(d.id, { terminalState: decision.terminalState });
  }

  for (const d of restored.descriptors) {
    if (d.type === "markdown" || d.type === "notes") continue;
    const descriptor =
      d.id === primaryDescriptor.id && primaryPtyId !== session.id
        ? { ...d, ptyId: primaryPtyId }
        : d;
    const decision = decidePaneRestore({
      descriptor,
      sessionId: session.id,
      livePtyIds: opts.livePtyIds,
    });
    if (decision.kind !== "attach") continue;
    try {
      opts.initTerminal(d.id);
      if (opts.attachLivePtyToPane && opts.livePtyIds?.has(decision.ptyId)) {
        await opts.attachLivePtyToPane(d.id, decision.ptyId);
      } else {
        await opts.attachPtyListeners(d.id);
      }
    } catch (e) {
      log(
        `restoreSessionPanes(${session.id}): failed to attach pane ${d.id}: ${e}`,
      );
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
  const primaryPtyId = opts.primaryPtyId ?? session.id;
  const mainPaneId = initPrimaryPane(
    session.id,
    descriptor?.spawnProfileRef,
    descriptor,
  );
  if (canAttachPty(primaryPtyId, opts.livePtyIds)) {
    updateInstance(mainPaneId, {
      ptyId: primaryPtyId,
      terminalState: { kind: "attached", ptyId: primaryPtyId },
    });
    opts.initTerminal(mainPaneId);
    if (opts.attachLivePtyToPane && opts.livePtyIds?.has(primaryPtyId)) {
      await opts.attachLivePtyToPane(mainPaneId, primaryPtyId);
    } else {
      await opts.attachPtyListeners(mainPaneId);
    }
  } else {
    updateInstance(mainPaneId, { terminalState: { kind: "empty" } });
  }
  return mainPaneId;
}

function initPrimaryPane(
  sessionId: string,
  spawnProfileRef: SpawnProfileRef | undefined,
  descriptor?: PaneStatePayload["descriptors"][number],
): string {
  const profileRef: SpawnProfileRef = spawnProfileRef ?? {
    kind: "registered",
    id: "claude",
  };
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

function stripKnownStaleCommandPanes(
  persisted: PaneStatePayload,
  livePtyIds: ReadonlySet<string> | null | undefined,
): { layout: LayoutNode | null; descriptors: PaneStatePayload["descriptors"] } {
  const staleCommandIds = new Set(
    persisted.descriptors
      .filter(
        (d) =>
          decidePaneRestore({
            descriptor: d,
            sessionId: "",
            livePtyIds,
          }).kind === "strip",
      )
      .map((d) => d.id),
  );
  if (staleCommandIds.size === 0) {
    return { layout: persisted.layout, descriptors: persisted.descriptors };
  }

  return {
    layout: stripLeaves(persisted.layout, staleCommandIds),
    descriptors: persisted.descriptors.filter(
      (d) => !staleCommandIds.has(d.id),
    ),
  };
}

function stripLeaves(
  node: LayoutNode,
  paneIds: Set<string>,
): LayoutNode | null {
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
