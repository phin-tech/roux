<script lang="ts">
  import SessionTabs from "./SessionTabs.svelte";
  import SplitPane from "./SplitPane.svelte";
  import StatusBar from "./StatusBar.svelte";
  import { sessionState } from "$lib/stores/sessions";
  import { paneTrees } from "$lib/stores/panes";
  import { settings, updateSetting } from "$lib/stores/settings";
  import type { Snippet } from "svelte";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
    settingsPanel?: Snippet;
  }

  let { onNewSession, onOpenSettings, settingsPanel }: Props = $props();

  let dragging = $state(false);
  let sidebarWidth = $derived($settings.tabWidth);

  function onDragStart(e: MouseEvent) {
    dragging = true;
    e.preventDefault();
    const onMove = (ev: MouseEvent) => {
      const w = $settings.tabPosition === "left" ? ev.clientX : window.innerWidth - ev.clientX;
      const clamped = Math.max(180, Math.min(500, w));
      updateSetting("tabWidth", clamped);
    };
    const onUp = () => {
      dragging = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }
</script>

<div class="h-screen flex flex-col bg-bg-deep text-text-primary">
  <!-- Main area -->
  <div class="flex flex-1 min-h-0"
    class:flex-row={$settings.tabPosition === "left"}
    class:flex-row-reverse={$settings.tabPosition === "right"}
  >
    <!-- Sidebar -->
    <div style="width: {sidebarWidth}px" class="shrink-0">
      <SessionTabs {onNewSession} {onOpenSettings} />
    </div>

    <!-- Resize handle -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-1 cursor-col-resize bg-transparent hover:bg-accent-dim transition-colors shrink-0"
      class:bg-accent-dim={dragging}
      onmousedown={onDragStart}
    ></div>

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
          {@const tree = $paneTrees.get(session.id)}
          {#if tree}
            <div class="flex-1 min-h-0 flex" class:hidden={session.id !== $sessionState.activeSessionId}>
              <SplitPane node={tree} sessionId={session.id} sessionActive={session.id === $sessionState.activeSessionId} />
            </div>
          {/if}
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
