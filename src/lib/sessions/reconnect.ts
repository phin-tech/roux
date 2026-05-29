import { get } from "svelte/store";
import type { Session } from "$lib/types";
import { updateSessionStatus } from "$lib/stores/sessions";
import { reconnectSessionShellPty, spawnShell } from "$lib/tauri";
import { replacePty, createPane, updateInstance, getInstance, type PaneInstance } from "$lib/panes/instances";
import { sessionLayouts, collectLeafIds, type LayoutNode } from "$lib/panes/layout";
import { loadPaneState, stripCommandPanes, type PaneDescriptor, type PaneStatePayload } from "$lib/panes/persistence";
import { resolveProfileRef, type SpawnProfile, type SpawnProfileRef } from "$lib/panes/profiles";
import { runProfileInPane } from "$lib/panes/profileRunner";
import { renderProjectPromptForSession } from "$lib/projectPromptTemplates";
import { log } from "$lib/logging";
import { setLogicalFocus } from "$lib/panes/focus";

const reconnecting = new Set<string>();

interface ContinuePlan {
  flags?: string[];
}

type ReconnectIntent = "reconnect" | "continue";

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

function findSessionPrimaryInstance(sessionId: string): PaneInstance | null {
  const primaryPaneId = findSessionPrimaryPaneId(sessionId);
  if (!primaryPaneId) return null;
  return getInstance(primaryPaneId) ?? null;
}

function ensurePrimaryPaneForReconnect(
  sessionId: string,
  descriptor?: PaneDescriptor | null,
): string {
  const existing = findSessionPrimaryPaneId(sessionId);
  if (existing) return existing;

  const paneId = descriptor?.id ?? `${sessionId}-main`;
  const spawnProfileRef: SpawnProfileRef =
    descriptor?.spawnProfileRef ?? { kind: "registered", id: "claude" };

  if (getInstance(paneId)) {
    updateInstance(paneId, {
      type: "shell",
      ptyId: sessionId,
      name: descriptor?.name,
      workingDir: descriptor?.workingDir,
      command: descriptor?.command,
      docPath: descriptor?.docPath,
      spawnProfileRef,
      provider: descriptor?.provider,
      providerSessionId: descriptor?.providerSessionId,
      nonoProfile: descriptor?.nonoProfile,
      nonoAllowDirs: descriptor?.nonoAllowDirs,
      notesScope: descriptor?.notesScope,
      notesViewMode: descriptor?.notesViewMode,
      restoreError: undefined,
      terminalState: undefined,
    });
  } else {
    createPane({
      id: paneId,
      type: "shell",
      ptyId: sessionId,
      name: descriptor?.name,
      workingDir: descriptor?.workingDir,
      command: descriptor?.command,
      docPath: descriptor?.docPath,
      spawnProfileRef,
      provider: descriptor?.provider,
      providerSessionId: descriptor?.providerSessionId,
      nonoProfile: descriptor?.nonoProfile,
      nonoAllowDirs: descriptor?.nonoAllowDirs,
      notesScope: descriptor?.notesScope,
      notesViewMode: descriptor?.notesViewMode,
    });
  }

  sessionLayouts.update((m) => {
    if (m.has(sessionId)) return m;
    const next = new Map(m);
    next.set(sessionId, { kind: "leaf", paneId });
    return next;
  });
  setLogicalFocus(paneId);
  return paneId;
}

// Tokens that are safe to type into ANY shell verbatim — POSIX (bash/zsh),
// PowerShell, and cmd.exe alike. We deliberately don't try to shell-quote
// here: POSIX `'`-quoting doesn't escape correctly in PowerShell or cmd,
// and Roux supports all three. Real Claude/Codex session ids are UUID-
// shaped and pass cleanly; if a value falls outside this set we drop the
// exact-resume path and let the caller fall back to `--continue` /
// `resume --last`.
//
// Notably excluded: `%` triggers env-var expansion in cmd.exe (`%FOO%`),
// which would substitute environment values into the typed command.
const SAFE_SHELL_ARG = /^[A-Za-z0-9_@+=:,./-]+$/;

function providerSessionArg(value: string | undefined): string | null {
  const trimmed = value?.trim() ?? "";
  if (!trimmed) return null;
  return SAFE_SHELL_ARG.test(trimmed) ? trimmed : null;
}

