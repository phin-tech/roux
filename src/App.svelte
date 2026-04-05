<script lang="ts">
  import { onMount } from "svelte";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { addSession } from "$lib/stores/sessions";
  import { listSessions } from "$lib/tauri";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);

  onMount(async () => {
    await initSettings();
    const sessions = await listSessions();
    for (const s of sessions) {
      addSession(s);
    }
  });
</script>

<Layout
  onNewSession={() => (showNewSessionDialog = true)}
  onOpenSettings={() => (showSettings = !showSettings)}
/>

<NewSessionDialog
  visible={showNewSessionDialog}
  onclose={() => (showNewSessionDialog = false)}
/>
