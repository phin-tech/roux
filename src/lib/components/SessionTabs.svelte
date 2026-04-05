<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import { sessionState, setActiveSession, removeSession, renameSession } from "$lib/stores/sessions";
  import { killSession } from "$lib/tauri";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
  }

  let { onNewSession, onOpenSettings }: Props = $props();

  async function handleClose(id: string) {
    await killSession(id);
    removeSession(id);
  }
</script>

<div class="h-full flex flex-col bg-bg-base border-r border-border-subtle">
  <div class="px-4 pt-3.5 pb-2.5 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Sessions</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$sessionState.sessions.length}
    </span>
  </div>

  <div class="flex-1 overflow-y-auto px-2 scrollbar-thin">
    {#each $sessionState.sessions as session (session.id)}
      <SessionCard
        {session}
        active={session.id === $sessionState.activeSessionId}
        onselect={() => setActiveSession(session.id)}
        onclose={() => handleClose(session.id)}
        onrename={(newName) => renameSession(session.id, newName)}
      />
    {/each}
  </div>

  <div class="p-2 border-t border-border-subtle flex gap-1">
    <button
      class="flex-1 py-2 bg-accent-dim border-none rounded-md text-accent text-xs font-sans cursor-pointer flex items-center justify-center gap-1.5 transition-all duration-150 hover:bg-accent hover:text-bg-deep"
      onclick={onNewSession}
    >
      <span class="text-sm">+</span> New
    </button>
    <button
      class="py-2 px-3 bg-bg-elevated border border-border-subtle rounded-md text-text-secondary text-xs cursor-pointer flex items-center justify-center transition-all duration-150 hover:bg-bg-hover hover:text-text-primary"
      onclick={onOpenSettings}
    >
      &#9881;
    </button>
  </div>
</div>
