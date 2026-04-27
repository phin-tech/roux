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
  import { openPathInFinder } from "$lib/tauri";
  import type { Session } from "$lib/types";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Trash from "@lucide/svelte/icons/trash";
  import FileX from "@lucide/svelte/icons/file-x";
  import X from "@lucide/svelte/icons/x";

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
  let actionError = $state<{ sessionId: string | null; message: string } | null>(null);
  let menuOpenFor = $state<string | null>(null);

  $effect(() => {
    if (collapsed) return;
    loadError = null;
    loadArchivedSessions().catch((e) => {
      loadError = String(e);
    });
  });

  const archivedList = $derived($archivedSessionsState.sessions);
  const worktreeExists = $derived($archivedSessionsState.worktreeExists);

  $effect(() => {
    if (menuOpenFor == null) return;
    const openSessionId = menuOpenFor;
    const onPointerDown = (ev: PointerEvent) => {
      const target = ev.target;
      if (!(target instanceof Element)) return;
      const row = target.closest<HTMLElement>("[data-archived-menu-root]");
      if (!row || row.dataset.archivedMenuRoot !== openSessionId) {
        menuOpenFor = null;
      }
    };
    const onKeydown = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") menuOpenFor = null;
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeydown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeydown);
    };
  });

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
    actionError = null;
    menuOpenFor = null;
    try {
      await restoreArchivedSession(s.id);
    } catch (err) {
      actionError = { sessionId: s.id, message: `Failed to restore: ${err}` };
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
    actionError = null;
    menuOpenFor = null;
    try {
      await removeArchivedSessionForever(s.id);
    } catch (err) {
      actionError = { sessionId: s.id, message: `Failed to delete history: ${err}` };
    }
  }

  function handleViewNotes(e: Event, s: Session) {
    e.stopPropagation();
    menuOpenFor = null;
    openNotesForSession(s.id);
  }

  async function handleShowWorktree(e: Event, s: Session) {
    e.stopPropagation();
    actionError = null;
    menuOpenFor = null;
    try {
      await openPathInFinder(s.worktreePath);
    } catch (err) {
      actionError = { sessionId: s.id, message: `Failed to reveal worktree: ${err}` };
    }
  }

  async function handleCleanWorktree(e: Event, s: Session) {
    e.stopPropagation();
    const confirmed = window.confirm(
      `Remove the worktree at ${s.worktreePath}?\n\n` +
        `The session history entry stays. Restore becomes unavailable afterward.`,
    );
    if (!confirmed) return;
    actionError = null;
    menuOpenFor = null;
    try {
      await cleanArchivedWorktree(s.id, s.repoRoot, s.worktreePath);
    } catch (err) {
      actionError = { sessionId: s.id, message: `Failed to remove worktree: ${err}` };
    }
  }

  function toggleMenu(e: Event, sessionId: string) {
    e.stopPropagation();
    menuOpenFor = menuOpenFor === sessionId ? null : sessionId;
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
        <div class="mb-1 flex items-center gap-2 border border-red/30 bg-red/10 px-2 py-1 text-[11px] text-red">
          <span class="min-w-0 flex-1 truncate">{loadError}</span>
          <button
            type="button"
            class="flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center text-red/80 hover:text-red focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/50"
            aria-label="Dismiss archived sessions error"
            title="Dismiss"
            onclick={() => (loadError = null)}
          >
            <X size={11} />
          </button>
        </div>
      {/if}
      {#if archivedList.length === 0}
        <div class="px-2 py-1 text-[11px] text-text-muted">
          No closed sessions yet.
        </div>
      {:else}
        {#each archivedList as s (s.id)}
          {@const wtExists = worktreeExists.get(s.id) ?? true}
          <div
            class="group relative mb-1 border border-transparent px-2 py-1.5 text-left text-sm transition-colors duration-150 hover:border-border-subtle hover:bg-bg-active/40 focus-within:border-border-subtle focus-within:bg-bg-active/40"
            data-testid="archived-session-row"
            data-archived-menu-root={s.id}
          >
            <div class="flex min-h-6 items-center gap-2">
              <span class="inline-block h-2 w-2 shrink-0 rounded-full bg-text-muted/40"></span>
              <span class="min-w-0 flex-1 truncate text-[12px] font-medium text-text-primary">{sessionDisplayName(s)}</span>
              <span class="shrink-0 whitespace-nowrap text-[10px] text-text-muted">
                {s.endedAt ? formatRelative(s.endedAt) : "closed"}
              </span>
              <button
                type="button"
                class="inline-flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded border px-2 text-[10px] font-medium whitespace-nowrap transition-colors duration-150
                  {wtExists
                    ? 'border-accent-dim/50 bg-accent-dim/15 text-accent hover:bg-accent-dim/30'
                    : 'border-border-subtle bg-transparent text-text-muted opacity-60'}
                  disabled:cursor-not-allowed"
                disabled={!wtExists}
                title={wtExists
                  ? "Move this session back to Active sessions"
                  : "Cannot restore because the worktree is no longer on disk"}
                aria-label={wtExists
                  ? `Restore ${sessionDisplayName(s)}`
                  : `Cannot restore ${sessionDisplayName(s)} because the worktree is no longer on disk`}
                onclick={(e) => handleRestore(e, s)}
              >
                <RotateCcw size={11} />
                <span>Restore</span>
              </button>
              <button
                type="button"
                class="inline-flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded border border-border-subtle bg-bg-elevated text-text-secondary transition-colors duration-150 hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
                title="More actions"
                aria-label="More actions"
                data-testid="archived-session-menu"
                onclick={(e) => toggleMenu(e, s.id)}
              >
                <MoreHorizontal size={13} />
              </button>
            </div>
            <div class="ml-4 mt-0.5 flex min-h-5 items-center gap-1.5 overflow-hidden text-[10px] text-text-muted">
              <span class="min-w-0 truncate" title={`${s.branch} · ${s.worktreePath}`}>
                {s.branch}
                {#if s.worktreePath}
                  <span class="text-text-muted/60">·</span>
                  {s.worktreePath}
                {/if}
              </span>
              {#if s.isWorktree}
                <span
                  class="shrink-0 rounded px-1 py-0.5 {wtExists ? 'bg-green/15 text-green' : 'bg-text-muted/15 text-text-muted'}"
                  title={wtExists ? "Worktree still exists on disk" : "Worktree has been removed"}
                >{wtExists ? "on disk" : "gone"}</span>
              {/if}
            </div>
            {#if actionError?.sessionId === s.id}
              <div class="ml-4 mt-1 flex items-center gap-2 border border-red/30 bg-red/10 px-2 py-1 text-[10px] text-red">
                <span class="min-w-0 flex-1 truncate">{actionError.message}</span>
                <button
                  type="button"
                  class="flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center text-red/80 hover:text-red focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/50"
                  aria-label="Dismiss archived session action error"
                  title="Dismiss"
                  onclick={() => (actionError = null)}
                >
                  <X size={11} />
                </button>
              </div>
            {/if}
            {#if menuOpenFor === s.id}
              <div
                class="absolute right-2 top-8 z-10 flex min-w-40 flex-col rounded border border-border bg-bg-elevated p-1 shadow-lg"
                data-testid="archived-session-menu-content"
              >
                <button
                  type="button"
                  class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-bg-hover disabled:opacity-40"
                  title="Open notes for this archived session"
                  aria-label="Open notes for this archived session"
                  onclick={(e) => handleViewNotes(e, s)}
                >
                  <StickyNote size={12} />
                  <span>Notes</span>
                </button>
                {#if s.isWorktree}
                  <button
                    type="button"
                    class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-bg-hover disabled:opacity-40"
                    disabled={!wtExists}
                    title="Show this worktree folder in your file manager"
                    aria-label="Show this worktree folder in your file manager"
                    onclick={(e) => handleShowWorktree(e, s)}
                  >
                    <FolderOpen size={12} />
                    <span>Reveal</span>
                  </button>
                  <button
                    type="button"
                    class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-amber/20 enabled:hover:text-amber disabled:opacity-40"
                    disabled={!wtExists}
                    title="Delete the worktree folder but keep this history entry"
                    aria-label="Delete the worktree folder but keep this history entry"
                    onclick={(e) => handleCleanWorktree(e, s)}
                  >
                    <Trash size={12} />
                    <span>Remove worktree</span>
                  </button>
                {/if}
                <button
                  type="button"
                  class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-red/20 enabled:hover:text-red"
                  title="Permanently delete this archived session entry"
                  aria-label="Permanently delete this archived session entry"
                  onclick={(e) => handleDeleteForever(e, s)}
                >
                  <FileX size={12} />
                  <span>Delete history</span>
                </button>
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>
