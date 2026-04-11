import { get } from "svelte/store";
import type { Session } from "$lib/types";
import { updateSessionStatus } from "$lib/stores/sessions";
import { reconnectSessionPty, spawnShell } from "$lib/tauri";
import { replacePty, createPane, updateInstance, getInstance } from "$lib/panes/instances";
import { sessionLayouts, collectLeafIds } from "$lib/panes/layout";
import { loadPaneState, stripCommandPanes, type PaneDescriptor, type PaneStatePayload } from "$lib/panes/persistence";
import { log } from "$lib/logging";

const reconnecting = new Set<string>();

// ── Integrity preflight ───────────────────────────────────────────────────────

function validatePanePayload(sessionId: string, payload: PaneStatePayload): boolean {
  const { layout, descriptors } = payload;

  // All descriptor IDs must be unique.
  const ids = descriptors.map((d) => d.id);
  if (new Set(ids).size !== ids.length) {
    log(`pane restore preflight(${sessionId}): duplicate descriptor ids`);
    return false;
  }

  // Must have exactly one claude descriptor matching the main pane.
  const mainId = `${sessionId}-main`;
  const claudeDescs = descriptors.filter((d) => d.type === "claude");
  if (claudeDescs.length !== 1 || claudeDescs[0].id !== mainId) {
    log(`pane restore preflight(${sessionId}): expected exactly one claude descriptor with id ${mainId}`);
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
      await spawnShell(ptyId, descriptor.workingDir ?? sessionWorktreePath);
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

// ── Main-pane-only reconnect (extracted from original reconnectSession) ───────

async function reconnectMainPaneOnly(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  const mainPaneId = `${session.id}-main`;
  replacePty(mainPaneId, session.id);
  const updated = await reconnectSessionPty(session.id, extraFlags);
  const { attachPtyListeners } = await import("$lib/panes/terminals");
  await attachPtyListeners(mainPaneId);
  updateSessionStatus(session.id, updated.status as Session["status"]);
  log(`Session ${session.id} reconnected (main pane only)`);
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

    // Guard: if the current layout is not the default main-only leaf, we're
    // dealing with a mid-session disconnect. Don't rehydrate from disk — trust
    // the live runtime state instead.
    const currentTree = get(sessionLayouts).get(session.id);
    const isMainOnly =
      currentTree?.kind === "leaf" &&
      currentTree.paneId === `${session.id}-main`;

    if (!isMainOnly) {
      return await reconnectMainPaneOnly(session, extraFlags);
    }

    // Try to load persisted pane state from disk.
    const persisted = await loadPaneState(session.id);
    if (!persisted) {
      return await reconnectMainPaneOnly(session, extraFlags);
    }

    // Fast-path: persisted tree is also main-only.
    if (
      persisted.layout.kind === "leaf" &&
      persisted.layout.paneId === `${session.id}-main`
    ) {
      return await reconnectMainPaneOnly(session, extraFlags);
    }

    // Integrity preflight: reject corrupt/mismatched data before touching state.
    if (!validatePanePayload(session.id, persisted)) {
      log(`pane restore preflight failed for ${session.id}, falling back to main-pane-only reconnect`);
      return await reconnectMainPaneOnly(session, extraFlags);
    }

    // Strip command panes — they cannot be restarted.
    const { tree: strippedTree, descriptors: strippedDescs } = stripCommandPanes(
      persisted.layout,
      persisted.descriptors,
    );

    if (!strippedTree) {
      return await reconnectMainPaneOnly(session, extraFlags);
    }

    // Reconnect the Claude PTY. Abort layout restore if this fails.
    const updated = await reconnectMainPaneOnly(session, extraFlags);

    // Rehydrate non-main panes. All PaneInstances must exist BEFORE we apply
    // the layout tree, so the renderer can resolve every leaf.
    const leafIds = collectLeafIds(strippedTree);
    const descById = new Map(strippedDescs.map((d) => [d.id, d]));
    const nonMainIds = leafIds.filter((id) => id !== `${session.id}-main`);

    for (const paneId of nonMainIds) {
      const descriptor = descById.get(paneId);
      if (!descriptor) continue;
      await rehydratePane(paneId, descriptor, session.worktreePath);
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

export async function retryShellPane(paneId: string): Promise<void> {
  const instance = getInstance(paneId);
  if (!instance || instance.type !== "shell" || !instance.restoreError) return;

  const ptyId = crypto.randomUUID();
  try {
    await spawnShell(ptyId, instance.workingDir ?? "");
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
