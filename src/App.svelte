<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import ProfileCustomEditor from "$lib/components/ProfileCustomEditor.svelte";
  import DoctorPanel from "$lib/components/DoctorPanel.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import NotesPanel from "$lib/components/NotesPanel.svelte";
  import WatchesPane from "$lib/components/WatchesPane.svelte";
  import NotificationsPane from "$lib/components/NotificationsPane.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import KeymapHud from "$lib/components/KeymapHud.svelte";
  import QuitDialog from "$lib/components/QuitDialog.svelte";
  import UpdateBanner from "$lib/components/UpdateBanner.svelte";
  import {
    loadKeymap,
    keymapState,
    hudVisible,
    enterTree as keymapEnterTree,
    rearmTree as keymapRearmTree,
    exitTree as keymapExitTree,
  } from "$lib/keymap/store";
  import { resolveKey } from "$lib/keymap/resolve";
  import {
    closeCommandSurface,
    commandSurface,
    openLeaderPrompt,
    setLeaderPromptValue,
    toggleCommandSurface,
  } from "$lib/stores/commandSurface";
  import { runStartupCheck, runManualCheck } from "$lib/stores/updater";
  import { initSettings, settings } from "$lib/stores/settings";
  import { projects } from "$lib/stores/projects";
  import { addSession, setActiveSession, sessionState, updateSessionStatus } from "$lib/stores/sessions";
  import { addOrUpdateWatch, watchState, ghAvailable as ghAvailableStore, flashSession } from "$lib/stores/watches";
  import { hydrateNotifications, applyNotificationEvent } from "$lib/stores/notifications";
  import { initSession, initSessionWithProfile, splitPane } from "$lib/panes/actions";
  import { hasSplitPanes } from "$lib/panes/layout";
  import { setLogicalFocus, focusedPaneId } from "$lib/panes/focus";
  import { getTerminalController } from "$lib/panes/terminalRuntime";
  import { initPersistence, flushPaneState, loadPaneState } from "$lib/panes/persistence";
  import { loadBuiltinProfiles, type SpawnProfileRef } from "$lib/panes/profiles";
  import { loadBuiltinLayouts, loadUserLayouts } from "$lib/panes/layouts";
  import {
    customProfileModalState,
    submitCustomProfile,
    closeCustomProfileEditor,
  } from "$lib/stores/customProfileModal";
  import { routeStatusUpdate, applyStatusRouting } from "$lib/panes/statusRouting";
  import { initAgentNotifications } from "$lib/panes/agentNotifications";
  import { clearPermissionInfo } from "$lib/panes/agentState";
  import { listSessions, checkSetupStatus, checkSetupNeeded, onRouxStatusUpdate, onAgentAttentionCleared, onRouxCommand, spawnShell, onWatchUpdate, listWatches, onNotificationEvent, quitApp, submitRouxReply } from "$lib/tauri";
  import { collectPaneTree } from "$lib/panes/query";
  import { profileRegistry } from "$lib/panes/profiles";
  import { runProfileInPane } from "$lib/panes/profileRunner";
  import type { RouxCommand } from "$lib/tauri";
  import { listen } from "@tauri-apps/api/event";
  import { registerCommands, registry } from "$lib/commands";
  import { closeFocusedPane } from "$lib/panes/actions";
  import { queries } from "$lib/queries";
  import { normalizeTheme, isLightTheme } from "$lib/themes";
  import { initLogging, log, logError } from "$lib/logging";
  import { isMacPlatform } from "$lib/platform";
  import {
    armSessionHints,
    hideSessionHints,
    armPaneHints,
    hidePaneHints,
  } from "$lib/stores/ui";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);
  let showNotes = $state(false);
  let showWatches = $state(false);
  let showNotifications = $state(false);
  let showSetupPrompt = $state(false);
  let showQuitDialog = $state(false);

  /** Returns true if a pane was closed, false if there was nothing to close */
  async function closeCurrentFocusedPane(): Promise<boolean> {
    const state = get(sessionState);
    if (!state.activeSessionId) return false;
    return closeFocusedPane(state.activeSessionId);
  }

  async function forceQuit() {
    // Flush any pending pane state debounce before quitting.
    try { await flushPaneState(); } catch {}
    // Do NOT close/remove sessions here. The Rust quit_app command kills PTYs
    // and persists sessions to disk (they load as "disconnected" on next launch).
    // Removing sessions before quit empties sessions.json, breaking restore.
    quitApp();
  }

  async function handleQuitRequested() {
    if (showQuitDialog) return; // Already showing
    const state = get(sessionState);
    if (state.sessions.length > 0 && get(settings).confirmOnQuit) {
      showQuitDialog = true;
      return;
    }
    await forceQuit();
  }

  async function handleCloseRequested() {
    const state = get(sessionState);
    if (!state.activeSessionId) {
      quitApp();
      return;
    }
    // Cmd+W: close focused split pane first
    if (hasSplitPanes(state.activeSessionId)) {
      const closed = await closeCurrentFocusedPane();
      if (closed) return;
    }
    // No split panes left — trigger quit flow
    await handleQuitRequested();
  }

  function executeCommandById(commandId: string) {
    // Keymap-owned pseudo-commands: not in the registry, but callable from
    // binds via their string id.
    if (commandId === "keymap.exit-tree") {
      keymapExitTree();
      return;
    }
    if (commandId === "keymap.reload") {
      void loadKeymap();
      return;
    }

    const cmd = registry.get(commandId);
    if (!cmd) return;

    if (cmd.id === "session.new") {
      showNewSessionDialog = true;
      return;
    }
    if (cmd.id === "app.settings") {
      showSettings = !showSettings;
      if (showSettings) showNotes = false;
      return;
    }
    if (cmd.id === "ui.toggle-notes") {
      showNotes = !showNotes;
      if (showNotes) showSettings = false;
      return;
    }
    if (cmd.id === "ui.toggle-watches") {
      showWatches = !showWatches;
      if (showWatches) { showSettings = false; showNotes = false; showNotifications = false; }
      return;
    }
    if (cmd.id === "ui.toggle-notifications") {
      showNotifications = !showNotifications;
      if (showNotifications) { showSettings = false; showNotes = false; showWatches = false; }
      return;
    }
    if (cmd.id === "app.command-palette") {
      toggleCommandSurface("palette");
      return;
    }
    if (cmd.id === "app.leader-mode") {
      keymapEnterTree("leader");
      return;
    }
    if (cmd.id === "app.quit") {
      void handleQuitRequested();
      return;
    }

    // onInput commands drop into a prompt UI rather than executing.
    if (cmd.onInput) {
      openLeaderPrompt(cmd.id, getLeaderPromptInitialValue(cmd.id));
      return;
    }

    if (cmd.execute) void cmd.execute();
  }

  function dispatchKeymapAction(action: { kind: "command"; id: string } | { kind: "enterTree"; tree: string }) {
    if (action.kind === "enterTree") {
      keymapEnterTree(action.tree);
      return;
    }
    executeCommandById(action.id);
  }

  function isCommandAvailable(commandId: string): boolean {
    const cmd = registry.get(commandId);
    if (!cmd) return false;
    return !cmd.available || cmd.available();
  }

  function getLeaderPromptInitialValue(commandId: string): string {
    if (commandId === "pane.rename") {
      return queries.focusedPane()?.name ?? "";
    }
    return "";
  }

  function submitLeaderPrompt() {
    const surface = get(commandSurface);
    if (!surface.leaderPromptCommandId) return;
    const cmd = registry.get(surface.leaderPromptCommandId);
    if (!cmd?.onInput) return;
    const value = surface.leaderPromptValue;
    closeCommandSurface();
    void cmd.onInput(value);
  }

  function handleKeyDown(e: KeyboardEvent) {
    // Arm the session-hint overlay when the platform primary modifier is
    // pressed on its own. The store handles the 200ms delay; quick chords
    // like Cmd/Ctrl+K or Cmd/Ctrl+1 release before the delay elapses and
    // never reveal the overlay.
    if ((isMacPlatform() && e.key === "Meta") || (!isMacPlatform() && e.key === "Control")) {
      if ($settings.showSessionHintsOnCommand !== false) armSessionHints();
    }
    if (e.key === "Alt") {
      if ($settings.showPaneHintsOnOption) armPaneHints();
    }

    // Preserve Escape-blur fix for terminal focus. WebKit otherwise drops
    // xterm's hidden textarea focus when Escape is pressed outside a pane.
    if (e.key === "Escape") {
      const focused = get(focusedPaneId);
      if (focused && getTerminalController(focused)) {
        e.preventDefault();
      }
    }

    // Command surfaces own keyboard focus while open. Don't fire keymap
    // binds while the palette is searching or the leader prompt is capturing
    // a rename input.
    const surface = get(commandSurface);
    if (surface.open && surface.mode === "palette") {
      // Palette handles its own keys; stay out of the way.
      return;
    }
    if (surface.open && surface.mode === "leader" && surface.leaderPromptCommandId) {
      if (e.key === "Escape") {
        e.preventDefault();
        closeCommandSurface();
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        submitLeaderPrompt();
        return;
      }
      return;
    }

    // Keymap dispatch.
    const km = get(keymapState);
    const resolution = resolveKey(e, km, isCommandAvailable);
    switch (resolution.kind) {
      case "enterTree":
        e.preventDefault();
        // Preserve Roux's prefix-toggle muscle memory: pressing the prefix
        // while any tree is active exits. Applies whether we're at the
        // root of the leader tree or nested inside a drill-down.
        if (km.treePath.length > 0) {
          keymapExitTree();
          return;
        }
        keymapRearmTree(resolution.tree);
        return;
      case "drillInto":
        e.preventDefault();
        keymapEnterTree(resolution.tree);
        return;
      case "chord":
        e.preventDefault();
        dispatchKeymapAction(resolution.action);
        if (!resolution.keepTreeOpen) keymapExitTree();
        return;
      case "passthrough":
        return;
      case "exit":
        e.preventDefault();
        keymapExitTree();
        return;
      case "none":
        break;
    }

  }

  function handleKeyUp(e: KeyboardEvent) {
    if ((isMacPlatform() && e.key === "Meta") || (!isMacPlatform() && e.key === "Control")) {
      hideSessionHints();
    }
    if (e.key === "Alt") hidePaneHints();
  }

  function handleWindowBlur() {
    hideSessionHints();
    hidePaneHints();
  }

  let prevSurfaceOpen = $state(false);
  $effect(() => {
    const open = $commandSurface.open;
    if (prevSurfaceOpen && !open) {
      queueMicrotask(() => {
        const active = document.activeElement as HTMLElement | null;
        if (active && active !== document.body && active.tagName !== "HTML") return;
        const focused = get(focusedPaneId);
        if (focused) setLogicalFocus(focused);
      });
    }
    prevSurfaceOpen = open;
  });

  $effect(() => {
    const theme = normalizeTheme($settings.theme);
    document.documentElement.dataset.theme = theme;
    document.body.dataset.theme = theme;
    document.documentElement.style.setProperty("--font-sans", $settings.uiFontFamily ?? "sans-serif");
    document.documentElement.style.colorScheme = isLightTheme(theme) ? "light" : "dark";
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeyDown, true);
    window.removeEventListener("keyup", handleKeyUp, true);
    window.removeEventListener("blur", handleWindowBlur);
  });

  onMount(async () => {
    // One-shot cleanup of old localStorage-backed pane state keys.
    // These were used before pane state moved to disk (per-session JSON files).
    try {
      localStorage.removeItem("roux:pane-layouts-v2");
      localStorage.removeItem("roux:pane-descriptors");
    } catch {}

    registerCommands();
    void loadKeymap();
    // Use capture phase so we intercept before xterm.js swallows the event
    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);
    window.addEventListener("blur", handleWindowBlur);

    // Listen for Tauri close-requested event (red button / Cmd+W)
    await listen("close-requested", () => void handleCloseRequested());
    // Listen for macOS Quit menu / Dock quit
    await listen("quit-requested", () => void handleQuitRequested());

    const loadedSettings = await initSettings();
    await initLogging(loadedSettings.enableLogging ?? false);
    log(`Settings loaded, restoreSessionsOnLaunch=${loadedSettings.restoreSessionsOnLaunch}`);

    // Populate the built-in spawn-profile registry so pane pickers and
    // restored panes can resolve { kind: "registered", id: "claude" } etc.
    // User profiles are already loaded by initSettings via setUserProfiles.
    void loadBuiltinProfiles();
    void loadBuiltinLayouts();
    void loadUserLayouts();

    // Start watching agent-state transitions so per-pane generating→idle
    // transitions fire a completion notification. Window-focus suppression
    // and OS fan-out happen on the Rust side of notificationsPush.
    initAgentNotifications();

    // Kick off a silent background update check (5s debounce, respects user toggle)
    runStartupCheck();

    // Check CLI setup and tool availability. The Doctor panel covers
    // cli/hooks/skill; gh is informational (still tracked for the UI).
    const status = await checkSetupStatus();
    ghAvailableStore.set(status.ghAvailable);
    const setupNeeded = await checkSetupNeeded();
    if (setupNeeded) {
      log("First-time setup needed");
      showSetupPrompt = true;
    }

    // Load projects (global, independent of session restore)
    const { loadProjects } = await import("$lib/stores/projects");
    await loadProjects();

    if (loadedSettings.restoreSessionsOnLaunch) {
      const sessions = await listSessions();
      log(`Restoring ${sessions.length} session(s)`);
      const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
      for (const s of sessions) {
        addSession(s);
        // Look up the persisted primary descriptor so the restored pane
        // keeps its spawnProfileRef. Without this, every session would
        // come back tagged as the Claude built-in profile regardless of
        // what the user actually chose at creation time, and the re-run
        // button + provider-specific UI would lie.
        const persisted = await loadPaneState(s.id);
        const primaryDescriptor = persisted?.descriptors.find(
          (d) => d.ptyId === s.id,
        );
        const mainPaneId = primaryDescriptor?.spawnProfileRef
          ? initSessionWithProfile(s.id, primaryDescriptor.spawnProfileRef)
          : initSession(s.id);
        initTerminal(mainPaneId);
        await attachPtyListeners(mainPaneId);
        // Full layout restore (shell PTY re-spawn etc.) happens on reconnect click.
        // Startup only sets up the main pane in disconnected state.
      }
    }

    // Start auto-saving layout changes to localStorage
    initPersistence();

    // Listen for commands from roux-cli via socket server
    await onRouxCommand(async (cmd: RouxCommand) => {
      log(`roux-command: ${JSON.stringify(cmd)}`);
      switch (cmd.action) {
        case "split": {
          const sessionId = cmd.sessionId;
          if (!sessionId) break;
          const ptyId = crypto.randomUUID();
          const paneId = crypto.randomUUID();
          const session = $sessionState.sessions.find((s) => s.id === sessionId);
          if (!session) break;
          spawnShell(ptyId, session.worktreePath, session.id, paneId).then(async () => {
            const direction = cmd.direction === "vertical" ? "v" : "h";
            const newPaneId = splitPane(sessionId, direction, { id: paneId, type: "shell", ptyId });
            if (newPaneId) {
              const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
              initTerminal(newPaneId);
              await attachPtyListeners(newPaneId);
            }
          }).catch((e) => logError("Failed to spawn shell for socket split", e));
          break;
        }
        case "session-created": {
          // Reload sessions to pick up the newly created one
          const profileId = cmd.profileId;
          listSessions().then(async (sessions) => {
            const newSession = sessions.find((s) => s.id === cmd.sessionId);
            if (!newSession) return;
            addSession(newSession);
            // Default to the Claude built-in if the socket didn't specify a
            // profile. Use initSessionWithProfile so the pane instance carries
            // the spawnProfileRef — persistence + reconnect read it from
            // there to replay the startup command on reconnect. Bare
            // initSession drops the ref, so socket-created sessions would
            // come back as plain shells after restart.
            const effectiveProfileId = profileId ?? "claude";
            const profileRef: SpawnProfileRef = { kind: "registered", id: effectiveProfileId };
            const mainPaneId = initSessionWithProfile(newSession.id, profileRef);
            const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
            initTerminal(mainPaneId);
            await attachPtyListeners(mainPaneId);
            // Backend spawned a bare shell via create_session_shell; the
            // frontend replays every profile's startup command into it,
            // including Claude (the legacy direct-spawn path is gone).
            const profile = get(profileRegistry).get(effectiveProfileId);
            if (profile) {
              runProfileInPane(newSession.id, profile).catch((e) =>
                logError(`runProfileInPane failed for ${effectiveProfileId}`, e),
              );
            } else {
              logError(
                `session-created: profile '${effectiveProfileId}' not in registry; startup commands skipped`,
                null,
              );
            }
          });
          break;
        }
        case "shell-opened": {
          const sessionId = cmd.sessionId;
          if (!sessionId || !cmd.paneId || !cmd.ptyId) break;
          // Use the backend-provided paneId so socket focus commands can target it
          const newPaneId = splitPane(sessionId, "h", { id: cmd.paneId, type: "shell", ptyId: cmd.ptyId });
          if (newPaneId) {
            const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
            initTerminal(newPaneId);
            await attachPtyListeners(newPaneId);
          }
          break;
        }
        case "command-opened": {
          const sessionId = cmd.sessionId;
          if (!sessionId || !cmd.paneId || !cmd.ptyId) {
            log(`command-opened: missing fields session=${cmd.sessionId} pane=${cmd.paneId} pty=${cmd.ptyId}`);
            break;
          }
          log(`command-opened: session=${sessionId} pane=${cmd.paneId} pty=${cmd.ptyId} cmd=${cmd.command}`);
          // Use the backend-provided paneId so socket focus commands can target it
          const newPaneId = splitPane(sessionId, "h", {
            id: cmd.paneId,
            type: "command",
            ptyId: cmd.ptyId,
            command: cmd.command,
            workingDir: cmd.workingDir,
          });
          log(`command-opened: splitPane returned ${newPaneId}`);
          if (newPaneId) {
            const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
            initTerminal(newPaneId);
            const { updateInstance } = await import("$lib/panes/instances");
            updateInstance(newPaneId, {
              commandStatus: "running" as const,
              commandStartedAt: Date.now(),
            });
            await attachPtyListeners(newPaneId);
            log(`command-opened: terminal and listeners attached for ${newPaneId}`);
          }
          break;
        }
        case "focus": {
          if (cmd.sessionId) {
            setActiveSession(cmd.sessionId);
          }
          if (cmd.paneId) {
            setLogicalFocus(cmd.paneId);
          }
          break;
        }
        case "panes-list-request": {
          if (!cmd.sessionId || !cmd.requestId) break;
          try {
            const snapshot = collectPaneTree(cmd.sessionId);
            await submitRouxReply(cmd.requestId, snapshot);
          } catch (e) {
            logError("panes-list-request failed", e);
            await submitRouxReply(cmd.requestId, { error: String(e) }).catch(() => {});
          }
          break;
        }
        case "pane-create": {
          if (!cmd.sessionId || !cmd.requestId) break;
          const requestId = cmd.requestId;
          const sessionId = cmd.sessionId;
          try {
            const session = $sessionState.sessions.find((s) => s.id === sessionId);
            if (!session) throw new Error("session not found");
            const workingDir = cmd.workingDir ?? session.worktreePath;
            const direction = cmd.direction === "vertical" ? "v" : "h";
            const profileId = cmd.profileId ?? "plain-shell";

            const ptyId = crypto.randomUUID();
            const paneId = crypto.randomUUID();
            await spawnShell(ptyId, workingDir, sessionId, paneId);

            // Bare shell marker — no profile startup commands. Any other id
            // is backend-validated, so the registry lookup should succeed;
            // throw if it doesn't so the CLI caller sees the failure.
            let profile = null;
            if (profileId !== "plain-shell") {
              profile = get(profileRegistry).get(profileId) ?? null;
              if (!profile) {
                throw new Error(`profile '${profileId}' not found in registry`);
              }
            }

            const newPaneId = splitPane(sessionId, direction, {
              id: paneId,
              type: "shell",
              ptyId,
              workingDir,
              spawnProfileRef: profile
                ? { kind: "registered", id: profile.id }
                : undefined,
            });
            if (!newPaneId) throw new Error("splitPane returned null");

            const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
            initTerminal(newPaneId);
            await attachPtyListeners(newPaneId);

            if (profile) {
              // Fire-and-forget: startup commands are typed into the live PTY;
              // the CLI caller doesn't wait for them to finish running.
              runProfileInPane(ptyId, profile).catch((e) =>
                logError(`runProfileInPane failed for ${profile.id}`, e),
              );
            }

            await submitRouxReply(requestId, { pane_id: paneId, pty_id: ptyId });
          } catch (e) {
            logError("pane-create failed", e);
            await submitRouxReply(requestId, { error: String(e) }).catch(() => {});
          }
          break;
        }
      }
    });

    // Hydrate watches from backend
    listWatches().then((watches) => {
      watchState.set(watches);
    });

    // Listen for watch updates
    await onWatchUpdate((event) => {
      addOrUpdateWatch(event.watch);
      if (event.changed && event.watch.scope.type === "session") {
        flashSession(event.watch.scope.sessionId);
      }
    });

    // Hydrate + subscribe to notifications
    await hydrateNotifications();
    await onNotificationEvent((payload) => {
      applyNotificationEvent(payload);
    });

    // Listen for global status updates from hooks. Tier-1 routing (with a
    // `rouxPaneId` in the payload) updates the pane's runtime agentState so
    // the session-card aggregate and provider-specific UI light up. Legacy
    // events without a pane id fall through to cwd-based session status,
    // which still drives notification fan-out.
    await onRouxStatusUpdate((update) => {
      const routing = applyStatusRouting(routeStatusUpdate(update));
      if (routing.kind === "pane") {
        // Tier-1 routing already wrote to agentState; the session aggregate
        // is derived from pane state so we don't also poke session.status.
        return;
      }

      const sessions = $sessionState.sessions;
      const match = sessions.find(
        (s) => s.worktreePath === update.cwd || s.repoRoot === update.cwd,
      );
      if (match) {
        updateSessionStatus(match.id, update.status as any, null, null);
      }
    });

    // Backend FSM tells us when a pane exited `Attention`; clear any
    // stale `permissionInfo` so the Allow/Deny affordance disappears
    // alongside the auto-dismissed notification. Backend already gates
    // on the `autoClearAttentionState` setting — the event simply
    // doesn't fire when it's off.
    await onAgentAttentionCleared(({ paneId }) => {
      clearPermissionInfo(paneId);
    });
  });
