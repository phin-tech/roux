<script lang="ts">
  import { activeSession } from "$lib/stores/sessions";
  import type { StatusBarPosition } from "$lib/types";

  interface Props {
    position?: StatusBarPosition;
  }
  let { position = "bottom" }: Props = $props();

  const statusDotClass: Record<string, string> = {
    idle: "bg-green",
    thinking: "bg-amber animate-pulse",
    generating: "bg-blue",
    error: "bg-red",
    disconnected: "bg-gray",
    attention: "bg-amber animate-pulse",
  };
</script>

<div
  class="flex h-6 items-center gap-3 bg-bg-base px-3 text-[11px] text-text-muted"
  class:border-t={position === "bottom"}
  class:border-b={position === "top"}
  class:border-border-subtle={true}
>
  {#if $activeSession}
    <div class="flex items-center gap-1.5">
      <div class="w-1.5 h-1.5 rounded-full {statusDotClass[$activeSession.status] ?? 'bg-gray'}"></div>
      <span class="font-medium tracking-tight text-text-secondary">{$activeSession.name}</span>
    </div>
    {#if $activeSession.isGitRepo}
      <span class="text-text-secondary">&bull;</span>
      <span class="font-mono text-text-muted">&#9095; {$activeSession.branch}</span>
    {/if}
    <span class="text-text-secondary">&bull;</span>
    <span class="font-mono text-text-muted">{$activeSession.model ?? "--"}</span>
    <span class="text-text-secondary">&bull;</span>
    <span class="font-mono text-text-muted">
      {$activeSession.cost != null ? `$${$activeSession.cost.toFixed(2)}` : "--"}
    </span>
  {:else}
    <span>No active session</span>
  {/if}
</div>
