<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import DocPanel from "$lib/components/DocPanel.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { addSession, sessionState, updateSessionStatus, updateSessionPermission } from "$lib/stores/sessions";
  import { initSessionPanes, addSplit, focusedPaneId, removePane, paneTrees, hasSplitPanes } from "$lib/stores/panes";
  import { listSessions, onRouxStatusUpdate, spawnShell } from "$lib/tauri";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);
  let showDocs = $state(false);

  async function splitCurrentSession(direction: "horizontal" | "vertical") {
    const state = get(sessionState);
    if (!state.activeSessionId) return;
    const session = state.sessions.find((s) => s.id === state.activeSessionId);
    if (!session) return;

    const paneId = crypto.randomUUID();
    const ptyId = crypto.randomUUID();
    await spawnShell(ptyId, session.worktreePath);
    addSplit(state.activeSessionId, direction, { id: paneId, type: "shell", ptyId });
  }

  /** Returns true if a pane was closed, false if there was nothing to close */
  function closeFocusedPane(): boolean {
    const state = get(sessionState);
    if (!state.activeSessionId) return false;
    const focused = get(focusedPaneId);
    if (!focused) return false;
    // Don't close the main claude pane
    if (focused === state.activeSessionId + "-main") return false;

    removePane(state.activeSessionId, focused);
    return true;
  }

  function handleCloseRequested() {
    const state = get(sessionState);
    if (!state.activeSessionId) {
      // No sessions — close the window
      getCurrentWindow().destroy();
      return;
    }

    // If there are split panes, close the focused one
    if (hasSplitPanes(state.activeSessionId)) {
      const closed = closeFocusedPane();
      if (closed) return;
    }

    // No split panes to close — close the window
    getCurrentWindow().destroy();
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.metaKey && e.key === "d" && !e.shiftKey) {
      e.preventDefault();
      splitCurrentSession("horizontal");
    }
    if (e.metaKey && (e.key === "D" || (e.key === "d" && e.shiftKey))) {
      e.preventDefault();
      splitCurrentSession("vertical");
    }
    if (e.metaKey && e.key === "b") {
      e.preventDefault();
      showDocs = !showDocs;
    }
    if (e.metaKey && e.key === "w") {
      e.preventDefault();
      closeFocusedPane();
    }
  }

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeyDown);
  });

  onMount(async () => {
    window.addEventListener("keydown", handleKeyDown);

    // Listen for Tauri close-requested event (Cmd+W or red button)
    await listen("close-requested", () => handleCloseRequested());

    const loadedSettings = await initSettings();
    // Only restore sessions if setting is enabled
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
      // Match by worktreePath (where claude is actually running)
      const match = sessions.find(
        (s) => s.worktreePath === update.cwd || s.repoRoot === update.cwd
      );
      if (match) {
        updateSessionStatus(match.id, update.status as any, null, null);
        if (update.status === "attention") {
          // Only update permission info if this event has tool details
          // (PermissionRequest has toolName; Notification may not — don't overwrite)
          if (update.toolName) {
            updateSessionPermission(match.id, {
              toolName: update.toolName,
              toolInput: update.toolInput ?? {},
              message: update.message ?? "",
            });
          } else if (update.message && !match.permissionInfo) {
            // Notification with message but no tool — use message as fallback
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
  onOpenDocs={() => (showDocs = !showDocs)}
>
  {#snippet settingsPanel()}
    <SettingsPanel visible={showSettings} onclose={() => (showSettings = false)} />
  {/snippet}
  {#snippet docsPanel()}
    <DocPanel visible={showDocs} onclose={() => (showDocs = false)} />
  {/snippet}
</Layout>

<NewSessionDialog
  visible={showNewSessionDialog}
  onclose={() => (showNewSessionDialog = false)}
/>
