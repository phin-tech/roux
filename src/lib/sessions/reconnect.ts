import { get } from "svelte/store";
import type { Session } from "$lib/types";
import { updateSessionStatus } from "$lib/stores/sessions";
import { reconnectSessionPty, reconnectSessionShellPty, spawnShell } from "$lib/tauri";
import { replacePty, createPane, updateInstance, getInstance } from "$lib/panes/instances";
import { sessionLayouts, collectLeafIds, type LayoutNode } from "$lib/panes/layout";
import { loadPaneState, stripCommandPanes, type PaneDescriptor, type PaneStatePayload } from "$lib/panes/persistence";
import { resolveProfileRef } from "$lib/panes/profiles";
import { runProfileInPane } from "$lib/panes/profileRunner";
import { log } from "$lib/logging";

const reconnecting = new Set<string>();

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Find the pane id of the session's primary pane — the one that hosts the
 * session-owned PTY. Identified by `ptyId === sessionId`, which is how the
 * Rust side keys the initial Claude PTY: `pty_manager.spawn(session_id, …)`
 * stores it under `session_id`. Returns null if no such pane exists (zero-
 * pane session or mid-close state).
 */
function findSessionPrimaryPaneId(sessionId: string): string | null {
  const layout = get(sessionLayouts).get(sessionId);
  if (!layout) return null;
  for (const leafId of collectLeafIds(layout)) {
    if (getInstance(leafId)?.ptyId === sessionId) return leafId;
  }
  return null;
}

/**
 * Find the id of the primary-pane descriptor in a persisted payload — i.e.
 * the one whose persisted ptyId matches the session id. Returns null if
 * there isn't exactly one such descriptor (zero or multiple is corrupt).
 */
function findPrimaryDescriptorId(
  sessionId: string,
  descriptors: PaneDescriptor[],
): string | null {
  const primary = descriptors.filter((d) => d.ptyId === sessionId);
  return primary.length === 1 ? primary[0].id : null;
}

/** True when the given layout is a single leaf that matches the primary pane. */
function isSinglePrimaryLeaf(
  layout: LayoutNode,
  primaryPaneId: string | null,
): boolean {
  return (
    primaryPaneId !== null &&
    layout.kind === "leaf" &&
    layout.paneId === primaryPaneId
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

  // Exactly one pane must host the session-owned PTY. Multiple primaries
  // means the persisted state was written by an older schema or was
  // concurrently mutated; zero means nothing to reconnect.
  const primaryDescs = descriptors.filter((d) => d.ptyId === sessionId);
  if (primaryDescs.length !== 1) {
    log(
      `pane restore preflight(${sessionId}): expected exactly one primary-pane descriptor (ptyId == sessionId), got ${primaryDescs.length}`,
    );
    return false;
  }

  // All descriptor types must be known.
  const knownTypes = new Set(["shell", "command", "markdown"]);
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
  // The primary pane (the one that hosts the session-owned PTY) is already
  // created by initSession on startup — reconnectPrimaryPaneOnly attaches
  // its PTY. Skip it here so we don't double-create the instance.
  if (descriptor.ptyId === sessionId) return;

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
        // Preserve the profile the pane was launched from so the
        // re-run button and provider-specific UI light up after
        // restart. Dropping this silently reverted every restored
        // pane to "plain shell" in the UI.
        spawnProfileRef: descriptor.spawnProfileRef,
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
        spawnProfileRef: descriptor.spawnProfileRef,
      });
      updateInstance(paneId, { restoreError: errMsg });
    }
    return;
  }

  // command panes are stripped before rehydration; this branch is unreachable
}

// ── Primary-pane-only reconnect (extracted from original reconnectSession) ──

