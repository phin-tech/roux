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

<div class="h-8 bg-bg-base border-t border-border-subtle flex items-center px-4 gap-4 font-mono text-[11px] text-text-secondary">
  {#if $activeSession}
    <div class="flex items-center gap-1.5">
      <div class="w-1.5 h-1.5 rounded-full {statusDotClass[$activeSession.status] ?? 'bg-gray'}"></div>
      <span>{$activeSession.name}</span>
    </div>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span class="text-accent">&#9095; {$activeSession.branch}</span>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span>{$activeSession.model ?? "—"}</span>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span class="text-green">
      {$activeSession.cost != null ? `$${$activeSession.cost.toFixed(2)}` : "—"}
    </span>
  {:else}
    <span class="text-text-muted">No active session</span>
  {/if}
</div>
