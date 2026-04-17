<script lang="ts">
  import SessionTabs from "./SessionTabs.svelte";
  import CollapsedSidebar from "./CollapsedSidebar.svelte";
  import SplitPane from "./SplitPane.svelte";
  import StatusBar from "./StatusBar.svelte";
  import { sessionState } from "$lib/stores/sessions";
  import { sessionLayouts } from "$lib/panes/layout";
  import { settings, updateSetting } from "$lib/stores/settings";
  import type { Snippet } from "svelte";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
    onToggleWatches: () => void;
    onToggleNotifications: () => void;
    settingsPanel?: Snippet;
  }

  let { onNewSession, onOpenSettings, onToggleWatches, onToggleNotifications, settingsPanel }: Props = $props();

  let dragging = $state(false);
  let sidebarWidth = $derived($settings.tabWidth);
  let statusBarPosition = $derived($settings.statusBarPosition ?? "bottom");

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

<div class="flex h-screen flex-col overflow-hidden bg-bg-deep text-text-primary">
  <div
    class="flex min-h-0 flex-1"
    class:flex-row={$settings.tabPosition === "left"}
    class:flex-row-reverse={$settings.tabPosition === "right"}
  >
    {#if $settings.sidebarCollapsed}
      <CollapsedSidebar />
    {:else}
      <div style="width: {sidebarWidth}px" class="shrink-0">
        <SessionTabs {onNewSession} {onOpenSettings} {onToggleWatches} {onToggleNotifications} />
      </div>

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="group relative flex min-h-0 w-1 shrink-0 cursor-col-resize self-stretch flex-col items-center"
        onmousedown={onDragStart}
      >
        <div
          class="min-h-0 max-w-[0.5px] min-w-[0.5px] flex-1 transition-all duration-150 {dragging ? 'bg-white/30' : 'bg-white/20 group-hover:bg-white/40'}"
        ></div>
      </div>
    {/if}

    <div class="relative flex min-w-0 flex-1 flex-col overflow-hidden bg-bg-deep">
      {#if statusBarPosition === "top"}
        <StatusBar position="top" />
      {/if}
      {#if $sessionState.sessions.length === 0}
        <div class="flex flex-1 flex-col items-center justify-center gap-4 text-center text-text-secondary">
          <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-border-subtle bg-bg-surface/80 text-accent shadow-[0_18px_40px_rgba(2,6,23,0.45)]">
            <span class="text-3xl">&#9095;</span>
          </div>
          <div class="space-y-1">
            <p class="text-base font-semibold tracking-tight text-text-primary">No active sessions</p>
            <p class="text-sm text-text-secondary">Start a new session to open a terminal workspace.</p>
          </div>
          <button
            type="button"
            onclick={onNewSession}
            class="rounded-lg border border-border-subtle bg-bg-surface/80 px-4 py-2 text-sm font-semibold text-text-primary shadow-[0_18px_40px_rgba(2,6,23,0.45)] transition-colors hover:border-accent hover:text-accent"
          >
            New Session
          </button>
        </div>
      {:else}
        {#each $sessionState.sessions as session (session.id)}
          {@const tree = $sessionLayouts.get(session.id)}
          {#if tree}
            <div
              class="flex min-h-0 flex-1 bg-bg-deep"
              class:hidden={session.id !== $sessionState.activeSessionId}
            >
              <SplitPane
                node={tree}
                sessionId={session.id}
                visible={session.id === $sessionState.activeSessionId}
              />
            </div>
          {/if}
        {/each}
      {/if}

      {#if settingsPanel}
        {@render settingsPanel()}
      {/if}

      {#if statusBarPosition === "bottom"}
        <StatusBar position="bottom" />
      {/if}
    </div>
  </div>
</div>
