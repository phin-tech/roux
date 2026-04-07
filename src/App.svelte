<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SetupPrompt from "$lib/components/SetupPrompt.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import NotesPanel from "$lib/components/NotesPanel.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import { initSettings, settings } from "$lib/stores/settings";
  import { projects } from "$lib/stores/projects";
  import { addSession, setActiveSession, sessionState, updateSessionStatus, updateSessionPermission } from "$lib/stores/sessions";
  import { addSplit, initSessionPanes, hasSplitPanes, focusedPaneId } from "$lib/stores/panes";
  import { listSessions, checkSetupNeeded, onRouxStatusUpdate, onRouxCommand, spawnShell } from "$lib/tauri";
  import type { RouxCommand } from "$lib/tauri";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { registerCommands, registry } from "$lib/commands";
  import { closeFocusedPane } from "$lib/panes/actions";
  import { normalizeTheme, isLightTheme } from "$lib/themes";
  import { initLogging, log, logError } from "$lib/logging";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);
  let showNotes = $state(false);
  let showPalette = $state(false);
  let showSetupPrompt = $state(false);

  function buildShortcutString(e: KeyboardEvent): string {
    const parts: string[] = [];
    if (e.metaKey) parts.push("cmd");
    if (e.shiftKey) parts.push("shift");
    if (e.altKey) parts.push("alt");
    if (e.ctrlKey) parts.push("ctrl");
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
    if (e.metaKey && e.key === "k") {
      e.preventDefault();
      showPalette = !showPalette;
      return;
    }

    // Don't intercept shortcuts when palette is open
    if (showPalette) return;

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

    // Check if first-time CLI setup is needed
    const needsSetup = await checkSetupNeeded();
    if (needsSetup) {
      log("First-time setup needed");
      showSetupPrompt = true;
    }

    // Load projects (global, independent of session restore)
    const { loadProjects } = await import("$lib/stores/projects");
    await loadProjects();

    if (loadedSettings.restoreSessionsOnLaunch) {
      const sessions = await listSessions();
      log(`Restoring ${sessions.length} session(s)`);
      for (const s of sessions) {
        addSession(s);
        const shellPanes = initSessionPanes(s.id);
        log(`  Session '${s.name}' (${s.id}): restored ${shellPanes.length} shell pane(s)`);
        // Spawn fresh shell PTYs for restored layout
        for (const pane of shellPanes) {
          spawnShell(pane.ptyId, pane.workingDir ?? s.worktreePath).catch((e) => {
            logError(`Failed to spawn shell for pane ${pane.id}`, e);
          });
        }
      }
    }

    // Listen for commands from roux-cli via socket server
    await onRouxCommand((cmd: RouxCommand) => {
      log(`roux-command: ${JSON.stringify(cmd)}`);
      switch (cmd.action) {
        case "split": {
          const sessionId = cmd.sessionId;
          if (!sessionId) break;
          const paneId = crypto.randomUUID();
          const ptyId = crypto.randomUUID();
          const session = $sessionState.sessions.find((s) => s.id === sessionId);
          if (!session) break;
          spawnShell(ptyId, session.worktreePath).then(() => {
            const direction = (cmd.direction === "vertical" ? "vertical" : "horizontal") as "horizontal" | "vertical";
            addSplit(sessionId, direction, { id: paneId, type: "shell", ptyId });
          }).catch((e) => logError("Failed to spawn shell for socket split", e));
          break;
        }
        case "session-created": {
          // Reload sessions to pick up the newly created one
          listSessions().then((sessions) => {
            const newSession = sessions.find((s) => s.id === cmd.sessionId);
            if (newSession) {
              addSession(newSession);
              initSessionPanes(newSession.id);
            }
          });
          break;
        }
        case "shell-opened": {
          const sessionId = cmd.sessionId;
          if (!sessionId || !cmd.paneId || !cmd.ptyId) break;
          addSplit(sessionId, "horizontal", { id: cmd.paneId, type: "shell", ptyId: cmd.ptyId });
          break;
        }
        case "command-opened": {
          const sessionId = cmd.sessionId;
          if (!sessionId || !cmd.paneId || !cmd.ptyId) break;
          addSplit(sessionId, "horizontal", {
            id: cmd.paneId,
            type: "command",
            ptyId: cmd.ptyId,
            command: cmd.command,
            workingDir: cmd.workingDir,
          });
          break;
        }
        case "focus": {
          if (cmd.sessionId) {
            setActiveSession(cmd.sessionId);
          }
          if (cmd.paneId) {
            focusedPaneId.set(cmd.paneId);
          }
          break;
        }
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
  ondone={() => (showSetupPrompt = false)}
/>
