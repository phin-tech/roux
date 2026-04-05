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

<div class="flex h-7 items-center gap-3 border-t border-hairline bg-bg-base/95 px-4 text-[10px] text-text-muted backdrop-blur-sm">
  {#if $activeSession}
    <div class="flex items-center gap-1.5">
      <div class="w-1.5 h-1.5 rounded-full {statusDotClass[$activeSession.status] ?? 'bg-gray'}"></div>
      <span class="font-medium tracking-tight text-text-secondary">{$activeSession.name}</span>
    </div>
    <span class="opacity-40">&bull;</span>
    <span class="font-mono text-text-muted">&#9095; {$activeSession.branch}</span>
    <span class="opacity-40">&bull;</span>
    <span class="font-mono text-text-muted">{$activeSession.model ?? "--"}</span>
    <span class="opacity-40">&bull;</span>
    <span class="font-mono text-text-muted">
      {$activeSession.cost != null ? `$${$activeSession.cost.toFixed(2)}` : "--"}
    </span>
  {:else}
    <span>No active session</span>
  {/if}
</div>
