import { get } from "svelte/store";
import { registry } from "./registry";
import type { CommandItem } from "./registry";
import { queries } from "$lib/queries";
import { navigatePane, movePaneInDirection, resizePane, toggleStack } from "$lib/panes/layout";
import { toggleFullscreen, setLogicalFocus, focusedPaneId } from "$lib/panes/focus";
import { paneSlotById } from "$lib/stores/ui";
import { paneInstances, updateInstance, getAttachedPtyId } from "$lib/panes/instances";
import { splitPane, closeFocusedPane } from "$lib/panes/actions";
import {
  profileList,
  type SpawnProfile,
  type SpawnProfileRef,
} from "$lib/panes/profiles";
import { runProfileInPane } from "$lib/panes/profileRunner";
import { spawnShell, spawnTask, listDocs, notificationsPush, listSessionPtys, killPty, setPtyName } from "$lib/tauri";
import { attachPtyToPane } from "$lib/panes/attach";
import { openCustomProfileEditor } from "$lib/stores/customProfileModal";
import { log, logError } from "$lib/logging";

/**
 * Spawn a new shell pane seeded by a specific profile. Shared by both
 * "Split right with profile" and "Split down with profile" palette
 * commands. Plain shell profiles get the same path with no setup/startup
 * commands, so the helper handles every registry entry uniformly.
 */
async function spawnShellPaneWithProfile(
  direction: "h" | "v",
  profile: SpawnProfile,
): Promise<void> {
  const session = queries.activeSession();
  const activeId = queries.activeSessionId();
  if (!session || !activeId) return;

  const ptyId = crypto.randomUUID();
  const paneId = crypto.randomUUID();
  const nonoProfile = profile.nonoProfile ?? undefined;
  const nonoAllowDirs = profile.nonoAllowDirs?.length
    ? profile.nonoAllowDirs
    : undefined;
  log(
    `Split ${direction} with profile "${profile.id}": pane=${paneId} pty=${ptyId} cwd=${session.worktreePath}`,
  );
  try {
    await spawnShell(
      ptyId,
      session.worktreePath,
      session.id,
      paneId,
      nonoProfile,
      nonoAllowDirs,
      profile.id,
    );
  } catch (e) {
    logError(`Failed to spawn shell for profile "${profile.id}"`, e);
    return;
  }

  const spawnProfileRef: SpawnProfileRef =
    profile.source === "inline"
      ? { kind: "inline", profile }
      : { kind: "registered", id: profile.id };

  const newPaneId = splitPane(activeId, direction, {
    id: paneId,
    type: "shell",
    ptyId,
    spawnProfileRef,
    nonoProfile,
    nonoAllowDirs,
  });
  if (!newPaneId) return;

  const { connectPaneTerminal } = await import("$lib/panes/terminals");
  await connectPaneTerminal(newPaneId, (payload) => {
    log(`Shell pane ${newPaneId} exited (code=${payload.code})`);
    updateInstance(newPaneId, {
      terminalState: { kind: "dead", ptyId, exitCode: payload.code ?? null },
    });
  });

  // The attach is synchronous enough that the pending-output channel
  // catches any bytes emitted before we start typing. If the profile
  // itself throws (bad setup command, dead PTY, etc.) the pane is
  // already in the tree and the shell is alive, so we surface a
  // notification instead of tearing the pane down.
  try {
    await runProfileInPane(ptyId, profile);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    logError(`runProfileInPane failed for split-with-profile "${profile.id}"`, e);
    void notificationsPush({
      level: "warning",
      source: { type: "internal" },
      title: `Profile setup failed: ${profile.name}`,
      subtitle: null,
      body: msg,
      sessionId: session.id,
      actions: [
        {
          id: "dismiss",
          label: "Dismiss",
          kind: { type: "dismiss" },
          primary: true,
        },
      ],
      dedupKey: null,
    }).catch((pushErr) =>
      logError("split-with-profile: notificationsPush failed", pushErr),
    );
  }
}

/**
 * Build a palette sub-picker item for each registered profile. Used by
 * both the horizontal and vertical "Split with profile" commands. The
 * `onPick` callback is the concrete action that runs after the user
 * chooses a profile in the drill-in.
 *
 * Also appends a "Custom…" entry at the end that defers to the global
 * `ProfileCustomEditor` modal via `openCustomProfileEditor()` — matches
 * the parity of the new-session dialog's profile picker.
 */