function fallbackContinueFlags(provider: SpawnProfile["provider"] | null, profileId?: string): string[] | undefined {
  if (provider === "claude" || profileId === "claude") return ["--continue"];
  if (provider === "codex" || profileId === "codex") return ["resume", "--last"];
  return undefined;
}

function defaultContinuePlan(
  instance: PaneInstance | null,
  profile: SpawnProfile | null,
): ContinuePlan {
  const provider = instance?.provider ?? profile?.provider ?? null;
  const fallbackFlags = fallbackContinueFlags(provider, profile?.id);
  const providerSessionId = providerSessionArg(instance?.providerSessionId);
  if (providerSessionId) {
    if (provider === "claude" || profile?.id === "claude") {
      return { flags: ["--resume", providerSessionId] };
    }
    if (provider === "codex" || profile?.id === "codex") {
      return { flags: ["resume", providerSessionId] };
    }
  }
  return { flags: fallbackFlags };
}

function flagsForIntent(
  intent: ReconnectIntent,
  instance: PaneInstance | null,
  profile: SpawnProfile | null,
  explicitFlags?: string[],
): string[] | undefined {
  if (explicitFlags !== undefined) return explicitFlags;
  if (intent !== "continue") return undefined;
  return defaultContinuePlan(instance, profile).flags;
}

function profileWithFlags(
  profile: SpawnProfile,
  flags: string[] | undefined,
): SpawnProfile {
  if (!flags?.length) return profile;
  const baseCmd = profile.startupCommand ?? "";
  const combined = `${baseCmd} ${flags.join(" ")}`.trim();
  return {
    ...profile,
    startupCommand: combined.length ? combined : undefined,
  };
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
  const knownTypes = new Set(["shell", "command", "markdown", "notes"]);
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

  if (descriptor.type === "notes") {
    createPane({
      id: paneId,
      type: "notes",
      ptyId: "",
      name: descriptor.name ?? "Notes",
      notesScope: descriptor.notesScope ?? "session",
      notesViewMode: descriptor.notesViewMode ?? "edit",
    });
    return;
  }

  if (descriptor.type === "shell") {
    const ptyId = crypto.randomUUID();
    const profileId = descriptor.spawnProfileRef?.kind === "registered"
      ? descriptor.spawnProfileRef.id
      : descriptor.spawnProfileRef?.kind === "inline"
        ? descriptor.spawnProfileRef.profile.id
        : null;
    try {
      await spawnShell(
        ptyId,
        descriptor.workingDir ?? sessionWorktreePath,
        sessionId,
        paneId,
        descriptor.nonoProfile ?? null,
        descriptor.nonoAllowDirs ?? null,
        profileId,
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
        provider: descriptor.provider,
        providerSessionId: descriptor.providerSessionId,
        nonoProfile: descriptor.nonoProfile,
        nonoAllowDirs: descriptor.nonoAllowDirs,
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
        provider: descriptor.provider,
        providerSessionId: descriptor.providerSessionId,
        nonoProfile: descriptor.nonoProfile,
        nonoAllowDirs: descriptor.nonoAllowDirs,
      });
      updateInstance(paneId, { restoreError: errMsg });
    }
    return;
  }

  // command panes are stripped before rehydration; this branch is unreachable
}

// ── Primary-pane-only reconnect (shell path) ────────────────────────────────

/**
 * Reconnect the session-owned primary PTY by respawning a plain shell on
 * the backend, re-attaching pane listeners, and replaying the pane's
 * resolved spawn profile. Extra flags (Continue / Resume / New from the
 * Claude-builtin SessionPicker) are appended to the profile's startup
 * command before replay, so the flags are typed into the shell rather
 * than passed to a direct binary launch.
 */
