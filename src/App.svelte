<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import ProfileCustomEditor from "$lib/components/ProfileCustomEditor.svelte";
  import DoctorPanel from "$lib/components/DoctorPanel.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import MultiLineEditor from "$lib/components/MultiLineEditor.svelte";
  import LibraryWindow from "$lib/components/LibraryWindow.svelte";
  import LibraryVariablePrompt from "$lib/components/LibraryVariablePrompt.svelte";
  import { multiLineEditor } from "$lib/stores/multiLineEditor";
  import { libraryWindow } from "$lib/stores/libraryWindow";
  import { libraryVariablePrompt } from "$lib/stores/libraryVariablePrompt";
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
    openCommandPaletteWithCommand,
    openLeaderPrompt,
    setLeaderPromptValue,
    toggleCommandSurface,
  } from "$lib/stores/commandSurface";
  import { runStartupCheck, runManualCheck } from "$lib/stores/updater";
  import { initSettings, settings } from "$lib/stores/settings";
  import { addSession, setActiveSession, sessionState, updateSessionStatus } from "$lib/stores/sessions";
  import { addOrUpdateWatch, watchState, ghAvailable as ghAvailableStore, flashSession } from "$lib/stores/watches";
  import { hydrateNotifications, applyNotificationEvent } from "$lib/stores/notifications";
  import { initPtyInventoryPolling } from "$lib/stores/ptyInventory";
  import { initSessionWithProfile, splitPane } from "$lib/panes/actions";
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
  import {
    newProjectDialogState,
    closeNewProjectDialog,
  } from "$lib/stores/newProjectDialog";
  import NewProjectDialog from "$lib/components/NewProjectDialog.svelte";
  import { routeStatusUpdate, applyStatusRouting } from "$lib/panes/statusRouting";
  import { initAgentNotifications } from "$lib/panes/agentNotifications";
  import { installSessionPrEffect } from "$lib/stores/sessionPrLookup";
  import { clearPermissionInfo } from "$lib/panes/agentState";
  import { listSessions, checkSetupStatus, checkSetupNeeded, onRouxStatusUpdate, onAgentAttentionCleared, onRouxCommand, spawnShell, onWatchUpdate, listWatches, onNotificationEvent, quitApp, submitRouxReply } from "$lib/tauri";
  import { collectPaneTree } from "$lib/panes/query";
  import { profileRegistry } from "$lib/panes/profiles";
  import { runProfileInPane } from "$lib/panes/profileRunner";
  import { getProjectPrompt } from "$lib/stores/projects";
  import type { RouxCommand } from "$lib/tauri";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { handleFileDrop } from "$lib/dnd/handleFileDrop";
  import { registerCommands, registry } from "$lib/commands";
  import { setupAppMenu, teardownAppMenu, claimFire } from "$lib/menu/appMenu";
  import { eventToAccelerator } from "$lib/menu/accelerators";
  import { closeFocusedPane } from "$lib/panes/actions";
  import { queries } from "$lib/queries";
  import { normalizeTheme, isLightTheme, resolveTerminalTheme } from "$lib/themes";
  import { userTerminalThemes, loadUserTerminalThemes } from "$lib/stores/userTerminalThemes";
  import { initLogging, log, logError } from "$lib/logging";
  import { isMacPlatform } from "$lib/platform";
  import {
    armSessionHints,
    hideSessionHints,
    armPaneHints,
    hidePaneHints,
    activeSidebar,
    openSidebar,
    closeSidebar,
    toggleSidebar,
  } from "$lib/stores/ui";

  let showNewSessionDialog = $state(false);
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
      toggleSidebar("settings");
      return;
    }
    if (cmd.id === "ui.toggle-notes") {
      toggleSidebar("notes");
      return;
    }
    if (cmd.id === "ui.toggle-watches") {
      toggleSidebar("watches");
      return;
    }
    if (cmd.id === "ui.toggle-notifications") {
      toggleSidebar("notifications");
      return;
    }
    if (cmd.id === "ui.toggle-sessions") {
      toggleSidebar("sessions");
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

    if (cmd.getItems) {
      openCommandPaletteWithCommand(cmd.id);
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
    // Dedup OS-level menu accelerators against the in-webview keymap
    // dispatcher. When a chord matches an active menu item's accelerator
    // Tauri fires the menu action in the native menu handler while the
    // webview still receives the keydown; without this claim the command
    // would run twice.
    const menuAccelerator = eventToAccelerator(e);
    if (menuAccelerator && !claimFire(menuAccelerator)) {
      e.preventDefault();
      return;
    }

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
    // MultiLineEditor modal owns all keys while open — otherwise global
    // chords like Cmd+D (split pane) would fire while the user is editing.
    if (get(multiLineEditor).open) {
      return;
    }
    if (get(libraryWindow).open) {
      return;
    }
    if (get(libraryVariablePrompt).open) {
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
        // While a tree is armed, unbound keys are dropped per the
        // keymap contract (`resolve.ts` §1e). Without preventDefault
        // the character leaks through to xterm while the tree stays
        // armed — user sees typing land in the terminal mid-chord.
        if (km.treePath.length > 0) e.preventDefault();
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
    // Drive the terminal frame chrome from the *actual* terminal palette so a
    // light terminal theme inside a light GUI doesn't get wrapped in a dark
    // frame (and vice versa).
    const terminalBg = resolveTerminalTheme(
      theme,
      $settings.terminalTheme,
      $userTerminalThemes,
    ).background;
    document.documentElement.style.setProperty("--color-terminal-bg", terminalBg);
  });

  let unlistenDragDrop: (() => void) | null = null;
  let unlistenSessionPrEffect: (() => void) | null = null;
  let stopPtyInventoryPolling: (() => void) | null = null;
  let tauriUnlisteners: Array<() => void> = [];

  function cleanupAppLifecycle() {
    window.removeEventListener("keydown", handleKeyDown, true);
    window.removeEventListener("keyup", handleKeyUp, true);
    window.removeEventListener("blur", handleWindowBlur);
    window.removeEventListener("beforeunload", cleanupAppLifecycle);
    for (const unlisten of tauriUnlisteners.splice(0)) {
      try {
        unlisten();
      } catch {
        // Best-effort cleanup during reload/shutdown.
      }
    }
    unlistenDragDrop?.();
    unlistenDragDrop = null;
    unlistenSessionPrEffect?.();
    unlistenSessionPrEffect = null;
    stopPtyInventoryPolling?.();
    stopPtyInventoryPolling = null;
    teardownAppMenu();
  }

  onDestroy(() => {
    cleanupAppLifecycle();
  });

  onMount(async () => {
    // One-shot cleanup of old localStorage-backed pane state keys.
    // These were used before pane state moved to disk (per-session JSON files).
    try {
      localStorage.removeItem("roux:pane-layouts-v2");
      localStorage.removeItem("roux:pane-descriptors");
    } catch {}

    registerCommands();
    // Await keymap load so the native menu's accelerators reflect the
    // current preset on first paint. Without this the menu builds with
    // the empty default keymap and has to rebuild once the real one
    // arrives.
    await loadKeymap();
    await setupAppMenu(executeCommandById);
    // Use capture phase so we intercept before xterm.js swallows the event
    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);
    window.addEventListener("blur", handleWindowBlur);
    window.addEventListener("beforeunload", cleanupAppLifecycle);

    // Listen for Tauri close-requested event (red button / Cmd+W)
    tauriUnlisteners.push(await listen("close-requested", () => void handleCloseRequested()));
    // Listen for macOS Quit menu / Dock quit
    tauriUnlisteners.push(await listen("quit-requested", () => void handleQuitRequested()));

    // Native file drag-and-drop: write the dropped path(s) into the target pane's terminal.
    // Tauri reports drop position in PHYSICAL pixels; document.elementFromPoint expects CSS
    // (logical) pixels, so divide by the current scaleFactor for correct hit-testing on HiDPI.
    const dragDropWebview = getCurrentWebviewWindow();
    unlistenDragDrop = await dragDropWebview.onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const { paths, position } = event.payload;
      void (async () => {
        try {
          const scale = await dragDropWebview.scaleFactor();
          await handleFileDrop({
            paths,
            position: { x: position.x / scale, y: position.y / scale },
          });
        } catch (error) {
          logError("Failed to handle dropped file(s)", error);
        }
      })();
    });

    const loadedSettings = await initSettings();
    void loadUserTerminalThemes();
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

    // Resolve the active session's branch to an open PR (when gh is
    // available) so the status bar can render a PR chip and the optional
    // auto-watch flow can create a session-scoped PR watch.
    unlistenSessionPrEffect = installSessionPrEffect();

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

    // Probe worktrunk once at launch so the activity rail can conditionally
    // render the Worktrunk icon without each consumer running its own probe.
    // Non-blocking; failures leave the store in "not detected" state.
    const { refreshWorktrunkDetection } = await import(
      "$lib/stores/worktrunkDetection"
    );
    void refreshWorktrunkDetection();

    if (loadedSettings.restoreSessionsOnLaunch) {
      const sessions = await listSessions();
      log(`Restoring ${sessions.length} session(s)`);
      // Fan out a worktrunk-metadata refresh in parallel so session cards
      // can surface dirty/ahead/behind/dev-server chips without each card
      // making its own Tauri call. Non-blocking; failures are silent.
      const { refreshWorktreeMetadataForRepos } = await import(
        "$lib/stores/worktreeMetadata"
      );
      void refreshWorktreeMetadataForRepos(sessions.map((s) => s.repoRoot));
      const [{ initTerminal, attachPtyListeners }, { attachPtyToPane }, { listAllPtys }] = await Promise.all([
        import("$lib/panes/terminals"),
        import("$lib/panes/attach"),
        import("$lib/tauri"),
      ]);
      const { restoreSessionPanes } = await import("$lib/panes/restore");
      let livePtyIds: Set<string> | null = null;
      try {
        livePtyIds = new Set((await listAllPtys()).map((pty) => pty.id));
      } catch (e) {
        livePtyIds = null;
        log(`Unable to read live PTY inventory during restore: ${e}`);
      }
      for (const s of sessions) {
        addSession(s);
        const persisted = await loadPaneState(s.id);
        await restoreSessionPanes(s, persisted, {
          initTerminal,
          attachPtyListeners,
          attachLivePtyToPane: attachPtyToPane,
          livePtyIds,
        });
      }
    }

    stopPtyInventoryPolling = initPtyInventoryPolling();

    // Start auto-saving layout changes to localStorage
    initPersistence();

    // Listen for commands from roux-cli via socket server
    tauriUnlisteners.push(await onRouxCommand(async (cmd: RouxCommand) => {
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
              runProfileInPane(newSession.id, profile, {
                appendSystemPrompt: getProjectPrompt(newSession.projectId),
              }).catch((e) =>
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
              runProfileInPane(ptyId, profile, {
                appendSystemPrompt: getProjectPrompt(session.projectId),
              }).catch((e) =>
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
    }));

    // Hydrate watches from backend
    listWatches().then((watches) => {
      watchState.set(watches);
    });

    // Listen for watch updates
    tauriUnlisteners.push(await onWatchUpdate((event) => {
      addOrUpdateWatch(event.watch);
      if (event.changed && event.watch.scope.type === "session") {
        flashSession(event.watch.scope.sessionId);
      }
    }));

    // Hydrate + subscribe to notifications
    await hydrateNotifications();
    tauriUnlisteners.push(await onNotificationEvent((payload) => {
      applyNotificationEvent(payload);
    }));

    // Listen for global status updates from hooks. Tier-1 routing (with a
    // `rouxPaneId` in the payload) updates the pane's runtime agentState so
    // the session-card aggregate and provider-specific UI light up. Legacy
    // events without a pane id fall through to cwd-based session status,
    // which still drives notification fan-out.
    tauriUnlisteners.push(await onRouxStatusUpdate((update) => {
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
    }));

    // Backend FSM tells us when a pane exited `Attention`; clear any
    // stale `permissionInfo` so the Allow/Deny affordance disappears
    // alongside the auto-dismissed notification. Backend already gates
    // on the `autoClearAttentionState` setting — the event simply
    // doesn't fire when it's off.
    tauriUnlisteners.push(await onAgentAttentionCleared(({ paneId }) => {
      clearPermissionInfo(paneId);
    }));
  });
</script>

<Layout
  onNewSession={() => (showNewSessionDialog = true)}
>
  {#snippet settingsPanel()}
    <SettingsPanel visible={$activeSidebar === "settings"} onclose={closeSidebar} />
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

<!-- Global new-project dialog host. Driven by `newProjectDialogState`,
     flipped by the `project.new` / `project.edit` commands. -->
<NewProjectDialog
  visible={$newProjectDialogState.visible}
  project={$newProjectDialogState.project}
  onclose={closeNewProjectDialog}
/>

<CommandPalette
  open={$commandSurface.open && $commandSurface.mode === "palette"}
  onclose={closeCommandSurface}
  onNewSession={() => (showNewSessionDialog = true)}
  onSettings={() => toggleSidebar("settings")}
  onCheckForUpdates={() => { openSidebar("settings"); void runManualCheck(); }}
  initialCommandId={$commandSurface.initialCommandId}
/>

<MultiLineEditor />

<LibraryWindow />

<LibraryVariablePrompt />

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
