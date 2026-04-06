<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SetupPrompt from "$lib/components/SetupPrompt.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import { initSettings, settings } from "$lib/stores/settings";
  import { addSession, sessionState, updateSessionStatus, updateSessionPermission } from "$lib/stores/sessions";
  import { initSessionPanes, hasSplitPanes } from "$lib/stores/panes";
  import { listSessions, checkSetupNeeded, onRouxStatusUpdate, spawnShell } from "$lib/tauri";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { registerCommands, registry } from "$lib/commands";
  import { closeFocusedPane } from "$lib/panes/actions";
  import { normalizeTheme, isLightTheme } from "$lib/themes";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);
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

    // Check if first-time CLI setup is needed
    const needsSetup = await checkSetupNeeded();
    if (needsSetup) {
      showSetupPrompt = true;
    }

    if (loadedSettings.restoreSessionsOnLaunch) {
      const sessions = await listSessions();
      for (const s of sessions) {
        addSession(s);
        const shellPanes = initSessionPanes(s.id);
        // Spawn fresh shell PTYs for restored layout
        for (const pane of shellPanes) {
          spawnShell(pane.ptyId, s.worktreePath).catch(() => {});
        }
      }
    }

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