async function reconnectPrimaryPaneOnly(
  session: Session,
  extraFlags?: string[],
  intent: ReconnectIntent = "reconnect",
): Promise<Session> {
  const primaryPaneId = findSessionPrimaryPaneId(session.id);
  if (!primaryPaneId) {
    throw new Error(
      `reconnectSession(${session.id}): no primary pane found to reconnect`,
    );
  }

  // Read nono config from the live primary pane instance so the respawn
  // lands in the same sandbox the pane started in.
  const instance = getInstance(primaryPaneId);
  const nonoProfile = instance?.nonoProfile ?? null;
  const nonoAllowDirs = instance?.nonoAllowDirs ?? null;
  const profile = resolveProfileRef(instance?.spawnProfileRef);

  replacePty(primaryPaneId, session.id);
  const updated = await reconnectSessionShellPty(
    session.id,
    nonoProfile,
    nonoAllowDirs,
    profile?.id ?? null,
  );
  const { connectPaneTerminal } = await import("$lib/panes/terminals");
  await connectPaneTerminal(primaryPaneId);
  updateSessionStatus(session.id, updated.status as Session["status"]);

  // Replay the primary pane's profile, appending any extra flags to the
  // startup command so Claude's Continue/Resume/New flows still work.
  // A replay failure is logged, not surfaced — the shell itself is alive.
  //
  // No second-attempt fallback here on purpose: `runProfileInPane` writes
  // the command and the trailing newline as separate PTY writes, so a
  // partial failure (command typed, newline failed) would leave a half-
  // typed line in the shell and a retry would compound the mess. If write
  // fails, we leave the shell in whatever state it landed in and log.
  if (profile) {
    const flags = flagsForIntent(intent, instance ?? null, profile, extraFlags);
    const effectiveProfile = profileWithFlags(profile, flags);
    try {
      const appendSystemPrompt = await renderProjectPromptForSession(
        session,
        effectiveProfile,
      );
      await runProfileInPane(session.id, effectiveProfile, {
        ...(appendSystemPrompt.trim() ? { appendSystemPrompt } : {}),
      });
    } catch (e) {
      log(
        `reconnectPrimaryPaneOnly(${session.id}): profile "${profile.id}" replay failed — ${e}`,
      );
    }
  }

  log(`Session ${session.id} reconnected (primary pane only, shell path)`);
  return updated;
}

async function replayRestoredPaneProfile(
  session: Session,
  paneId: string,
  intent: ReconnectIntent,
): Promise<void> {
  const instance = getInstance(paneId);
  if (!instance || instance.type !== "shell" || instance.restoreError) return;
  const profile = resolveProfileRef(instance.spawnProfileRef);
  if (!profile) return;

  const flags = flagsForIntent(intent, instance, profile);
  const effectiveProfile = profileWithFlags(profile, flags);
  try {
    const appendSystemPrompt = await renderProjectPromptForSession(
      session,
      effectiveProfile,
    );
    await runProfileInPane(instance.ptyId, effectiveProfile, {
      ...(appendSystemPrompt.trim() ? { appendSystemPrompt } : {}),
    });
  } catch (e) {
    log(`replayRestoredPaneProfile(${paneId}): profile "${profile.id}" replay failed — ${e}`);
  }
}

// ── Public API ────────────────────────────────────────────────────────────────

async function reconnectSessionInternal(
  session: Session,
  extraFlags?: string[],
  intent: ReconnectIntent = "reconnect",
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

    if (currentTree && !isPrimaryOnly) {
      return await reconnectPrimaryPaneOnly(session, extraFlags, intent);
    }

    // Try to load persisted pane state from disk.
    const persisted = await loadPaneState(session.id);
    if (!persisted) {
      ensurePrimaryPaneForReconnect(session.id);
      return await reconnectPrimaryPaneOnly(session, extraFlags, intent);
    }

    // Fast-path: persisted tree is also a lone primary leaf.
    const persistedPrimaryId = findPrimaryDescriptorId(
      session.id,
      persisted.descriptors,
    );
    if (isSinglePrimaryLeaf(persisted.layout, persistedPrimaryId)) {
      const primaryDesc = persisted.descriptors.find((d) => d.id === persistedPrimaryId);
      ensurePrimaryPaneForReconnect(session.id, primaryDesc);
      return await reconnectPrimaryPaneOnly(session, extraFlags, intent);
    }

    // Integrity preflight: reject corrupt/mismatched data before touching state.
    if (!validatePanePayload(session.id, persisted)) {
      log(
        `pane restore preflight failed for ${session.id}, falling back to primary-pane-only reconnect`,
      );
      const primaryDesc = persisted.descriptors.find((d) => d.ptyId === session.id);
      ensurePrimaryPaneForReconnect(session.id, primaryDesc);
      return await reconnectPrimaryPaneOnly(session, extraFlags, intent);
    }

    // Strip command panes — they cannot be restarted.
    const { tree: strippedTree, descriptors: strippedDescs } = stripCommandPanes(
      persisted.layout,
      persisted.descriptors,
    );

    if (!strippedTree) {
      ensurePrimaryPaneForReconnect(session.id);
      return await reconnectPrimaryPaneOnly(session, extraFlags, intent);
    }

    // Reconnect the session-owned PTY. Abort layout restore if this fails.
    const primaryDesc = strippedDescs.find((d) => d.ptyId === session.id);
    ensurePrimaryPaneForReconnect(session.id, primaryDesc);
    const updated = await reconnectPrimaryPaneOnly(session, extraFlags, intent);

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

    // Wire terminals for panes that spawned successfully. The adapter
    // boundary owns init-before-attach ordering so early PTY output is not
    // dropped before the controller exists.
    const { connectPaneTerminal } = await import("$lib/panes/terminals");
    const { updateInstance } = await import("$lib/panes/instances");
    for (const paneId of nonMainIds) {
      const instance = getInstance(paneId);
      if (!instance || instance.restoreError || instance.type === "markdown" || instance.type === "notes") continue;
      const ptyId = instance.ptyId;
      await connectPaneTerminal(paneId, (payload) => {
        log(`Restored shell ${paneId} exited (code=${payload.code})`);
        updateInstance(paneId, {
          terminalState: {
            kind: "dead",
            ptyId,
            exitCode: payload.code ?? null,
          },
        });
      });
      await replayRestoredPaneProfile(session, paneId, intent);
    }

    log(`Session ${session.id} reconnected with ${nonMainIds.length} additional pane(s)`);
    return updated;
  } finally {
    reconnecting.delete(session.id);
  }
}

