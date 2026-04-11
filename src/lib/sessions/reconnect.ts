import { get } from "svelte/store";
import type { Session } from "$lib/types";
import { updateSessionStatus } from "$lib/stores/sessions";
import { reconnectSessionPty, spawnShell } from "$lib/tauri";
import { replacePty, createPane, updateInstance, getInstance } from "$lib/panes/instances";
import { sessionLayouts, collectLeafIds, type LayoutNode } from "$lib/panes/layout";
import { loadPaneState, stripCommandPanes, type PaneDescriptor, type PaneStatePayload } from "$lib/panes/persistence";
import { log } from "$lib/logging";

const reconnecting = new Set<string>();

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Find the pane id of the session's claude pane by walking the current layout.
 * Returns null if the session has no layout or no claude pane.
 */
function findClaudePaneId(sessionId: string): string | null {
  const layout = get(sessionLayouts).get(sessionId);
  if (!layout) return null;
  for (const leafId of collectLeafIds(layout)) {
    if (getInstance(leafId)?.type === "claude") return leafId;
  }
  return null;
}

/** Find the claude descriptor id in a persisted payload. */
function findClaudeDescriptorId(descriptors: PaneDescriptor[]): string | null {
  const claudeDescs = descriptors.filter((d) => d.type === "claude");
  return claudeDescs.length === 1 ? claudeDescs[0].id : null;
}

/** True when the given layout is a single claude leaf. */
function isSingleClaudeLeaf(layout: LayoutNode, claudePaneId: string | null): boolean {
  return (
    claudePaneId !== null &&
    layout.kind === "leaf" &&
    layout.paneId === claudePaneId
  );
}

// ── Integrity preflight ───────────────────────────────────────────────────────

function validatePanePayload(sessionId: string, payload: PaneStatePayload): boolean {
  const { layout, descriptors } = payload;

  // All descriptor IDs must be unique.
  const ids = descriptors.map((d) => d.id);
  if (new Set(ids).size !== ids.length) {
    log(`pane restore preflight(${sessionId}): duplicate descriptor ids`);
    return false;
  }

  // Must have exactly one claude descriptor.
  const claudeDescs = descriptors.filter((d) => d.type === "claude");
  if (claudeDescs.length !== 1) {
    log(`pane restore preflight(${sessionId}): expected exactly one claude descriptor, got ${claudeDescs.length}`);
    return false;
  }

  // All descriptor types must be known.
  const knownTypes = new Set(["claude", "shell", "command", "markdown"]);
  for (const d of descriptors) {
    if (!knownTypes.has(d.type)) {
      log(`pane restore preflight(${sessionId}): unknown descriptor type "${d.type}"`);
      return false;
    }
  }

  // Every leaf in the tree must have exactly one matching descriptor.
  const leafIds = collectLeafIds(layout);
  const descById = new Map(descriptors.map((d) => [d.id, d]));
  for (const leafId of leafIds) {
    if (!descById.has(leafId)) {
      log(`pane restore preflight(${sessionId}): leaf "${leafId}" has no descriptor`);
      return false;
    }
  }

  return true;
}

// ── Pane rehydration ──────────────────────────────────────────────────────────

async function rehydratePane(
  paneId: string,
  descriptor: PaneDescriptor,
  sessionId: string,
  sessionWorktreePath: string,
): Promise<void> {
  if (descriptor.type === "claude") return; // main pane already exists

  if (descriptor.type === "markdown") {
    createPane({
      id: paneId,
      type: "markdown",
      ptyId: "",
      name: descriptor.name,
      docPath: descriptor.docPath,
    });
    return;
  }

  if (descriptor.type === "shell") {
    const ptyId = crypto.randomUUID();
    try {
      await spawnShell(
        ptyId,
        descriptor.workingDir ?? sessionWorktreePath,
        sessionId,
        paneId,
      );
      createPane({
        id: paneId,
        type: "shell",
        ptyId,
        name: descriptor.name,
        workingDir: descriptor.workingDir,
      });
    } catch (e) {
      const errMsg = String(e);
      log(`rehydratePane(${paneId}): shell spawn failed — ${errMsg}`);
      createPane({
        id: paneId,
        type: "shell",
        ptyId: "",
        name: descriptor.name,
        workingDir: descriptor.workingDir,
      });
      updateInstance(paneId, { restoreError: errMsg });
    }
    return;
  }

  // command panes are stripped before rehydration; this branch is unreachable
}

// ── Claude-pane-only reconnect (extracted from original reconnectSession) ────

