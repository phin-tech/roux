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
  class="flex h-8 items-center gap-3 bg-bg-base px-3 text-[12px] text-text-muted"
  class:border-t={position === "bottom"}
  class:border-b={position === "top"}
  class:border-border-subtle={true}
>
  {#if $activeSession}
    <div class="flex items-center gap-2">
      <div class="w-2.5 h-2.5 rounded-full {statusDotClass[$activeSession.status] ?? 'bg-gray'}"></div>
      <span class="text-[14px] font-semibold tracking-tight text-text-primary">{$activeSession.name}</span>
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