</script>

<Layout
  onNewSession={() => (showNewSessionDialog = true)}
  onOpenSettings={() => (showSettings = !showSettings)}
  onToggleWatches={() => { showWatches = !showWatches; if (showWatches) { showSettings = false; showNotes = false; showNotifications = false; } }}
  onToggleNotifications={() => { showNotifications = !showNotifications; if (showNotifications) { showSettings = false; showNotes = false; showWatches = false; } }}
>
  {#snippet settingsPanel()}
    <SettingsPanel visible={showSettings} onclose={() => (showSettings = false)} />
    {@const activeSession = $sessionState.sessions.find(s => s.id === $sessionState.activeSessionId)}
    <NotesPanel
      visible={showNotes}
      projectId={activeSession?.projectId ?? null}
      projectName={$projects.find(p => p.id === activeSession?.projectId)?.name ?? null}
      onclose={() => (showNotes = false)}
    />
    <WatchesPane
      visible={showWatches}
      onclose={() => (showWatches = false)}
    />
    <NotificationsPane
      visible={showNotifications}
      onclose={() => (showNotifications = false)}
    />
  {/snippet}
</Layout>

<NewSessionDialog
  visible={showNewSessionDialog}
  onclose={() => (showNewSessionDialog = false)}
/>

<!-- Global custom-profile editor host. Opened by palette flows
     (split-with-profile) that can't mount their own modal. Resolves
     the pending `openCustomProfileEditor()` promise on submit/cancel. -->
<ProfileCustomEditor
  visible={$customProfileModalState.visible}
  onclose={closeCustomProfileEditor}
  onsubmit={submitCustomProfile}
/>

<CommandPalette
  open={$commandSurface.open && $commandSurface.mode === "palette"}
  onclose={closeCommandSurface}
  onNewSession={() => (showNewSessionDialog = true)}
  onSettings={() => (showSettings = !showSettings)}
  onCheckForUpdates={() => { showSettings = true; void runManualCheck(); }}
/>

{#if $hudVisible || ($commandSurface.open && $commandSurface.mode === "leader" && $commandSurface.leaderPromptCommandId)}
  <KeymapHud
    promptLabel={$commandSurface.leaderPromptCommandId ? registry.get($commandSurface.leaderPromptCommandId)?.label ?? "Input" : null}
    promptPlaceholder={$commandSurface.leaderPromptCommandId ? registry.get($commandSurface.leaderPromptCommandId)?.inputPlaceholder ?? "" : null}
    promptValue={$commandSurface.leaderPromptValue}
    onPromptInput={setLeaderPromptValue}
    onPromptSubmit={submitLeaderPrompt}
  />
{/if}

<UpdateBanner />

<QuitDialog
  visible={showQuitDialog}
  oncancel={() => (showQuitDialog = false)}
/>

<DoctorPanel
  mode="onboarding"
  visible={showSetupPrompt}
  ondone={() => (showSetupPrompt = false)}
/>
