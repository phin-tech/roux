<script lang="ts">
  import {
    archivedSessionsState,
    loadArchivedSessions,
    restoreArchivedSession,
    removeArchivedSessionForever,
    cleanArchivedWorktree,
  } from "$lib/stores/archivedSessions";
  import { sessionDisplayName } from "$lib/stores/sessions";
  import { openNotesForSession } from "$lib/stores/ui";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Session } from "$lib/types";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  interface Props {
    collapsed?: boolean;
    oncollapsedchange?: (collapsed: boolean) => void;
    onresizestart?: (event: MouseEvent) => void;
    resizing?: boolean;
  }

  let {
    collapsed = true,
    oncollapsedchange,
    onresizestart,
    resizing = false,
  }: Props = $props();
  let loadError = $state<string | null>(null);

  $effect(() => {
    if (collapsed) return;
    loadError = null;
    loadArchivedSessions().catch((e) => {
      loadError = String(e);
    });
  });

  const archivedList = $derived($archivedSessionsState.sessions);
  const worktreeExists = $derived($archivedSessionsState.worktreeExists);

  function formatRelative(ts: number): string {
    const secs = Math.floor(Date.now() / 1000 - ts);
    if (secs < 10) return "just now";
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
    return `${Math.floor(secs / 86400)}d ago`;
  }

  async function handleRestore(e: Event, s: Session) {
    e.stopPropagation();
    try {
      await restoreArchivedSession(s.id);
    } catch (err) {
      loadError = `Failed to restore: ${err}`;
    }
  }

  async function handleDeleteForever(e: Event, s: Session) {
    e.stopPropagation();
    const wtExists = worktreeExists.get(s.id) ?? true;
    const wtNote =
      s.isWorktree && wtExists
        ? `\n\nThe worktree at ${s.worktreePath} will remain on disk — delete it manually if you don't need it.`
        : "";
    const confirmed = window.confirm(
      `Permanently delete "${sessionDisplayName(s)}"? This cannot be undone.${wtNote}`,
    );
    if (!confirmed) return;
    try {
      await removeArchivedSessionForever(s.id);
    } catch (err) {
      loadError = `Failed to delete: ${err}`;
    }
  }

  function handleViewNotes(e: Event, s: Session) {
    e.stopPropagation();
    openNotesForSession(s.id);
  }

  async function handleShowWorktree(e: Event, s: Session) {
    e.stopPropagation();
    try {
      const url = s.worktreePath.startsWith("file://")
        ? s.worktreePath
        : `file://${s.worktreePath}`;
      await openUrl(url);
    } catch (err) {
      loadError = `Failed to open worktree: ${err}`;
    }
  }

  async function handleCleanWorktree(e: Event, s: Session) {
    e.stopPropagation();
    const confirmed = window.confirm(
      `Remove the worktree at ${s.worktreePath}?\n\n` +
        `The session history entry stays. Restore becomes unavailable afterward.`,
    );
    if (!confirmed) return;
    try {
      await cleanArchivedWorktree(s.id, s.repoRoot, s.worktreePath);
    } catch (err) {
      loadError = `Failed to remove worktree: ${err}`;
    }
  }

  function toggleCollapsed() {
    collapsed = !collapsed;
    oncollapsedchange?.(collapsed);
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  {#if collapsed}
    <div class="border-t border-hairline pt-2"></div>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="group flex h-2 shrink-0 cursor-row-resize items-center"
      onmousedown={onresizestart}
      title="Resize archived sessions"
    >
      <div class="h-px w-full transition-colors duration-150 {resizing ? 'bg-white/30' : 'bg-white/15 group-hover:bg-white/35'}"></div>
    </div>
  {/if}
  <button
    class="flex w-full shrink-0 cursor-pointer items-center gap-1.5 bg-transparent px-2 py-1.5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
    onclick={toggleCollapsed}
    title="Archived sessions"
  >
    <ChevronRight size={12} class="text-text-secondary transition-transform duration-150 {collapsed ? '' : 'rotate-90'}" />
    <span class="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">Archived</span>
    <span class="text-[10px] text-text-muted/60">· {archivedList.length}</span>
  </button>

  {#if !collapsed}
    <div class="app-scrollbar min-h-0 flex-1 overflow-y-auto px-1 pb-2">
      {#if loadError}
        <div class="mb-2 rounded border border-red/40 bg-red/10 px-2 py-1 text-[11px] text-red">
          {loadError}
        </div>
      {/if}
      {#if archivedList.length === 0}
        <div class="px-2 py-1 text-[11px] text-text-muted">
          No closed sessions yet.
        </div>
      {:else}
        {#each archivedList as s (s.id)}
          {@const wtExists = worktreeExists.get(s.id) ?? true}
          <div class="mb-1 flex flex-col gap-1 rounded border border-transparent px-2 py-1.5 text-left text-sm hover:border-border-subtle hover:bg-bg-hover">
            <div class="flex items-center gap-2">
              <span class="inline-block h-2 w-2 shrink-0 rounded-full bg-text-muted/40"></span>
              <span class="min-w-0 flex-1 truncate text-[12px] text-text-primary">{sessionDisplayName(s)}</span>
              <span class="shrink-0 text-[10px] text-text-muted">
                {s.endedAt ? formatRelative(s.endedAt) : "closed"}
              </span>
            </div>
            <div class="ml-4 truncate text-[10px] text-text-muted">{s.branch}</div>
            {#if s.isWorktree}
              <div class="ml-4 flex items-center gap-1 text-[10px]">
                <span
                  class="shrink-0 rounded px-1 py-0.5 {wtExists ? 'bg-green/15 text-green' : 'bg-text-muted/15 text-text-muted'}"
                  title={wtExists ? "Worktree still exists on disk" : "Worktree has been removed"}
                >{wtExists ? "on disk" : "gone"}</span>
              </div>
            {/if}
            <div class="ml-4 mt-0.5 flex flex-wrap gap-1">
              <button
                class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-secondary hover:border-accent hover:bg-bg-hover hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-border-subtle disabled:hover:bg-transparent disabled:hover:text-text-secondary"
                disabled={!wtExists}
                title={wtExists ? "Restore to active sessions" : "Worktree removed — restore unavailable"}
                onclick={(e) => handleRestore(e, s)}
              >Restore</button>
              <button
                class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-secondary hover:border-accent hover:bg-bg-hover hover:text-text-primary"
                onclick={(e) => handleViewNotes(e, s)}
              >Notes</button>
              {#if s.isWorktree && wtExists}
                <button
                  class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-secondary hover:border-accent hover:bg-bg-hover hover:text-text-primary"
                  title="Reveal the worktree in your file manager"
                  onclick={(e) => handleShowWorktree(e, s)}
                >Reveal</button>
                <button
                  class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-secondary hover:border-amber hover:bg-bg-hover hover:text-amber"
                  title="Remove the worktree on disk; keep the history entry"
                  onclick={(e) => handleCleanWorktree(e, s)}
                >Clean</button>
              {/if}
              <button
                class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-secondary hover:border-red hover:bg-bg-hover hover:text-red"
                title="Delete this session history entry forever"
                onclick={(e) => handleDeleteForever(e, s)}
              >Delete</button>
            </div>
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>
