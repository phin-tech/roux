<script lang="ts">
  import { onMount } from "svelte";
  import { listClaudeSessions } from "$lib/tauri";
  import type { ClaudeSession } from "$lib/types";

  interface Props {
    cwd: string;
    onResume: (claudeSessionId: string) => void;
    onNew: () => void;
  }

  let { cwd, onResume, onNew }: Props = $props();

  let sessions = $state<ClaudeSession[]>([]);
  let loading = $state(true);
  let filter = $state("");

  const filtered = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => s.summary.toLowerCase().includes(q) || s.sessionId.includes(q));
  });

  function timeAgo(ts: number): string {
    const diff = Math.floor(Date.now() / 1000 - ts);
    if (diff < 60) return "just now";
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  onMount(async () => {
    try {
      sessions = await listClaudeSessions(cwd);
    } catch {
      sessions = [];
    }
    loading = false;
  });
</script>

<div class="flex h-full w-full flex-col items-center justify-center p-8">
  <div class="w-full max-w-md space-y-4">
    <div class="text-center space-y-1">
      <div class="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl border border-border-subtle bg-bg-surface/80 text-accent shadow-[0_12px_32px_rgba(2,6,23,0.4)]">
        <span class="text-xl">&#9095;</span>
      </div>
      <p class="pt-3 text-sm font-semibold tracking-tight text-text-primary">Resume or start new</p>
      <p class="text-xs text-text-secondary">{cwd.split("/").pop()}</p>
    </div>

    <button
      class="flex w-full items-center justify-center gap-2 rounded-xl border border-accent-dim/20 bg-accent-dim/15 py-2.5 text-sm text-accent cursor-pointer transition-all hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
      onclick={onNew}
    >
      <span class="text-base">+</span> New Session
    </button>

    {#if loading}
      <p class="text-center text-xs text-text-muted">Loading sessions...</p>
    {:else if sessions.length > 0}
      {#if sessions.length > 5}
        <input
          class="w-full rounded-lg border border-border-subtle bg-bg-surface/80 px-2.5 py-1.5 text-[11px] text-text-primary placeholder:text-text-muted outline-none focus:border-border"
          placeholder="Filter sessions..."
          bind:value={filter}
        />
      {/if}

      <div class="app-scrollbar max-h-64 space-y-1.5 overflow-y-auto">
        {#each filtered as cs (cs.sessionId)}
          <button
            class="group flex w-full items-start gap-3 rounded-xl border border-border-subtle bg-bg-surface/70 px-3 py-2.5 text-left cursor-pointer transition-colors hover:border-border hover:bg-bg-surface focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
            onclick={() => onResume(cs.sessionId)}
          >
            <div class="min-w-0 flex-1">
              <div class="truncate text-[12px] font-medium text-text-primary">
                {cs.summary || "Empty session"}
              </div>
              <div class="mt-0.5 flex items-center gap-2">
                <span class="font-mono text-[10px] text-text-secondary">{cs.sessionId.slice(0, 8)}</span>
                <span class="text-[10px] text-text-muted">{timeAgo(cs.modifiedAt)}</span>
              </div>
            </div>
            <span class="shrink-0 pt-1 text-[10px] text-text-secondary opacity-0 transition-opacity group-hover:opacity-100">&#8594;</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>
