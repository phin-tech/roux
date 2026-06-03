/**
 * PTY attach/detach logic.
 *
 * Owns the atomic sequence for wiring a PTY to a pane: clear the terminal,
 * update pane state, call backend attach (which returns a replay buffer),
 * write the replay, subscribe to live output, and mark the PTY as read.
 */

import {
  getInstance,
  updateInstance,
  replacePty,
  findPaneByPtyId,
} from "./instances";
import type { PaneInstance } from "./instances";
import { getTerminalController } from "./terminalRuntime";
import { connectPaneTerminal } from "./terminals";
import { attachPtyToPane as tauriAttachPty, markPtyRead } from "$lib/tauri";
import { log } from "$lib/logging";
import type { SpawnProfileRef } from "./profiles";

export interface AttachOptions {
  /** Profile ID from the PTY (e.g. "claude", "codex"). Updates pane's spawnProfileRef. */
  profile?: string | null;
  /** Display name from the PTY. Applied to the pane so the titlebar reflects the attached PTY. */
  name?: string | null;
}

/**
 * Attach a PTY to a pane. The atomic sequence:
 * 1. Verify pane exists
 * 2. Clear source pane state if PTY is moving from another pane
 * 3. Unsubscribe old PTY listeners and clear output channel
 * 4. Clear terminal display
 * 5. Update pane binding to the new PTY
 * 6. Call backend attach (gets replay bytes + resizes PTY)
 * 7. Write replay bytes to terminal
 * 8. Subscribe to live output via connectPaneTerminal
 * 9. Mark PTY as read
 */
export async function attachPtyToPane(
  paneId: string,
  ptyId: string,
  options: AttachOptions = {},
): Promise<void> {
  const pane = getInstance(paneId);
  if (!pane) {
    log(`attachPtyToPane: pane ${paneId} not found`);
    return;
  }

  // Clear the source pane's state if this PTY is currently attached elsewhere.
  // Without this, the old pane keeps stale terminalState pointing to a PTY
  // that's now attached to a different pane.
  const sourcePaneId = findSourcePane(ptyId, paneId);
  if (sourcePaneId) {
    updateInstance(sourcePaneId, {
      terminalState: { kind: "empty" },
    });
    log(`attachPtyToPane: cleared source pane ${sourcePaneId}`);
  }

  // Tear down existing PTY subscriptions and clear the output channel before
  // re-attaching. Without this, stale `onSessionExit` listeners from the old
  // PTY would fire against the new pane state, and the output channel would
  // route bytes from the wrong PTY. Mirrors the cleanup in `replacePty`.
  replacePty(paneId, ptyId);

  const terminal = getTerminalController(paneId);

  // Clear terminal display so stale output from the previous session is not
  // visible when the replay arrives.
  terminal?.reset();

  // Build the update payload. Always set terminalState; conditionally update
  // spawnProfileRef if the PTY carries profile info.
  const updates: Partial<PaneInstance> = {
    terminalState: { kind: "attached", ptyId },
  };

  if (options.profile) {
    const profileRef: SpawnProfileRef = {
      kind: "registered",
      id: options.profile,
    };
    updates.spawnProfileRef = profileRef;
  }

  if (options.name !== undefined) {
    updates.name = options.name ?? undefined;
  }

  // Update pane state to reflect the new attached PTY. `replacePty` already
  // updated `ptyId`; set `terminalState` so reactive reads see a consistent
  // attached state before the async backend call completes.
  updateInstance(paneId, updates);

  // Get the pane's current dimensions; fall back to 80x24 if the terminal
  // is not yet attached to a visible DOM container.
  const dims = terminal?.fit() ?? { cols: 80, rows: 24 };

  // Call backend — this records the new pane attachment, resizes the PTY,
  // and returns a replay buffer of recent output.
  try {
    const result = await tauriAttachPty(ptyId, paneId, dims.cols, dims.rows);

    if (result.replay_bytes && result.replay_bytes.length > 0) {
      const bytes = new Uint8Array(result.replay_bytes);
      terminal?.write(bytes);
    }
  } catch (e) {
    log(`attachPtyToPane: backend attach failed: ${e}`);
  }

  // Re-subscribe to live PTY output. connectPaneTerminal is idempotent for
  // the terminal controller (ensureTerminalController no-ops if it exists)
  // but will set up a fresh output channel for the new PTY.
  await connectPaneTerminal(paneId);

  // Mark as read so notification badges clear.
  await markPtyRead(ptyId).catch(() => {});

  log(
    `attachPtyToPane: attached ${ptyId} to pane ${paneId}${options.profile ? ` (profile=${options.profile})` : ""}`,
  );
}

/**
 * Find the pane that currently has a PTY attached, excluding the target pane.
 * Returns null if no other pane has this PTY.
 */
function findSourcePane(ptyId: string, excludePaneId: string): string | null {
  const pane = findPaneByPtyId(ptyId);
  if (pane && pane.id !== excludePaneId) {
    return pane.id;
  }
  return null;
}
