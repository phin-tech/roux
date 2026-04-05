<script lang="ts">
  import { activeSession } from "$lib/stores/sessions";

  const statusDotClass: Record<string, string> = {
    idle: "bg-green",
    thinking: "bg-amber animate-pulse",
    generating: "bg-blue",
    error: "bg-red",
    disconnected: "bg-gray",
    attention: "bg-amber animate-pulse",
  };
</script>

<div class="flex h-8 items-center gap-4 border-t border-white/6 bg-bg-base/95 px-4 text-[11px] text-text-secondary backdrop-blur-sm">
  {#if $activeSession}
    <div class="flex items-center gap-1.5">
      <div class="w-1.5 h-1.5 rounded-full {statusDotClass[$activeSession.status] ?? 'bg-gray'}"></div>
      <span class="font-medium tracking-tight text-text-primary">{$activeSession.name}</span>
    </div>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span class="font-mono text-accent">&#9095; {$activeSession.branch}</span>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span class="font-mono">{$activeSession.model ?? "--"}</span>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span class="font-mono text-green">
      {$activeSession.cost != null ? `$${$activeSession.cost.toFixed(2)}` : "--"}
    </span>
  {:else}
    <span class="text-text-muted">No active session</span>
  {/if}
</div>
