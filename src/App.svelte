<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SetupPrompt from "$lib/components/SetupPrompt.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import NotesPanel from "$lib/components/NotesPanel.svelte";
  import WatchesPane from "$lib/components/WatchesPane.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import { initSettings, settings } from "$lib/stores/settings";
  import { projects } from "$lib/stores/projects";
  import { addSession, setActiveSession, sessionState, updateSessionStatus, updateSessionPermission } from "$lib/stores/sessions";
  import { addOrUpdateWatch, watchState, ghAvailable as ghAvailableStore, flashSession } from "$lib/stores/watches";
  import { initSession, splitPane } from "$lib/panes/actions";
  import { hasSplitPanes } from "$lib/panes/layout";
  import { setLogicalFocus, focusedPaneId } from "$lib/panes/focus";
  import { paneInstances } from "$lib/panes/instances";
  import { initPersistence, loadLayout, clearLayout } from "$lib/panes/persistence";
  import { listSessions, checkSetupStatus, onRouxStatusUpdate, onRouxCommand, spawnShell, onWatchUpdate, listWatches } from "$lib/tauri";
  import type { RouxCommand } from "$lib/tauri";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { registerCommands, registry } from "$lib/commands";
  import { closeFocusedPane } from "$lib/panes/actions";
  import { normalizeTheme, isLightTheme } from "$lib/themes";
  import { initLogging, log, logError } from "$lib/logging";
  import { hasPrimaryModifier, isMacPlatform } from "$lib/platform";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);
  let showNotes = $state(false);
  let showWatches = $state(false);
  let showPalette = $state(false);
  let showSetupPrompt = $state(false);
  let ghAvailable = $state(true);

  function buildShortcutString(e: KeyboardEvent): string {
    const parts: string[] = [];
    if (hasPrimaryModifier(e)) parts.push("cmd");
    if (e.shiftKey) parts.push("shift");
    if (e.altKey) parts.push("alt");
    if (e.ctrlKey && isMacPlatform()) parts.push("ctrl");
    if (e.metaKey && !isMacPlatform()) parts.push("meta");
    // On macOS, Alt produces special characters (e.g. Alt+h → ˙).
    // Use the physical key (e.code) when Alt is held so shortcuts work.
    let key = e.key.toLowerCase();
    if (e.altKey && e.code.startsWith("Key")) {
      key = e.code.slice(3).toLowerCase();
    }
    parts.push(key);
    return parts.join("+");
  }

  /** Returns true if a pane was closed, false if there was nothing to close */
  async function closeCurrentFocusedPane(): Promise<boolean> {
    const state = get(sessionState);
    if (!state.activeSessionId) return false;
    return closeFocusedPane(state.activeSessionId);
  }

  async function handleCloseRequested() {
    const state = get(sessionState);
    if (!state.activeSessionId) {
      getCurrentWindow().destroy();
      return;
    }
    if (hasSplitPanes(state.activeSessionId)) {
      const closed = await closeCurrentFocusedPane();
      if (closed) return;
    }
    getCurrentWindow().destroy();
  }

  function handleKeyDown(e: KeyboardEvent) {
    // Command palette toggle
    if (hasPrimaryModifier(e) && e.key === "k") {
      e.preventDefault();
      showPalette = !showPalette;
      return;
    }

    // Don't intercept shortcuts when palette is open
    if (showPalette) return;

    // Prevent WebKit from blurring xterm's hidden textarea on Escape.
    // Without this, pressing Escape (e.g. to leave vim insert mode) causes
    // the terminal to lose DOM focus and stop accepting keyboard input.
    if (e.key === "Escape") {
      const focused = get(focusedPaneId);
      if (focused && get(paneInstances).get(focused)?.terminal) {
        e.preventDefault();
      }
    }

    const shortcut = buildShortcutString(e);
    const cmd = registry.getByShortcut(shortcut);
    if (cmd) {
      e.preventDefault();

      // Handle special commands that need local state
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
        if (showWatches) { showSettings = false; showNotes = false; }
        return;
      }
      if (cmd.id === "app.command-palette") {
        showPalette = true;
        return;
      }

      if (cmd.execute) void cmd.execute();
    }
  }

  $effect(() => {
    const theme = normalizeTheme($settings.theme);
    document.documentElement.dataset.theme = theme;
    document.body.dataset.theme = theme;
    document.documentElement.style.setProperty("--font-sans", $settings.uiFontFamily);
    document.documentElement.style.colorScheme = isLightTheme(theme) ? "light" : "dark";
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeyDown, true);
  });

  onMount(async () => {
    registerCommands();
    // Use capture phase so we intercept before xterm.js swallows the event
    window.addEventListener("keydown", handleKeyDown, true);

    // Listen for Tauri close-requested event (Cmd+W or red button)
    await listen("close-requested", () => void handleCloseRequested());

    const loadedSettings = await initSettings();
    await initLogging(loadedSettings.enableLogging);
    log(`Settings loaded, restoreSessionsOnLaunch=${loadedSettings.restoreSessionsOnLaunch}`);

    // Check CLI setup and tool availability
    const status = await checkSetupStatus();
    ghAvailable = status.ghAvailable;
    ghAvailableStore.set(status.ghAvailable);
    if (!status.cliInstalled) {
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
        const mainPaneId = initSession(s.id);
        initTerminal(mainPaneId);
        await attachPtyListeners(mainPaneId);

        // Restore persisted layout if available (shell panes get fresh layouts;
        // command panes are stripped since their processes are gone)
        const persisted = loadLayout(s.id);
        if (persisted) {
          // For now we just use the fresh single-pane layout created by
          // initSession. Full shell pane restore (spawn fresh PTYs for each
          // persisted shell) is deferred to a later iteration.
          clearLayout(s.id);
        }
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
          const session = $sessionState.sessions.find((s) => s.id === sessionId);
          if (!session) break;
          spawnShell(ptyId, session.worktreePath).then(async () => {
            const direction = cmd.direction === "vertical" ? "v" : "h";
            const newPaneId = splitPane(sessionId, direction, { type: "shell", ptyId });
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
          listSessions().then(async (sessions) => {
            const newSession = sessions.find((s) => s.id === cmd.sessionId);
            if (newSession) {
              addSession(newSession);
              const mainPaneId = initSession(newSession.id);
              const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
              initTerminal(mainPaneId);
              await attachPtyListeners(mainPaneId);
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

    // Listen for global status updates from hooks and match by cwd
    await onRouxStatusUpdate((update) => {
      const sessions = $sessionState.sessions;
      const match = sessions.find(
        (s) => s.worktreePath === update.cwd || s.repoRoot === update.cwd
      );
      if (match) {
        updateSessionStatus(match.id, update.status as any, null, null);
        if (update.status === "attention") {
          if (update.toolName) {
            updateSessionPermission(match.id, {
              toolName: update.toolName,
              toolInput: update.toolInput ?? {},
              message: update.message ?? "",
            });
          } else if (update.message && !match.permissionInfo) {
            updateSessionPermission(match.id, {
              toolName: "",
              toolInput: {},
              message: update.message,
            });
          }
        } else {
          updateSessionPermission(match.id, null);
        }
      }
    });
  });
</script>

<Layout
  onNewSession={() => (showNewSessionDialog = true)}
  onOpenSettings={() => (showSettings = !showSettings)}
  onToggleWatches={() => { showWatches = !showWatches; if (showWatches) { showSettings = false; showNotes = false; } }}
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
  {/snippet}
</Layout>

<NewSessionDialog
  visible={showNewSessionDialog}
  onclose={() => (showNewSessionDialog = false)}
/>

<CommandPalette
  open={showPalette}
  onclose={() => (showPalette = false)}
  onNewSession={() => (showNewSessionDialog = true)}
  onSettings={() => (showSettings = !showSettings)}
/>

<SetupPrompt
  visible={showSetupPrompt}
  {ghAvailable}
  ondone={() => (showSetupPrompt = false)}
/>
