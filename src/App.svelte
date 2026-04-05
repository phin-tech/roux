<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { addSession, sessionState, updateSessionStatus, updateSessionPermission } from "$lib/stores/sessions";
  import { initSessionPanes, hasSplitPanes, focusedPaneId, removePane } from "$lib/stores/panes";
  import { listSessions, onRouxStatusUpdate } from "$lib/tauri";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { registerCommands, registry } from "$lib/commands";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);
  let showPalette = $state(false);

  function buildShortcutString(e: KeyboardEvent): string {
    const parts: string[] = [];
    if (e.metaKey) parts.push("cmd");
    if (e.shiftKey) parts.push("shift");
    if (e.altKey) parts.push("alt");
    if (e.ctrlKey) parts.push("ctrl");
    parts.push(e.key.toLowerCase());
    return parts.join("+");
  }

  /** Returns true if a pane was closed, false if there was nothing to close */
  function closeFocusedPane(): boolean {
    const state = get(sessionState);
    if (!state.activeSessionId) return false;
    const focused = get(focusedPaneId);
    if (!focused) return false;
    if (focused === state.activeSessionId + "-main") return false;
    removePane(state.activeSessionId, focused);
    return true;
  }

  function handleCloseRequested() {
    const state = get(sessionState);
    if (!state.activeSessionId) {
      getCurrentWindow().destroy();
      return;
    }
    if (hasSplitPanes(state.activeSessionId)) {
      const closed = closeFocusedPane();
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

      if (cmd.execute) cmd.execute();
    }
  }

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeyDown, true);
  });

  onMount(async () => {
    registerCommands();
    // Use capture phase so we intercept before xterm.js swallows the event
    window.addEventListener("keydown", handleKeyDown, true);

    // Listen for Tauri close-requested event (Cmd+W or red button)
    await listen("close-requested", () => handleCloseRequested());

    const loadedSettings = await initSettings();
    if (loadedSettings.restoreSessionsOnLaunch) {
      const sessions = await listSessions();
      for (const s of sessions) {
        addSession(s);
        initSessionPanes(s.id);
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
