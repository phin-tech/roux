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

<div class="flex h-screen flex-col bg-bg-deep text-text-primary">
  <div
    class="flex min-h-0 flex-1"
    class:flex-row={$settings.tabPosition === "left"}
    class:flex-row-reverse={$settings.tabPosition === "right"}
  >
    <div style="width: {sidebarWidth}px" class="shrink-0">
      <SessionTabs {onNewSession} {onOpenSettings} />
    </div>

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="group relative flex w-2 shrink-0 cursor-col-resize items-stretch justify-center"
      onmousedown={onDragStart}
    >
      <div
        class="my-3 w-px rounded-full transition-all duration-150 {dragging ? 'bg-border opacity-100' : 'bg-border-subtle opacity-0 group-hover:opacity-100'}"
      ></div>
    </div>

    <div class="relative flex min-w-0 flex-1 flex-col bg-bg-deep p-2">
      {#if $sessionState.sessions.length === 0}
        <div class="ui-panel flex flex-1 flex-col items-center justify-center gap-4 rounded-[1.25rem] text-center text-text-secondary">
          <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-border-subtle bg-bg-surface/80 text-accent shadow-[0_18px_40px_rgba(2,6,23,0.45)]">
            <span class="text-3xl">&#9095;</span>
          </div>
          <div class="space-y-1">
            <p class="text-base font-semibold tracking-tight text-text-primary">No active sessions</p>
            <p class="text-sm text-text-secondary">Start a new session to open a terminal workspace.</p>
          </div>
          <p class="text-[11px] font-medium uppercase tracking-[0.24em] text-text-muted">Click "New" in the sidebar</p>
        </div>
      {:else}
        {#each $sessionState.sessions as session (session.id)}
          {@const tree = $paneTrees.get(session.id)}
          {#if tree}
            <div
              class="flex min-h-0 flex-1 rounded-[1.25rem] bg-bg-deep"
              class:hidden={session.id !== $sessionState.activeSessionId}
            >
              <SplitPane
                node={tree}
                sessionId={session.id}
                sessionActive={session.id === $sessionState.activeSessionId}
              />
            </div>
          {/if}
        {/each}
      {/if}

      {#if settingsPanel}
        {@render settingsPanel()}
      {/if}
    </div>
  </div>

  <StatusBar />
</div>
