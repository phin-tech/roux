<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import {
    listLibraryItems,
    listLibrarySources,
    readLibraryItem,
    saveLibraryItem,
    type LibraryItem,
    type LibraryItemType,
    type LibraryRead,
    type LibrarySource,
    type SaveLibraryItemRequest,
  } from "$lib/tauri";
  import { activeSession } from "$lib/stores/sessions";
  import {
    closeLibraryWindow,
    libraryWindow,
    openLibraryEdit,
    openLibraryNew,
  } from "$lib/stores/libraryWindow";
  import LibraryItemEditor from "./LibraryItemEditor.svelte";

  let items = $state<LibraryItem[]>([]);
  let sources = $state<LibrarySource[]>([]);
  let selected: LibraryRead | null = $state(null);
  let selectedSessionId: string | null = $state(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let filter = $state("");
  let typeFilter = $state<"all" | "prompt" | "skill">("all");
  let windowEl: HTMLElement | undefined = $state();
  let editDirty = $state(false);
  let wasVisible = $state(false);

  let sessionId = $derived($activeSession?.id ?? null);
  let activeRepo = $derived($activeSession?.repoRoot ?? null);
  let visible = $derived($libraryWindow.open);

  const filteredItems = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return items.filter((item) => {
      if (typeFilter !== "all" && item.itemType !== typeFilter) return false;
      if (!q) return true;
      return (
        item.title.toLowerCase().includes(q) ||
        item.id.toLowerCase().includes(q) ||
        (item.description ?? "").toLowerCase().includes(q) ||
        item.tags.some((tag) => tag.toLowerCase().includes(q))
      );
    });
  });

  async function refresh() {
    if (!visible) return;
    loading = true;
    error = null;
    try {
      const [nextItems, nextSources] = await Promise.all([
        listLibraryItems(sessionId),
        listLibrarySources(),
      ]);
      items = nextItems;
      sources = nextSources;
      if (selected && !editDirty) {
        const selectedId = selected.item.id;
        if (nextItems.some((item) => item.id === selectedId)) {
          selected = await readLibraryItem(selectedId, sessionId);
          selectedSessionId = sessionId;
        } else {
          selected = null;
          selectedSessionId = null;
        }
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function selectItem(item: LibraryItem) {
    if (!confirmDiscard()) return;
    loading = true;
    error = null;
    editDirty = false;
    try {
      selected = await readLibraryItem(item.id, sessionId);
      selectedSessionId = sessionId;
      openLibraryEdit(item.id, item.itemType);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function newItem(itemType: LibraryItemType) {
    if (!confirmDiscard()) return;
    selected = null;
    selectedSessionId = null;
    editDirty = false;
    openLibraryNew(itemType);
  }

  async function saveEditedItem(request: SaveLibraryItemRequest) {
    error = null;
    try {
      const saved = await saveLibraryItem(request, sessionId);
      await refresh();
      const item = items.find((candidate) => candidate.id === saved.itemId);
      if (item) {
        selected = await readLibraryItem(item.id, sessionId);
        selectedSessionId = sessionId;
        editDirty = false;
        openLibraryEdit(item.id, item.itemType);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!visible) return;
    if (e.key === "Escape") {
      if (isEditableTarget(e.target)) return;
      e.preventDefault();
      closeSafely();
      return;
    }
    if (e.key === "Tab") {
      trapFocus(e);
    }
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    return Boolean(target.closest("input, textarea, select, [contenteditable='true'], .cm-editor"));
  }

  function focusableElements(): HTMLElement[] {
    if (!windowEl) return [];
    return Array.from(
      windowEl.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
      ),
    ).filter((el) => !el.hasAttribute("disabled") && el.offsetParent !== null);
  }

  function trapFocus(e: KeyboardEvent) {
    const focusable = focusableElements();
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function confirmDiscard(): boolean {
    if (!editDirty) return true;
    return window.confirm("Discard unsaved Library changes?");
  }

  function closeSafely() {
    if (!confirmDiscard()) return;
    editDirty = false;
    closeLibraryWindow();
  }

  function cancelEditor() {
    closeSafely();
  }

  function layerLabel(item: LibraryItem): string {
    if (item.sourceLayer === "activeRepo") return "active";
    if (item.sourceLayer === "localRepo" || item.sourceLayer === "gitRepo") return item.sourceLabel;
    return "global";
  }

  $effect(() => {
    void visible;
    void sessionId;
    if (visible) void refresh();
  });

  $effect(() => {
    if (visible && !wasVisible) {
      if ($libraryWindow.mode === "browse") {
        selected = null;
        selectedSessionId = null;
        filter = "";
        typeFilter = "all";
        editDirty = false;
      }
      requestAnimationFrame(() => windowEl?.focus());
    }
    wasVisible = visible;
  });

  $effect(() => {
    const itemId = $libraryWindow.itemId;
    if (!visible || !itemId) return;
    if (selected?.item.id === itemId && selectedSessionId === sessionId) return;
    void readLibraryItem(itemId, sessionId)
      .then((item) => {
        selected = item;
        selectedSessionId = sessionId;
      })
      .catch((e) => {
        error = e instanceof Error ? e.message : String(e);
      });
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if visible}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
    <div
      bind:this={windowEl}
      class="relative flex h-[min(860px,calc(100vh-32px))] w-[min(1180px,calc(100vw-32px))] min-h-0 overflow-hidden rounded-2xl border border-hairline bg-bg-deep shadow-[0_30px_80px_rgba(2,6,23,0.68),0_0_0_1px_rgba(255,255,255,0.04),0_0_48px_rgba(95,128,255,0.12)] before:pointer-events-none before:absolute before:inset-0 before:rounded-2xl before:shadow-[inset_0_1px_0_rgba(255,255,255,0.08),inset_0_0_42px_rgba(95,128,255,0.045)]"
      role="dialog"
      aria-modal="true"
      aria-labelledby="library-window-title"
      tabindex="-1"
    >
      <aside class="flex w-[min(320px,34vw)] min-w-[240px] shrink-0 flex-col border-r border-hairline bg-bg-surface/30 py-3">
        <div class="border-b border-hairline px-3 pb-3">
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              aria-label="Close library manager"
              title="Close library manager"
              onclick={closeSafely}
            >
              <X size={14} />
            </button>
            <div class="min-w-0">
              <div id="library-window-title" class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Library Manager</div>
              <div class="mt-1 text-[13px] font-semibold text-text-primary">Prompts & Skills</div>
            </div>
          </div>
          <div class="mt-3 flex gap-1">
            <button type="button" class="flex-1 rounded-md border border-border-subtle bg-bg-surface px-2 py-1.5 text-xs font-semibold text-text-secondary hover:bg-bg-hover hover:text-text-primary" onclick={() => newItem("prompt")}>+ Prompt</button>
            <button type="button" class="flex-1 rounded-md border border-border-subtle bg-bg-surface px-2 py-1.5 text-xs font-semibold text-text-secondary hover:bg-bg-hover hover:text-text-primary" onclick={() => newItem("skill")}>+ Skill</button>
          </div>
          <input
            class="mt-3 w-full border border-border-subtle bg-bg-deep px-3 py-2 text-[13px] text-text-primary placeholder:text-text-muted outline-none focus:border-border"
            placeholder="Filter library..."
            bind:value={filter}
          />
          <div class="mt-2 grid grid-cols-3 border border-border-subtle">
            {#each ["all", "prompt", "skill"] as type}
              <button
                type="button"
                class="px-2 py-1.5 text-[11px] font-medium uppercase tracking-[0.16em] {typeFilter === type ? 'bg-bg-active text-text-primary' : 'text-text-secondary hover:bg-bg-hover'}"
                onclick={() => (typeFilter = type as typeof typeFilter)}
              >
                {type}
              </button>
            {/each}
          </div>
        </div>

        <div class="app-scrollbar min-h-0 flex-1 overflow-y-auto p-2">
          {#if loading && items.length === 0}
            <p class="py-4 text-center text-xs text-text-muted">Loading...</p>
          {:else if filteredItems.length === 0}
            <p class="py-4 text-center text-xs text-text-muted">No prompts or skills found</p>
          {:else}
            {#each filteredItems as item (item.id)}
              <button
                type="button"
                class="mb-1 flex w-full cursor-pointer items-start gap-2 px-2 py-2 text-left transition-colors {selected?.item.id === item.id ? 'bg-bg-active' : 'hover:bg-bg-hover'}"
                onclick={() => selectItem(item)}
                title={item.sourcePath}
              >
                <span class="mt-0.5 shrink-0 border border-border-subtle px-1.5 py-0.5 text-[9px] uppercase tracking-[0.14em] text-text-secondary">{item.itemType}</span>
                <span class="min-w-0 flex-1">
                  <span class="block truncate text-[13px] font-semibold text-text-primary">{item.title}</span>
                  <span class="mt-0.5 block truncate font-mono text-[10px] text-text-muted">{item.id} · {layerLabel(item)}</span>
                </span>
              </button>
            {/each}
          {/if}
        </div>
      </aside>

      <section class="flex min-w-0 flex-1 flex-col bg-bg-deep">
        <div class="flex h-12 shrink-0 items-center justify-between border-b border-hairline px-4">
          <div class="min-w-0">
            <div class="truncate text-sm font-semibold text-text-primary">
              {$libraryWindow.mode === "new" ? `New ${$libraryWindow.itemType}` : selected ? `Editing ${selected.item.title}` : "Select an item"}
            </div>
            <div class="mt-0.5 truncate font-mono text-[10px] text-text-muted">
              {selected?.item.sourcePath ?? "Choose a source and save to write a Library markdown file"}
            </div>
          </div>
          <button type="button" class="rounded-lg border border-border-subtle bg-bg-surface px-3 py-1.5 text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary" onclick={refresh}>Refresh</button>
        </div>

        {#if error}
          <div class="mx-4 mt-3 border border-red/30 bg-red/10 px-3 py-2 text-xs text-red">{error}</div>
        {/if}

        <div class="app-scrollbar min-h-0 flex-1 overflow-y-auto p-4">
          {#if $libraryWindow.mode === "new"}
            <LibraryItemEditor
              item={null}
              itemType={$libraryWindow.itemType}
              {sources}
              activeRepo={activeRepo}
              onsave={saveEditedItem}
              oncancel={cancelEditor}
              ondirtychange={(dirty) => (editDirty = dirty)}
            />
          {:else if $libraryWindow.mode === "edit" && selected}
            <LibraryItemEditor
              item={selected}
              itemType={selected.item.itemType}
              {sources}
              activeRepo={activeRepo}
              onsave={saveEditedItem}
              oncancel={cancelEditor}
              ondirtychange={(dirty) => (editDirty = dirty)}
            />
          {:else if $libraryWindow.mode === "browse"}
            <div class="flex h-full min-h-[360px] items-center justify-center text-sm text-text-muted">Select a prompt or skill, or create a new one.</div>
          {:else}
            <div class="flex h-full min-h-[360px] items-center justify-center text-sm text-text-muted">Loading editor...</div>
          {/if}
        </div>
      </section>
    </div>
  </div>
{/if}