async function reconnectClaudePaneOnly(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  const claudePaneId = findClaudePaneId(session.id);
  if (!claudePaneId) {
    throw new Error(`reconnectSession(${session.id}): no claude pane found to reconnect`);
  }
  replacePty(claudePaneId, session.id);
  const updated = await reconnectSessionPty(session.id, extraFlags);
  const { attachPtyListeners } = await import("$lib/panes/terminals");
  await attachPtyListeners(claudePaneId);
  updateSessionStatus(session.id, updated.status as Session["status"]);
  log(`Session ${session.id} reconnected (claude pane only)`);
  return updated;
}

// ── Public API ────────────────────────────────────────────────────────────────

export async function reconnectSession(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  if (reconnecting.has(session.id)) {
    throw new Error(`Reconnect already in progress for ${session.id}`);
  }
  reconnecting.add(session.id);
  try {
    log(`Reconnecting session ${session.id} (${session.name})`);

    const liveClaudePaneId = findClaudePaneId(session.id);

    // Guard: if the current layout is not a lone claude leaf, we're dealing
    // with a mid-session disconnect. Don't rehydrate from disk — trust the
    // live runtime state instead.
    const currentTree = get(sessionLayouts).get(session.id);
    const isClaudeOnly =
      !!currentTree && isSingleClaudeLeaf(currentTree, liveClaudePaneId);

    if (!isClaudeOnly) {
      return await reconnectClaudePaneOnly(session, extraFlags);
    }

    // Try to load persisted pane state from disk.
    const persisted = await loadPaneState(session.id);
    if (!persisted) {
      return await reconnectClaudePaneOnly(session, extraFlags);
    }

    // Fast-path: persisted tree is also a lone claude leaf.
    const persistedClaudeId = findClaudeDescriptorId(persisted.descriptors);
    if (isSingleClaudeLeaf(persisted.layout, persistedClaudeId)) {
      return await reconnectClaudePaneOnly(session, extraFlags);
    }

    // Integrity preflight: reject corrupt/mismatched data before touching state.
    if (!validatePanePayload(session.id, persisted)) {
      log(`pane restore preflight failed for ${session.id}, falling back to claude-pane-only reconnect`);
      return await reconnectClaudePaneOnly(session, extraFlags);
    }

    // Strip command panes — they cannot be restarted.
    const { tree: strippedTree, descriptors: strippedDescs } = stripCommandPanes(
      persisted.layout,
      persisted.descriptors,
    );

    if (!strippedTree) {
      return await reconnectClaudePaneOnly(session, extraFlags);
    }

    // Reconnect the Claude PTY. Abort layout restore if this fails.
    const updated = await reconnectClaudePaneOnly(session, extraFlags);

    // Rehydrate non-claude panes. All PaneInstances must exist BEFORE we
    // apply the layout tree, so the renderer can resolve every leaf.
    const leafIds = collectLeafIds(strippedTree);
    const descById = new Map(strippedDescs.map((d) => [d.id, d]));
    const claudeDescId = findClaudeDescriptorId(strippedDescs);
    const nonMainIds = leafIds.filter((id) => id !== claudeDescId);

    for (const paneId of nonMainIds) {
      const descriptor = descById.get(paneId);
      if (!descriptor) continue;
      await rehydratePane(paneId, descriptor, session.id, session.worktreePath);
    }

    // Apply the layout tree AFTER all PaneInstances are in the store.
    sessionLayouts.update((m) => {
      const next = new Map(m);
      next.set(session.id, strippedTree);
      return next;
    });

    // Wire terminals for panes that spawned successfully. Order matters:
    // initTerminal must precede attachPtyListeners so early PTY output
    // is not dropped when instance.terminal is still null.
    const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
    for (const paneId of nonMainIds) {
      const instance = getInstance(paneId);
      if (!instance || instance.restoreError || instance.type === "markdown") continue;
      initTerminal(paneId);
      await attachPtyListeners(paneId, (payload) => {
        log(`Restored shell ${paneId} exited (code=${payload.code})`);
        import("$lib/panes/actions").then(({ closePane }) =>
          closePane(session.id, paneId),
        );
      });
    }

    log(`Session ${session.id} reconnected with ${nonMainIds.length} additional pane(s)`);
    return updated;
  } finally {
    reconnecting.delete(session.id);
  }
}

export async function retryShellPane(paneId: string, sessionId: string): Promise<void> {
  const instance = getInstance(paneId);
  if (!instance || instance.type !== "shell" || !instance.restoreError) return;

  const ptyId = crypto.randomUUID();
  try {
    await spawnShell(ptyId, instance.workingDir ?? "", sessionId, paneId);
    updateInstance(paneId, { ptyId, restoreError: undefined });
    const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
    initTerminal(paneId);
    await attachPtyListeners(paneId);
    log(`retryShellPane(${paneId}): success`);
  } catch (e) {
    const errMsg = String(e);
    log(`retryShellPane(${paneId}): failed — ${errMsg}`);
    updateInstance(paneId, { restoreError: errMsg });
  }
}