async function reconnectPrimaryPaneOnly(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  const primaryPaneId = findSessionPrimaryPaneId(session.id);
  if (!primaryPaneId) {
    throw new Error(
      `reconnectSession(${session.id}): no primary pane found to reconnect`,
    );
  }
  replacePty(primaryPaneId, session.id);
  const updated = await reconnectSessionPty(session.id, extraFlags);
  const { attachPtyListeners } = await import("$lib/panes/terminals");
  await attachPtyListeners(primaryPaneId);
  updateSessionStatus(session.id, updated.status as Session["status"]);
  log(`Session ${session.id} reconnected (primary pane only)`);
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

    const livePrimaryPaneId = findSessionPrimaryPaneId(session.id);

    // Guard: if the current layout is not a lone primary leaf, we're
    // dealing with a mid-session disconnect. Don't rehydrate from disk —
    // trust the live runtime state instead.
    const currentTree = get(sessionLayouts).get(session.id);
    const isPrimaryOnly =
      !!currentTree && isSinglePrimaryLeaf(currentTree, livePrimaryPaneId);

    if (!isPrimaryOnly) {
      return await reconnectPrimaryPaneOnly(session, extraFlags);
    }

    // Try to load persisted pane state from disk.
    const persisted = await loadPaneState(session.id);
    if (!persisted) {
      return await reconnectPrimaryPaneOnly(session, extraFlags);
    }

    // Fast-path: persisted tree is also a lone primary leaf.
    const persistedPrimaryId = findPrimaryDescriptorId(
      session.id,
      persisted.descriptors,
    );
    if (isSinglePrimaryLeaf(persisted.layout, persistedPrimaryId)) {
      return await reconnectPrimaryPaneOnly(session, extraFlags);
    }

    // Integrity preflight: reject corrupt/mismatched data before touching state.
    if (!validatePanePayload(session.id, persisted)) {
      log(
        `pane restore preflight failed for ${session.id}, falling back to primary-pane-only reconnect`,
      );
      return await reconnectPrimaryPaneOnly(session, extraFlags);
    }

    // Strip command panes — they cannot be restarted.
    const { tree: strippedTree, descriptors: strippedDescs } = stripCommandPanes(
      persisted.layout,
      persisted.descriptors,
    );

    if (!strippedTree) {
      return await reconnectPrimaryPaneOnly(session, extraFlags);
    }

    // Reconnect the session-owned PTY. Abort layout restore if this fails.
    const updated = await reconnectPrimaryPaneOnly(session, extraFlags);

    // Rehydrate non-primary panes. All PaneInstances must exist BEFORE we
    // apply the layout tree, so the renderer can resolve every leaf.
    const leafIds = collectLeafIds(strippedTree);
    const descById = new Map(strippedDescs.map((d) => [d.id, d]));
    const primaryDescId = findPrimaryDescriptorId(session.id, strippedDescs);
    const nonMainIds = leafIds.filter((id) => id !== primaryDescId);

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

/**
 * Reconnect a session whose primary pane was created via
 * `createSessionShell` (i.e. every non-Claude-builtin profile). Kills
 * the old PTY, spawns a fresh plain shell on the backend, re-attaches
 * pane listeners, and replays the pane's profile commands so agents
 * like Codex come back up the way they were first launched.
 *
 * Separate from `reconnectSession` so that the legacy Claude spawn
 * path (which runs the claude binary directly, via `pty_manager.spawn`
 * with flags + nono wrapping) stays undisturbed. Callers dispatch based
 * on the primary pane's spawnProfileRef — Claude-builtin uses
 * `reconnectSession`, everything else uses this one.
 */
export async function reconnectSessionShell(session: Session): Promise<Session> {
  if (reconnecting.has(session.id)) {
    throw new Error(`Reconnect already in progress for ${session.id}`);
  }
  reconnecting.add(session.id);
  try {
    log(`Reconnecting shell session ${session.id} (${session.name})`);

    const primaryPaneId = findSessionPrimaryPaneId(session.id);
    if (!primaryPaneId) {
      throw new Error(
        `reconnectSessionShell(${session.id}): no primary pane found to reconnect`,
      );
    }

    // Point the frontend pane at the same session id — the Rust side
    // keys the shell PTY under session.id, identical to create_session
    // and create_session_shell. `replacePty` clears any stale listeners
    // before we attach the fresh ones.
    replacePty(primaryPaneId, session.id);

    const updated = await reconnectSessionShellPty(session.id);

    const { attachPtyListeners } = await import("$lib/panes/terminals");
    await attachPtyListeners(primaryPaneId);
    updateSessionStatus(session.id, updated.status as Session["status"]);

    // Re-run the pane's profile commands so the agent (Codex, a user
    // profile, etc.) comes back up automatically. Plain-shell panes
    // have no commands so this is a no-op for them. A profile-replay
    // failure during reconnect is logged but not fatal: the shell is
    // alive, and firing a startup-time notification before the window
    // has any UI context is worse than quiet.
    const instance = getInstance(primaryPaneId);
    const profile = resolveProfileRef(instance?.spawnProfileRef);
    if (profile) {
      try {
        await runProfileInPane(session.id, profile);
      } catch (e) {
        log(
          `reconnectSessionShell(${session.id}): profile "${profile.id}" replay failed — ${e}`,
        );
      }
    }

    log(`Session ${session.id} reconnected (shell path)`);
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
