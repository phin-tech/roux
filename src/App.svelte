<script lang="ts">
  import { onMount } from "svelte";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { addSession, sessionState, updateSessionStatus } from "$lib/stores/sessions";
  import { listSessions, onRouxStatusUpdate } from "$lib/tauri";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);

  onMount(async () => {
    const loadedSettings = await initSettings();
    // Only restore sessions if setting is enabled
    if (loadedSettings.restoreSessionsOnLaunch) {
      const sessions = await listSessions();
      for (const s of sessions) {
        addSession(s);
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
        updateSessionStatus(
          match.id,
          update.status as any,
          null,
          null
        );
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
