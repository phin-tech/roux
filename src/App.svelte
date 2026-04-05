<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { addSession, sessionState, updateSessionStatus, updateSessionPermission } from "$lib/stores/sessions";
  import { initSessionPanes, addSplit, focusedPaneId } from "$lib/stores/panes";
  import { listSessions, onRouxStatusUpdate, spawnShell } from "$lib/tauri";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);

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

  function handleKeyDown(e: KeyboardEvent) {
    if (e.metaKey && e.key === "d" && !e.shiftKey) {
      e.preventDefault();
      splitCurrentSession("horizontal");
    }
    if (e.metaKey && (e.key === "D" || (e.key === "d" && e.shiftKey))) {
      e.preventDefault();
      splitCurrentSession("vertical");
    }
  }

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeyDown);
  });

  onMount(async () => {
    window.addEventListener("keydown", handleKeyDown);

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
>
  {#snippet settingsPanel()}
    <SettingsPanel visible={showSettings} onclose={() => (showSettings = false)} />
  {/snippet}
</Layout>

<NewSessionDialog
  visible={showNewSessionDialog}
  onclose={() => (showNewSessionDialog = false)}
/>
