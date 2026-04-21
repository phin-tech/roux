<script lang="ts">
  import { sessionState, sessionDisplayName, setActiveSession } from "$lib/stores/sessions";
  import {
    archivedSessionsState,
    loadArchivedSessions,
    restoreArchivedSession,
    removeArchivedSessionForever,
    cleanArchivedWorktree,
  } from "$lib/stores/archivedSessions";
  import { openNotesForSession } from "$lib/stores/ui";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Session } from "$lib/types";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();

  let loadError = $state<string | null>(null);

  // Hydrate archived sessions when the pane opens. Safe to call repeatedly
  // — the store tracks a `loaded` flag and refresh is cheap.
  $effect(() => {
    if (!visible) return;
    loadError = null;
    loadArchivedSessions().catch((e) => {
      loadError = String(e);
    });
  });

  function formatRelative(ts: number): string {
    const secs = Math.floor(Date.now() / 1000 - ts);
    if (secs < 10) return "just now";
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
    return `${Math.floor(secs / 86400)}d ago`;
  }

  function statusLabel(s: Session): string {
    switch (s.status) {
      case "idle": return "idle";
      case "thinking": return "thinking";
      case "generating": return "generating";
      case "error": return "error";
      case "disconnected": return "disconnected";
      case "attention": return "needs attention";
      default: return s.status;
    }
  }

  function statusDotClass(s: Session): string {
    switch (s.status) {
      case "generating":
      case "thinking": return "bg-accent";
      case "error": return "bg-red";
      case "attention": return "bg-amber";
      case "disconnected": return "bg-text-muted/40";
      default: return "bg-green";
    }
  }

  function handleActiveRowClick(s: Session) {
    setActiveSession(s.id);
    onclose();
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
      await cleanArchivedWorktree(s.id, s.worktreePath);
    } catch (err) {
      loadError = `Failed to remove worktree: ${err}`;
    }
  }

  const activeList = $derived($sessionState.sessions);
  const archivedList = $derived($archivedSessionsState.sessions);
  const worktreeExists = $derived($archivedSessionsState.worktreeExists);
</script>

<div
  style="right: {visible ? '0.5rem' : '-420px'}; visibility: {visible ? 'visible' : 'hidden'};"
  class="absolute top-2 bottom-2 z-50 flex w-[400px] flex-col border border-hairline bg-bg-deep shadow-[-8px_8px_48px_rgba(2,6,23,0.55),0_0_0_1px_rgba(255,255,255,0.04)] transition-[right] duration-250"
>
  <div class="flex h-9 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3">
    <span class="text-sm font-semibold tracking-tight">Sessions</span>
    <button
      class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-base text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
      onclick={onclose}
    >&times;</button>
  </div>

  <div class="flex-1 overflow-y-auto p-2">
    {#if loadError}
      <div class="mb-2 rounded border border-red/40 bg-red/10 px-2 py-1 text-[11px] text-red">
        {loadError}
      </div>
    {/if}

    <div class="mb-3">
      <div class="mb-1 flex items-center gap-1 px-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">
        <span>Active</span>
        <span class="text-text-muted/60">· {activeList.length}</span>
      </div>
      {#if activeList.length === 0}
        <div class="px-2 py-1 text-[11px] text-text-muted">No active sessions.</div>
      {:else}
        {#each activeList as s (s.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="mb-1 flex cursor-pointer flex-col gap-0.5 rounded-lg border border-transparent bg-transparent px-2 py-1.5 text-left text-sm hover:border-border-subtle hover:bg-bg-hover"
            class:bg-bg-surface={s.id === $sessionState.activeSessionId}
            onclick={() => handleActiveRowClick(s)}
          >
            <div class="flex items-center gap-2">
              <span class="inline-block h-2 w-2 shrink-0 rounded-full {statusDotClass(s)}"></span>
              <span class="min-w-0 flex-1 truncate text-text-primary">{sessionDisplayName(s)}</span>
              <span class="shrink-0 text-[9px] uppercase tracking-wider text-text-muted/60">{statusLabel(s)}</span>
            </div>
            <div class="ml-4 truncate text-[11px] text-text-muted">{s.branch}</div>
          </div>
        {/each}
      {/if}
    </div>

    <div class="mb-3">
      <div class="mb-1 flex items-center gap-1 px-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">
        <span>History</span>
        <span class="text-text-muted/60">· {archivedList.length}</span>
      </div>
      {#if archivedList.length === 0}
        <div class="px-2 py-1 text-[11px] text-text-muted">
          No closed sessions yet. Closed sessions will appear here.
        </div>
      {:else}
        {#each archivedList as s (s.id)}
          {@const wtExists = worktreeExists.get(s.id) ?? true}
          <div class="mb-1 flex flex-col gap-1 rounded-lg border border-transparent px-2 py-1.5 text-left text-sm hover:border-border-subtle hover:bg-bg-hover">
            <div class="flex items-center gap-2">
              <span class="inline-block h-2 w-2 shrink-0 rounded-full bg-text-muted/40"></span>
              <span class="min-w-0 flex-1 truncate text-text-primary">{sessionDisplayName(s)}</span>
              <span class="shrink-0 text-[10px] text-text-muted">
                {s.endedAt ? `closed ${formatRelative(s.endedAt)}` : "closed"}
              </span>
            </div>
            <div class="ml-4 truncate text-[11px] text-text-muted">{s.branch}</div>
            {#if s.isWorktree}
              <div class="ml-4 flex items-center gap-1 text-[10px]">
                <span class="truncate text-text-muted">worktree: {s.worktreePath}</span>
                <span
                  class="shrink-0 rounded px-1 py-0.5 {wtExists ? 'bg-green/15 text-green' : 'bg-text-muted/15 text-text-muted'}"
                  title={wtExists ? "Worktree still exists on disk" : "Worktree has been removed"}
                >{wtExists ? "on disk" : "gone"}</span>
              </div>
            {/if}
            <div class="ml-4 mt-1 flex flex-wrap gap-1">
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
                >Show worktree</button>
                <button
                  class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-secondary hover:border-amber hover:bg-bg-hover hover:text-amber"
                  title="Remove the worktree on disk; keep the history entry"
                  onclick={(e) => handleCleanWorktree(e, s)}
                >Clean worktree</button>
              {/if}
              <button
                class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-muted hover:border-red hover:bg-bg-hover hover:text-red"
                onclick={(e) => handleDeleteForever(e, s)}
              >Delete forever</button>
            </div>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>