function profileSubItems(
  onPick: (profile: SpawnProfile) => void | Promise<void>,
): CommandItem[] {
  const items: CommandItem[] = [];
  for (const profile of get(profileList)) {
    const suffix =
      profile.source === "user"
        ? " (user)"
        : profile.provider
          ? ` · ${profile.provider}`
          : "";
    items.push({
      id: `profile:${profile.id}`,
      label: `${profile.name}${suffix}`,
      description: profile.startupCommand ?? undefined,
      action: () => onPick(profile),
    });
  }
  items.push({
    id: "profile:__custom__",
    label: "Custom…",
    description: "Define an ad-hoc inline profile for this pane",
    action: async () => {
      const profile = await openCustomProfileEditor();
      if (profile) await onPick(profile);
    },
  });
  return items;
}

/**
 * Look up a registered built-in profile by id for the keyboard-shortcut
 * split commands. Returns null if the registry isn't populated yet
 * (e.g. `loadBuiltinProfiles` hasn't finished on startup) or the provider
 * was removed.
 */
function findBuiltinProfile(id: string): SpawnProfile | null {
  return get(profileList).find((p) => p.id === id) ?? null;
}

function formatTimeAgo(timestampMs: number): string {
  const diff = Date.now() - timestampMs;
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function registerPaneCommands() {
  registry.register({
    id: "pane.split-horizontal",
    label: "Split Horizontal",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: async () => {
      const session = queries.activeSession();
      if (!session) return;
      const ptyId = crypto.randomUUID();
      const paneId = crypto.randomUUID();
      log(`Split horizontal: pane=${paneId} pty=${ptyId} cwd=${session.worktreePath}`);
      try {
        await spawnShell(ptyId, session.worktreePath, session.id, paneId, null, null, "shell");
      } catch (e) {
        logError("Failed to spawn shell for horizontal split", e);
        return;
      }
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      const newPaneId = splitPane(activeId, "h", { id: paneId, type: "shell", ptyId });
      if (newPaneId) {
        const { connectPaneTerminal } = await import("$lib/panes/terminals");
        await connectPaneTerminal(newPaneId, (payload) => {
          log(`Shell pane ${newPaneId} exited (code=${payload.code})`);
          updateInstance(newPaneId, {
            terminalState: { kind: "dead", ptyId, exitCode: payload.code ?? null },
          });
        });
      }
    },
  });

  registry.register({
    id: "pane.split-vertical",
    label: "Split Vertical",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: async () => {
      const session = queries.activeSession();
      if (!session) return;
      const ptyId = crypto.randomUUID();
      const paneId = crypto.randomUUID();
      log(`Split vertical: pane=${paneId} pty=${ptyId} cwd=${session.worktreePath}`);
      try {
        await spawnShell(ptyId, session.worktreePath, session.id, paneId, null, null, "shell");
      } catch (e) {
        logError("Failed to spawn shell for vertical split", e);
        return;
      }
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      const newPaneId = splitPane(activeId, "v", { id: paneId, type: "shell", ptyId });
      if (newPaneId) {
        const { connectPaneTerminal } = await import("$lib/panes/terminals");
        await connectPaneTerminal(newPaneId, (payload) => {
          log(`Shell pane ${newPaneId} exited (code=${payload.code})`);
          updateInstance(newPaneId, {
            terminalState: { kind: "dead", ptyId, exitCode: payload.code ?? null },
          });
        });
      }
    },
  });

  registry.register({
    id: "pane.split-horizontal-with-profile",
    label: "Split Right with Profile…",
    category: "Panes",
    available: () => queries.canSplitPane(),
    getItems: () => profileSubItems((p) => spawnShellPaneWithProfile("h", p)),
  });

  registry.register({
    id: "pane.split-vertical-with-profile",
    label: "Split Down with Profile…",
    category: "Panes",
    available: () => queries.canSplitPane(),
    getItems: () => profileSubItems((p) => spawnShellPaneWithProfile("v", p)),
  });

  // Fast-path shortcuts for the two first-class agents. Register a palette
  // entry each so keyboard users can drop a Claude or Codex shell next to
  // the focused pane without drilling into the profile picker. Shortcuts
  // no-op until `loadBuiltinProfiles` finishes populating the registry on
  // startup — acceptable for something a user can only hit after the
  // window is interactive.
  registry.register({
    id: "pane.split-claude",
    label: "Split Right → Claude",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: async () => {
      const profile = findBuiltinProfile("claude");
      if (!profile) {
        log("pane.split-claude: claude built-in profile not in registry yet");
        return;
      }
      await spawnShellPaneWithProfile("h", profile);
    },
  });

  registry.register({
    id: "pane.split-codex",
    label: "Split Right → Codex",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: async () => {
      const profile = findBuiltinProfile("codex");
      if (!profile) {
        log("pane.split-codex: codex built-in profile not in registry yet");
        return;
      }
      await spawnShellPaneWithProfile("h", profile);
    },
  });

  registry.register({
    id: "pane.focus-left",
    label: "Focus Pane Left",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "left");
    },
  });

  registry.register({
    id: "pane.focus-down",
    label: "Focus Pane Down",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "down");
    },
  });

  registry.register({
    id: "pane.focus-up",
    label: "Focus Pane Up",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "up");
    },
  });

  registry.register({
    id: "pane.focus-right",
    label: "Focus Pane Right",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "right");
    },
  });

  registry.register({
    id: "pane.move-left",
    label: "Move Pane Left",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "left");
    },
  });

  registry.register({
    id: "pane.move-down",
    label: "Move Pane Down",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "down");
    },
  });

  registry.register({
    id: "pane.move-up",
    label: "Move Pane Up",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "up");
    },
  });

  registry.register({
    id: "pane.move-right",
    label: "Move Pane Right",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "right");
    },
  });

  registry.register({
    id: "pane.toggle-fullscreen",
    label: "Toggle Fullscreen",
    category: "Panes",
    available: () => !!queries.focusedPaneId(),
    execute: () => toggleFullscreen(),
  });

  registry.register({
    id: "pane.resize-left",
    label: "Resize Pane Left",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "left", 0.05);
    },
  });

  registry.register({
    id: "pane.resize-down",
    label: "Resize Pane Down",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "down", 0.05);
    },
  });

  registry.register({
    id: "pane.resize-up",
    label: "Resize Pane Up",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "up", 0.05);
    },
  });

  registry.register({
    id: "pane.resize-right",
    label: "Resize Pane Right",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "right", 0.05);
    },
  });

  registry.register({
    id: "pane.close",
    label: "Close Pane",
    category: "Panes",
    available: () => queries.canClosePane(),
    execute: async () => {
      const activeId = queries.activeSessionId();
      if (activeId) {
        await closeFocusedPane(activeId);
      }
    },
  });

  registry.register({
    id: "pane.toggle-stack",
    label: "Toggle Stack",
    category: "Panes",
    available: () => queries.canTogglePaneStack(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) toggleStack(activeId);
    },
  });

  registry.register({
    id: "pane.rename",
    label: "Rename Pane",
    category: "Panes",
    available: () => !!queries.focusedPaneId(),
    inputPlaceholder: "Enter pane name...",
    getItems: () => [],
    onInput: async (name: string) => {
      const paneId = queries.focusedPaneId();
      if (!paneId) return;
      const trimmed = name.trim() || undefined;
      updateInstance(paneId, { name: trimmed });
      // Also update the PTY's name so it persists after detach
      const inst = get(paneInstances).get(paneId);
      const ptyId = inst ? getAttachedPtyId(inst) : null;
      if (ptyId) {
        await setPtyName(ptyId, trimmed ?? null).catch(() => {});
      }
    },
  });

  // -- Multi-step: Open Document as Pane --
  registry.register({
    id: "pane.open-doc",
    label: "Open Document",
    category: "Documents",
    getItems: async () => {
      const session = queries.activeSession();
      if (!session) return [];
      const docs = await listDocs(session.worktreePath);
      return docs.map((doc) => ({
        id: doc.path,
        label: doc.name,
        description: doc.relativePath,
        action: () => {
          const activeId = queries.activeSessionId();
          if (activeId) {
            splitPane(activeId, "h", {
              type: "markdown",
              ptyId: "",
              docPath: doc.path,
            });
          }
        },
      }));
    },
  });

  // -- Spawn command pane --
  registry.register({
    id: "pane.run-command",
    label: "Run Command",
    category: "Panes",
    available: () => queries.canSplitPane(),
    inputPlaceholder: "Enter command to run...",
    getItems: () => [],
    onInput: async (command: string) => {
      const session = queries.activeSession();
      const activeId = queries.activeSessionId();
      if (!session || !activeId) return;
      const paneId = `cmd-${crypto.randomUUID()}`;
      const ptyId = `${paneId}-${Date.now()}`;
      await spawnTask(ptyId, command, session.worktreePath, session.id, paneId, "command");
      const newPaneId = splitPane(activeId, "h", {
        id: paneId,
        type: "command",
        ptyId,
        command,
        workingDir: session.worktreePath,
      });
      if (newPaneId) {
        const { connectPaneTerminal } = await import("$lib/panes/terminals");
        updateInstance(newPaneId, {
          commandStatus: "running",
          commandStartedAt: Date.now(),
          elapsedTimer: setInterval(() => {}, 1000), // PaneShell handles display
        });
        await connectPaneTerminal(newPaneId, (payload) => {
          const status = payload.code === 0 ? "success" : "error";
          updateInstance(newPaneId, {
            commandStatus: status as "success" | "error",
            commandExitCode: payload.code,
          });
        });
      }
    },
  });

  for (let slot = 1; slot <= 10; slot++) {
    registry.register({
      id: `pane.focus-index-${slot}`,
      label: `Focus Pane ${slot}`,
      category: "Panes",
      available: () => {
        const slots = get(paneSlotById);
        for (const s of slots.values()) if (s === slot) return true;
        return false;
      },
      execute: () => {
        const slots = get(paneSlotById);
        for (const [paneId, s] of slots) {
          if (s === slot) {
            setLogicalFocus(paneId);
            return;
          }
        }
      },
    });
  }

  registry.register({
    id: "pane.focus-next",
    label: "Focus Next Pane",
    category: "Panes",
    available: () => get(paneSlotById).size > 1,
    execute: () => {
      const slots = get(paneSlotById);
      if (slots.size === 0) return;
      const focused = get(focusedPaneId);
      const currentSlot = focused ? slots.get(focused) ?? null : null;
      const ordered = [...slots.entries()].sort((a, b) => a[1] - b[1]);
      const nextIndex =
        currentSlot === null
          ? 0
          : (ordered.findIndex(([, s]) => s === currentSlot) + 1) % ordered.length;
      const next = ordered[nextIndex];
      if (next) setLogicalFocus(next[0]);
    },
  });

  // ── Notes pane commands ────────────────────────────────────────────────────

  registry.register({
    id: "pane.open-notes-horizontal",
    label: "Open Notes Pane (Horizontal)",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      splitPane(activeId, "h", {
        type: "notes",
        ptyId: "",
        name: "Notes",
        notesScope: "session",
        notesViewMode: "edit",
      });
    },
  });

  registry.register({
    id: "pane.open-notes-vertical",
    label: "Open Notes Pane (Vertical)",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      splitPane(activeId, "v", {
        type: "notes",
        ptyId: "",
        name: "Notes",
        notesScope: "session",
        notesViewMode: "edit",
      });
    },
  });

  registry.register({
    id: "pane.notes-show-session",
    label: "Notes: Session Scope",
    category: "Panes",
    available: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return false;
      return get(paneInstances).get(paneId)?.type === "notes";
    },
    execute: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return;
      const inst = get(paneInstances).get(paneId);
      if (inst?.type !== "notes") return;
      updateInstance(paneId, { notesScope: "session" });
    },
  });

  registry.register({
    id: "pane.notes-show-repo",
    label: "Notes: Repo Scope",
    category: "Panes",
    available: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return false;
      const inst = get(paneInstances).get(paneId);
      if (inst?.type !== "notes") return false;
      const session = queries.activeSession();
      return !!session?.repoRoot;
    },
    execute: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return;
      const inst = get(paneInstances).get(paneId);
      if (inst?.type !== "notes") return;
      updateInstance(paneId, { notesScope: "repo" });
    },
  });

  registry.register({
    id: "pane.notes-show-project",
    label: "Notes: Project Scope",
    category: "Panes",
    available: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return false;
      const inst = get(paneInstances).get(paneId);
      if (inst?.type !== "notes") return false;
      const session = queries.activeSession();
      return !!session?.projectId;
    },
    execute: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return;
      const inst = get(paneInstances).get(paneId);
      if (inst?.type !== "notes") return;
      updateInstance(paneId, { notesScope: "project" });
    },
  });

  registry.register({
    id: "pane.notes-show-global",
    label: "Notes: Global Scope",
    category: "Panes",
    available: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return false;
      return get(paneInstances).get(paneId)?.type === "notes";
    },
    execute: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return;
      const inst = get(paneInstances).get(paneId);
      if (inst?.type !== "notes") return;
      updateInstance(paneId, { notesScope: "global" });
    },
  });

  registry.register({
    id: "pane.notes-toggle-view-mode",
    label: "Notes: Toggle Edit/Read",
    category: "Panes",
    available: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return false;
      return get(paneInstances).get(paneId)?.type === "notes";
    },
    execute: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return;
      const inst = get(paneInstances).get(paneId);
      if (inst?.type !== "notes") return;
      const current = inst.notesViewMode ?? "edit";
      updateInstance(paneId, { notesViewMode: current === "edit" ? "read" : "edit" });
    },
  });

  // ── Attach Terminal command ────────────────────────────────────────────────

  registry.register({
    id: "pane.attach-terminal",
    label: "Attach Terminal\u2026",
    category: "Panes",
    available: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return false;
      const inst = get(paneInstances).get(paneId);
      return inst?.type === "shell" || inst?.type === "command";
    },
    getItems: async () => {
      const sessionId = queries.activeSessionId();
      if (!sessionId) return [];

      const currentPaneId = get(focusedPaneId);
      const currentInst = currentPaneId
        ? get(paneInstances).get(currentPaneId)
        : undefined;
      const currentPtyId = currentInst ? getAttachedPtyId(currentInst) : null;

      const ptys = await listSessionPtys(sessionId);

      const items: CommandItem[] = [];

      const attached = ptys.filter(
        (p) => p.status.type === "RunningAttached" && p.id !== currentPtyId,
      );
      const detached = ptys.filter((p) => p.status.type === "RunningDetached");
      const instances = get(paneInstances);

      for (const pty of attached) {
        const attachedStatus = pty.status as Extract<typeof pty.status, { type: "RunningAttached" }>;
        const paneInst = instances.get(attachedStatus.pane_id);
        const paneName = paneInst?.name;
        const label = paneName || pty.name || pty.profile || "Shell";
        const description = paneName
          ? pty.working_dir || "attached"
          : `attached · ${pty.working_dir || ""}`;
        const icon = pty.profile === "claude" ? "bot" : "terminal";
        items.push({
          id: `attach:${pty.id}`,
          label,
          icon,
          description: description.trim(),
          action: async () => {
            if (!currentPaneId) return;
            await attachPtyToPane(currentPaneId, pty.id, { profile: pty.profile });
          },
        });
      }

      for (const pty of detached) {
        const detachedStatus = pty.status as Extract<typeof pty.status, { type: "RunningDetached" }>;
        const ago = formatTimeAgo(detachedStatus.since_ms);
        const label = pty.name || pty.profile || "Shell";
        const icon = pty.profile === "claude" ? "bot" : "terminal";
        items.push({
          id: `attach:${pty.id}`,
          label,
          icon,
          description: `detached ${ago} · ${pty.working_dir || ""}`.trim(),
          action: async () => {
            if (!currentPaneId) return;
            await attachPtyToPane(currentPaneId, pty.id, { profile: pty.profile });
          },
        });
      }

      return items;
    },
  });

  // ── Kill Terminal command ──────────────────────────────────────────────────

  registry.register({
    id: "pane.kill-terminal",
    label: "Kill Terminal",
    category: "Panes",
    available: () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return false;
      const inst = get(paneInstances).get(paneId);
      if (!inst || (inst.type !== "shell" && inst.type !== "command")) return false;
      return !!getAttachedPtyId(inst);
    },
    execute: async () => {
      const paneId = get(focusedPaneId);
      if (!paneId) return;
      const inst = get(paneInstances).get(paneId);
      if (!inst) return;

      const ptyId = getAttachedPtyId(inst);
      if (!ptyId) return;

      await killPty(ptyId);

      updateInstance(paneId, {
        terminalState: { kind: "dead", ptyId, exitCode: null },
      });
    },
  });
}