export async function reconnectSession(
  session: Session,
  extraFlags?: string[],
): Promise<Session> {
  return reconnectSessionInternal(session, extraFlags, "reconnect");
}

export async function continueSession(session: Session): Promise<Session> {
  return reconnectSessionInternal(session, undefined, "continue");
}

/**
 * Reconnect a session whose primary pane was created via
 * `createSessionShell`. Kills the old PTY, spawns a fresh plain shell on
 * the backend, re-attaches pane listeners, and replays the pane's profile
 * commands so agents like Codex (or Claude in a shell) come back up the
 * way they were first launched.
 *
 * Distinct from `reconnectSession`: the latter does full layout
 * rehydration from persisted pane state, while this one is the
 * lightweight primary-pane-only path used by explicit per-pane
 * re-run/reconnect flows.
 */
export async function reconnectSessionShell(
  session: Session,
  extraStartupFlags?: string[],
): Promise<Session> {
  if (reconnecting.has(session.id)) {
    throw new Error(`Reconnect already in progress for ${session.id}`);
  }
  reconnecting.add(session.id);
  try {
    log(`Reconnecting shell session ${session.id} (${session.name})`);
    return await reconnectPrimaryPaneOnly(session, extraStartupFlags);
  } finally {
    reconnecting.delete(session.id);
  }
}

export async function continueSessionShell(session: Session): Promise<Session> {
  const primary = findSessionPrimaryInstance(session.id);
  const plan = defaultContinuePlan(primary, resolveProfileRef(primary?.spawnProfileRef));
  return reconnectSessionShell(session, plan.flags);
}

export async function retryShellPane(paneId: string, sessionId: string): Promise<void> {
  const instance = getInstance(paneId);
  if (!instance || instance.type !== "shell" || !instance.restoreError) return;

  const profileId = instance.spawnProfileRef?.kind === "registered"
    ? instance.spawnProfileRef.id
    : instance.spawnProfileRef?.kind === "inline"
      ? instance.spawnProfileRef.profile.id
      : null;
  const ptyId = crypto.randomUUID();
  try {
    await spawnShell(
      ptyId,
      instance.workingDir ?? "",
      sessionId,
      paneId,
      instance.nonoProfile ?? null,
      instance.nonoAllowDirs ?? null,
      profileId,
    );
    updateInstance(paneId, { ptyId, restoreError: undefined });
    const { connectPaneTerminal } = await import("$lib/panes/terminals");
    await connectPaneTerminal(paneId);
    log(`retryShellPane(${paneId}): success`);
  } catch (e) {
    const errMsg = String(e);
    log(`retryShellPane(${paneId}): failed — ${errMsg}`);
    updateInstance(paneId, { restoreError: errMsg });
  }
}
