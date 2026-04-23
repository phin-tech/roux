<script lang="ts">
  import ActivityRail from "./ActivityRail.svelte";
  import SidebarDock from "./SidebarDock.svelte";
  import SplitPane from "./SplitPane.svelte";
  import StatusBar from "./StatusBar.svelte";
  import { sessionState } from "$lib/stores/sessions";
  import { sessionLayouts } from "$lib/panes/layout";
  import { settings } from "$lib/stores/settings";
  import { sidebarLayout } from "$lib/stores/sidebarLayout";
  import type { Snippet } from "svelte";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
    onToggleWatches: () => void;
    onToggleNotifications: () => void;
    settingsPanel?: Snippet;
  }

  let { onNewSession, onOpenSettings, onToggleWatches, onToggleNotifications, settingsPanel }: Props = $props();

  let statusBarPosition = $derived($settings.statusBarPosition ?? "bottom");
  let railSide = $derived($sidebarLayout.railSide);
  let sidebarHidden = $derived($sidebarLayout.hidden);
</script>

{#snippet rail()}
  <div class="flex h-full w-[36px] shrink-0 flex-col border-hairline bg-bg-base/96 {railSide === 'left' ? 'border-r' : 'border-l'}">
    <ActivityRail />
  </div>
{/snippet}

{#snippet dock()}
  <SidebarDock
    {onNewSession}
    {onOpenSettings}
    {onToggleWatches}
    {onToggleNotifications}
  />
{/snippet}

<div class="flex h-screen flex-col overflow-hidden bg-bg-deep text-text-primary">
  <div class="flex min-h-0 flex-1 flex-row">
    {#if !sidebarHidden && railSide === "left"}
      {@render rail()}
      {@render dock()}
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

    {#if !sidebarHidden && railSide === "right"}
      {@render dock()}
      {@render rail()}
    {/if}
  </div>
</div>
