<script lang="ts">
  import {
    archivedSessionsState,
    loadArchivedSessions,
    restoreArchivedSession,
    removeArchivedSessionForever,
    cleanArchivedWorktree,
    bulkRestoreArchivedSessions,
    bulkRemoveArchivedWorktrees,
    bulkDeleteArchivedSessionsForever,
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
  import Search from "@lucide/svelte/icons/search";
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
  let bulkError = $state<string | null>(null);
  let menuOpenFor = $state<string | null>(null);
  let headerMenuOpen = $state(false);
  let filterText = $state("");
  let selected = $state(new Set<string>());

  $effect(() => {
    if (collapsed) return;
    loadError = null;
    loadArchivedSessions().catch((e) => {
      loadError = String(e);
    });
  });

  const archivedList = $derived($archivedSessionsState.sessions);
  const worktreeExists = $derived($archivedSessionsState.worktreeExists);

  const filteredList = $derived.by(() => {
    const q = filterText.trim().toLowerCase();
    if (!q) return archivedList;
    return archivedList.filter((s) => {
      const name = sessionDisplayName(s).toLowerCase();
      const branch = (s.branch ?? "").toLowerCase();
      const path = (s.worktreePath ?? "").toLowerCase();
      return name.includes(q) || branch.includes(q) || path.includes(q);
    });
  });

  // Drop selections that no longer match the filter or no longer exist.
  // Without this, the bulk toolbar can claim "3 selected" while showing 1 row.
  $effect(() => {
    const visibleIds = new Set(filteredList.map((s) => s.id));
    let changed = false;
    const next = new Set<string>();
    for (const id of selected) {
      if (visibleIds.has(id)) next.add(id);
      else changed = true;
    }
    if (changed) selected = next;
  });

  const allVisibleSelected = $derived(
    filteredList.length > 0 && filteredList.every((s) => selected.has(s.id)),
  );
  const someVisibleSelected = $derived(
    filteredList.some((s) => selected.has(s.id)) && !allVisibleSelected,
  );
  const selectedSessions = $derived(
    filteredList.filter((s) => selected.has(s.id)),
  );
  const hasSelection = $derived(selected.size > 0);
  const selectableWorktreeEntries = $derived(
    selectedSessions
      .filter((s) => s.isWorktree && (worktreeExists.get(s.id) ?? false))
      .map((s) => ({ id: s.id, repoRoot: s.repoRoot, worktreePath: s.worktreePath })),
  );
  const restorableSelected = $derived(
    selectedSessions.filter((s) => worktreeExists.get(s.id) ?? false),
  );
  const archivedWithWorktreeOnDisk = $derived(
    archivedList.filter((s) => s.isWorktree && (worktreeExists.get(s.id) ?? false)),
  );

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

  $effect(() => {
    if (!headerMenuOpen) return;
    const onPointerDown = (ev: PointerEvent) => {
      const target = ev.target;
      if (!(target instanceof Element)) return;
      if (!target.closest("[data-archived-header-menu]")) {
        headerMenuOpen = false;
      }
    };
    const onKeydown = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") headerMenuOpen = false;
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

  function toggleSelection(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  function toggleAllVisible() {
    if (allVisibleSelected) {
      const next = new Set(selected);
      for (const s of filteredList) next.delete(s.id);
      selected = next;
    } else {
      const next = new Set(selected);
      for (const s of filteredList) next.add(s.id);
      selected = next;
    }
  }

  function clearSelection() {
    selected = new Set();
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

  function describeBulkResult(verb: string, succeeded: number, failures: { id: string; error: string }[]): string | null {
    if (failures.length === 0) return null;
    const sample = failures[0];
    if (failures.length === 1 && succeeded === 0) {
      return `Failed to ${verb}: ${sample.error}`;
    }
    return `${verb}: ${succeeded} succeeded, ${failures.length} failed (e.g. ${sample.error})`;
  }

  async function handleBulkRestore() {
    if (restorableSelected.length === 0) return;
    bulkError = null;
    const ids = restorableSelected.map((s) => s.id);
    try {
      const result = await bulkRestoreArchivedSessions(ids);
      bulkError = describeBulkResult("restore", result.succeeded.length, result.failures);
      const remaining = new Set(selected);
      for (const id of result.succeeded) remaining.delete(id);
      selected = remaining;
    } catch (err) {
      bulkError = `Failed to restore: ${err}`;
    }
  }

  async function handleBulkRemoveWorktrees() {
    if (selectableWorktreeEntries.length === 0) return;
    const confirmed = window.confirm(
      `Remove ${selectableWorktreeEntries.length} worktree${selectableWorktreeEntries.length === 1 ? "" : "s"} on disk?\n\n` +
        `History entries stay. Restore becomes unavailable for each one afterward.`,
    );
    if (!confirmed) return;
    bulkError = null;
    try {
      const result = await bulkRemoveArchivedWorktrees(selectableWorktreeEntries);
      bulkError = describeBulkResult(
        "remove worktrees",
        result.succeeded.length,
        result.failures,
      );
    } catch (err) {
      bulkError = `Failed to remove worktrees: ${err}`;
    }
  }

  async function handleBulkDelete() {
    if (selected.size === 0) return;
    const ids = Array.from(selected);
    const confirmed = window.confirm(
      `Permanently delete ${ids.length} archived session${ids.length === 1 ? "" : "s"}? This cannot be undone.`,
    );
    if (!confirmed) return;
    bulkError = null;
    try {
      const result = await bulkDeleteArchivedSessionsForever(ids);
      bulkError = describeBulkResult(
        "delete history",
        result.succeeded.length,
        result.failures,
      );
      const remaining = new Set(selected);
      for (const id of result.succeeded) remaining.delete(id);
      selected = remaining;
    } catch (err) {
      bulkError = `Failed to delete history: ${err}`;
    }
  }

  async function handleClearAll() {
    headerMenuOpen = false;
    if (archivedList.length === 0) return;
    const confirmed = window.confirm(
      `Permanently delete all ${archivedList.length} archived session${archivedList.length === 1 ? "" : "s"}?\n\n` +
        `History entries are removed. Worktrees remain on disk — use "Remove all worktrees" first if you want them gone too. This cannot be undone.`,
    );
    if (!confirmed) return;
    bulkError = null;
    try {
      const result = await bulkDeleteArchivedSessionsForever(
        archivedList.map((s) => s.id),
      );
      bulkError = describeBulkResult(
        "clear archive",
        result.succeeded.length,
        result.failures,
      );
      selected = new Set();
    } catch (err) {
      bulkError = `Failed to clear archive: ${err}`;
    }
  }

  async function handleRemoveAllWorktrees() {
    headerMenuOpen = false;
    if (archivedWithWorktreeOnDisk.length === 0) return;
    const confirmed = window.confirm(
      `Remove ${archivedWithWorktreeOnDisk.length} worktree${archivedWithWorktreeOnDisk.length === 1 ? "" : "s"} on disk?\n\n` +
        `History entries stay. Restore becomes unavailable for each one afterward.`,
    );
    if (!confirmed) return;
    bulkError = null;
    try {
      const result = await bulkRemoveArchivedWorktrees(
        archivedWithWorktreeOnDisk.map((s) => ({
          id: s.id,
          repoRoot: s.repoRoot,
          worktreePath: s.worktreePath,
        })),
      );
      bulkError = describeBulkResult(
        "remove worktrees",
        result.succeeded.length,
        result.failures,
      );
    } catch (err) {
      bulkError = `Failed to remove worktrees: ${err}`;
    }
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
  <div class="relative flex w-full shrink-0 items-center" data-archived-header-menu>
    <button
      class="flex flex-1 cursor-pointer items-center gap-1.5 bg-transparent px-2 py-1.5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
      onclick={toggleCollapsed}
      title="Archived sessions"
    >
      <ChevronRight size={12} class="text-text-secondary transition-transform duration-150 {collapsed ? '' : 'rotate-90'}" />
      <span class="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">Archived</span>
      <span class="text-[10px] text-text-muted/60">· {archivedList.length}</span>
    </button>
    {#if !collapsed}
      <button
        type="button"
        class="mr-1 inline-flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded text-text-secondary transition-colors duration-150 hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-not-allowed disabled:opacity-40"
        title="More archived actions"
        aria-label="More archived actions"
        data-testid="archived-header-menu"
        disabled={archivedList.length === 0}
        onclick={() => (headerMenuOpen = !headerMenuOpen)}
      >
        <MoreHorizontal size={13} />
      </button>
      {#if headerMenuOpen}
        <div
          class="absolute right-1 top-7 z-20 flex min-w-44 flex-col rounded border border-border bg-bg-elevated p-1 shadow-lg"
          data-testid="archived-header-menu-content"
        >
          <button
            type="button"
            class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-amber/20 enabled:hover:text-amber disabled:opacity-40"
            disabled={archivedWithWorktreeOnDisk.length === 0}
            title={archivedWithWorktreeOnDisk.length === 0
              ? "No archived worktrees on disk"
              : `Delete ${archivedWithWorktreeOnDisk.length} worktree folder${archivedWithWorktreeOnDisk.length === 1 ? "" : "s"} but keep history`}
            onclick={handleRemoveAllWorktrees}
          >
            <Trash size={12} />
            <span>Remove all worktrees</span>
            {#if archivedWithWorktreeOnDisk.length > 0}
              <span class="ml-auto text-[10px] text-text-muted">{archivedWithWorktreeOnDisk.length}</span>
            {/if}
          </button>
          <button
            type="button"
            class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-red/20 enabled:hover:text-red disabled:opacity-40"
            disabled={archivedList.length === 0}
            title="Permanently delete every archived session entry"
            onclick={handleClearAll}
          >
            <FileX size={12} />
            <span>Clear all history</span>
            <span class="ml-auto text-[10px] text-text-muted">{archivedList.length}</span>
          </button>
        </div>
      {/if}
    {/if}
  </div>

  {#if !collapsed}
    <div class="px-2 pt-1 pb-1.5">
      <div class="relative">
        <Search size={11} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-text-muted" />
        <input
          type="text"
          class="w-full rounded border border-border bg-bg-deep py-1 pl-6 pr-6 text-[11px] text-text-primary placeholder:text-text-muted outline-none focus:border-accent-dim"
          placeholder="Filter archived…"
          bind:value={filterText}
          data-testid="archived-filter-input"
        />
        {#if filterText}
          <button
            type="button"
            class="absolute right-1 top-1/2 inline-flex h-4 w-4 -translate-y-1/2 cursor-pointer items-center justify-center rounded text-text-muted hover:bg-bg-hover hover:text-text-primary"
            aria-label="Clear filter"
            title="Clear filter"
            onclick={() => (filterText = "")}
          >
            <X size={11} />
          </button>
        {/if}
      </div>
    </div>

    {#if hasSelection}
      <div
        class="mx-2 mb-1 flex flex-wrap items-center gap-1 rounded border border-accent-dim/40 bg-accent-dim/10 px-2 py-1 text-[10px] text-text-secondary"
        data-testid="archived-bulk-toolbar"
      >
        <span class="text-text-primary">{selected.size} selected</span>
        <button
          type="button"
          class="ml-auto inline-flex h-5 cursor-pointer items-center gap-1 rounded border border-accent-dim/50 bg-accent-dim/15 px-1.5 text-[10px] text-accent transition-colors duration-150 hover:bg-accent-dim/30 disabled:cursor-not-allowed disabled:opacity-40"
          disabled={restorableSelected.length === 0}
          title={restorableSelected.length === 0
            ? "None of the selected sessions can be restored (worktrees missing)"
            : `Restore ${restorableSelected.length} session${restorableSelected.length === 1 ? "" : "s"}`}
          onclick={handleBulkRestore}
        >
          <RotateCcw size={10} />
          <span>Restore</span>
        </button>
        <button
          type="button"
          class="inline-flex h-5 cursor-pointer items-center gap-1 rounded border border-border-subtle bg-bg-elevated px-1.5 text-[10px] text-text-secondary transition-colors duration-150 hover:bg-amber/20 hover:text-amber disabled:cursor-not-allowed disabled:opacity-40"
          disabled={selectableWorktreeEntries.length === 0}
          title={selectableWorktreeEntries.length === 0
            ? "No worktrees on disk in the current selection"
            : `Remove ${selectableWorktreeEntries.length} worktree${selectableWorktreeEntries.length === 1 ? "" : "s"} on disk`}
          onclick={handleBulkRemoveWorktrees}
        >
          <Trash size={10} />
          <span>Worktrees</span>
        </button>
        <button
          type="button"
          class="inline-flex h-5 cursor-pointer items-center gap-1 rounded border border-border-subtle bg-bg-elevated px-1.5 text-[10px] text-text-secondary transition-colors duration-150 hover:bg-red/20 hover:text-red"
          title={`Permanently delete ${selected.size} history entr${selected.size === 1 ? "y" : "ies"}`}
          onclick={handleBulkDelete}
        >
          <FileX size={10} />
          <span>Delete</span>
        </button>
        <button
          type="button"
          class="inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded text-text-muted hover:bg-bg-hover hover:text-text-primary"
          title="Clear selection"
          aria-label="Clear selection"
          onclick={clearSelection}
        >
          <X size={11} />
        </button>
      </div>
    {/if}

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
      {#if bulkError}
        <div class="mb-1 flex items-center gap-2 border border-red/30 bg-red/10 px-2 py-1 text-[11px] text-red">
          <span class="min-w-0 flex-1 truncate" data-testid="archived-bulk-error">{bulkError}</span>
          <button
            type="button"
            class="flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center text-red/80 hover:text-red focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/50"
            aria-label="Dismiss bulk action error"
            title="Dismiss"
            onclick={() => (bulkError = null)}
          >
            <X size={11} />
          </button>
        </div>
      {/if}
      {#if archivedList.length === 0}
        <div class="px-2 py-1 text-[11px] text-text-muted">
          No closed sessions yet.
        </div>
      {:else if filteredList.length === 0}
        <div class="px-2 py-1 text-[11px] text-text-muted">
          No archived sessions match "{filterText}".
        </div>
      {:else}
        <label
          class="mb-1 flex cursor-pointer items-center gap-2 px-2 py-1 text-[10px] text-text-muted hover:text-text-secondary"
        >
          <input
            type="checkbox"
            class="h-3 w-3 cursor-pointer rounded border border-border bg-bg-deep accent-accent"
            checked={allVisibleSelected}
            indeterminate={someVisibleSelected}
            onchange={toggleAllVisible}
            data-testid="archived-select-all"
          />
          <span>
            {#if filterText}
              Select {filteredList.length} match{filteredList.length === 1 ? "" : "es"}
            {:else}
              Select all
            {/if}
          </span>
        </label>
        {#each filteredList as s (s.id)}
          {@const wtExists = worktreeExists.get(s.id) ?? true}
          {@const isSelected = selected.has(s.id)}
          <div
            class="group relative mb-1 border px-2 py-1.5 text-left text-sm transition-colors duration-150 {isSelected
              ? 'border-accent-dim/60 bg-accent-dim/10'
              : 'border-transparent hover:border-border-subtle hover:bg-bg-active/40 focus-within:border-border-subtle focus-within:bg-bg-active/40'}"
            data-testid="archived-session-row"
            data-archived-menu-root={s.id}
          >
            <div class="flex min-h-6 items-center gap-2">
              <input
                type="checkbox"
                class="h-3 w-3 shrink-0 cursor-pointer rounded border border-border bg-bg-deep accent-accent"
                checked={isSelected}
                onchange={() => toggleSelection(s.id)}
                aria-label={`Select ${sessionDisplayName(s)}`}
                data-testid="archived-row-checkbox"
              />
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
            <div class="ml-9 mt-0.5 flex min-h-5 items-center gap-1.5 overflow-hidden text-[10px] text-text-muted">
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
              <div class="ml-9 mt-1 flex items-center gap-2 border border-red/30 bg-red/10 px-2 py-1 text-[10px] text-red">
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
