<script lang="ts">
  import SessionTabs from "./SessionTabs.svelte";
  import Terminal from "./Terminal.svelte";
  import StatusBar from "./StatusBar.svelte";
  import { sessionState } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import type { Snippet } from "svelte";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
    settingsPanel?: Snippet;
  }

  let { onNewSession, onOpenSettings, settingsPanel }: Props = $props();
</script>

<div class="h-screen flex flex-col bg-bg-deep text-text-primary">
  <!-- Main area -->
  <div class="flex flex-1 min-h-0"
    class:flex-row={$settings.tabPosition === "left"}
    class:flex-row-reverse={$settings.tabPosition === "right"}
  >
    <!-- Sidebar -->
    <div style="width: {$settings.tabWidth}px" class="shrink-0">
      <SessionTabs {onNewSession} {onOpenSettings} />
    </div>

    <!-- Resize handle -->
    <div class="w-1 cursor-col-resize bg-transparent hover:bg-accent-dim transition-colors shrink-0"></div>

    <!-- Terminal area -->
    <div class="flex-1 relative flex flex-col min-w-0">
      {#if $sessionState.sessions.length === 0}
        <div class="flex-1 flex flex-col items-center justify-center gap-4 text-text-muted">
          <span class="text-5xl opacity-30">&#9636;</span>
          <span class="text-sm">No sessions</span>
          <span class="text-xs font-mono opacity-60">Click "+ New" to create a session</span>
        </div>
      {:else}
        {#each $sessionState.sessions as session (session.id)}
          <Terminal
            sessionId={session.id}
            active={session.id === $sessionState.activeSessionId}
          />
        {/each}
      {/if}

      <!-- Settings panel slot -->
      {#if settingsPanel}
        {@render settingsPanel()}
      {/if}
    </div>
  </div>

  <StatusBar />
</div>
