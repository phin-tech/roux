<script lang="ts">
  import DOMPurify from "dompurify";
  import { marked } from "marked";
  import { get } from "svelte/store";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    cloneLibrarySource,
    getLibrarySourceStatuses,
    librarySkillSyncRun,
    listLibraryItems,
    listLibrarySources,
    readLibraryItem,
    renderLibraryPrompt,
    setLibrarySources,
    syncLibrarySource,
    writeToSession,
    type LibraryGitStatus,
    type LibraryItemType,
    type LibraryItem,
    type LibraryRead,
    type LibrarySource,
    type SkillSyncMode,
  } from "$lib/tauri";
  import { settings, updateSetting } from "$lib/stores/settings";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Check from "@lucide/svelte/icons/check";
  import CloudOff from "@lucide/svelte/icons/cloud-off";
  import FilePenLine from "@lucide/svelte/icons/file-pen-line";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import GitCompareArrows from "@lucide/svelte/icons/git-compare-arrows";
  import History from "@lucide/svelte/icons/history";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Settings from "@lucide/svelte/icons/settings";
  import { activeSession } from "$lib/stores/sessions";
  import { openLibraryEdit, openLibraryNew, openLibraryWindow } from "$lib/stores/libraryWindow";
  import {
    initialLibraryVariableValue,
    libraryVariableType,
    requestLibraryVariables,
    validateLibraryVariableValues,
  } from "$lib/stores/libraryVariablePrompt";
  import { focusedPaneId } from "$lib/panes/focus";
  import { getAttachedPtyId, paneInstances } from "$lib/panes/instances";
  import { clearDraggedLibraryPrompt, writeLibraryPromptDragData } from "$lib/library/drag";
  import PinButton from "./PinButton.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();

  let items = $state<LibraryItem[]>([]);
  let selected: LibraryRead | null = $state(null);
  let renderedHtml = $state("");
  let loading = $state(false);
  let error = $state<string | null>(null);
  let filter = $state("");
  let typeFilter = $state<"all" | "prompt" | "skill">("all");
  let view = $state<"items" | "sources">("items");
  let variableValues = $state<Record<string, string>>({});
  let variableErrors = $state<Record<string, string>>({});
  let sendStatus = $state<"idle" | "sent" | "error">("idle");
  let sources = $state<LibrarySource[]>([]);
  let gitStatuses = $state<Record<string, LibraryGitStatus>>({});
  let repoDraft = $state("");
  let gitNameDraft = $state("");
  let gitUrlDraft = $state("");
  let gitBranchDraft = $state("main");
  let busySourceId = $state<string | null>(null);
  let filterInput: HTMLInputElement | undefined = $state();
  let wasVisible = $state(false);

  let sessionId = $derived($activeSession?.id ?? null);
  let activeRepo = $derived($activeSession?.repoRoot ?? null);

  // Sync now should be enabled whenever the effective mode for any source
  // (or the global vault / active repo, which always inherit the default)
  // is non-off. A user can have global=off plus a per-source override of
  // copy/symlink, in which case a sync run still does work.
  function skillSyncEnabledForAnySource(s: typeof $settings): boolean {
    const def = s.librarySkillSyncDefault ?? "off";
    if (def !== "off") return true;
    return (s.librarySources ?? []).some(
      (source) => (source.skillSync ?? def) !== "off",
    );
  }

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
      const [nextItems, nextSources, nextStatuses] = await Promise.all([
        listLibraryItems(sessionId),
        listLibrarySources(),
        getLibrarySourceStatuses(),
      ]);
      items = nextItems;
      sources = nextSources;
      gitStatuses = Object.fromEntries(nextStatuses.map((status) => [status.sourceId, status]));
      if (selected) {
        const selectedId = selected.item.id;
        if (nextItems.some((item) => item.id === selectedId)) {
          await showRead(await readLibraryItem(selectedId, sessionId));
        } else {
          selected = null;
          renderedHtml = "";
          variableValues = {};
          variableErrors = {};
        }
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function selectItem(item: LibraryItem) {
    loading = true;
    error = null;
    sendStatus = "idle";
    try {
      await showRead(await readLibraryItem(item.id, sessionId));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function showRead(next: LibraryRead) {
    selected = next;
    renderedHtml = DOMPurify.sanitize(await marked(next.body));
    variableValues = {};
    for (const variable of next.item.variables) {
      variableValues[variable.name] = initialLibraryVariableValue(variable);
    }
    variableErrors = {};
  }

  function newItem(itemType: LibraryItemType) {
    openLibraryNew(itemType);
  }

  function editSelected() {
    if (!selected) return;
    openLibraryEdit(selected.item.id, selected.item.itemType);
  }

  function targetPtyId(): string | null {
    const focused = get(focusedPaneId);
    if (focused) {
      const pane = get(paneInstances).get(focused);
      const ptyId = pane ? getAttachedPtyId(pane) : null;
      if (ptyId) return ptyId;
    }
    return sessionId;
  }

  async function sendSelected() {
    if (!selected) return;
    await sendRead(selected, async () => collectPromptVariables());
  }

  async function sendItem(item: LibraryItem, event?: MouseEvent) {
    event?.stopPropagation();
    const ptyId = targetPtyId();
    if (!ptyId) {
      error = "No active session or focused pane to send to.";
      sendStatus = "error";
      return;
    }
    try {
      const read = await readLibraryItem(item.id, sessionId);
      await sendRead(read, async () => {
        if (read.item.itemType !== "prompt") return {};
        return requestLibraryVariables({
          title: read.item.title,
          variables: read.item.variables,
          initialValues: {},
        });
      }, ptyId);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      sendStatus = "error";
    }
  }

  function onItemDragStart(event: DragEvent, item: LibraryItem) {
    if (!event.dataTransfer || !writeLibraryPromptDragData(event.dataTransfer, item)) {
      event.preventDefault();
      return;
    }
  }

  async function sendRead(
    read: LibraryRead,
    collectVariables: () => Promise<Record<string, string> | null>,
    resolvedPtyId: string | null = null,
  ) {
    const ptyId = targetPtyId();
    const target = resolvedPtyId ?? ptyId;
    if (!target) {
      error = "No active session or focused pane to send to.";
      sendStatus = "error";
      return;
    }
    try {
      const variables = read.item.itemType === "prompt" ? await collectVariables() : {};
      if (!variables) return;
      const content = read.item.itemType === "prompt"
          ? (await renderLibraryPrompt({
              itemId: read.item.id,
              sessionId,
              variables,
            })).content
          : read.body;
      await writeToSession(target, `${content}\r`);
      sendStatus = "sent";
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      sendStatus = "error";
    }
  }

  async function collectPromptVariables(): Promise<Record<string, string> | null> {
    if (!selected) return null;
    const nextValues = { ...variableValues };
    const errors = validateLibraryVariableValues(selected.item.variables, nextValues);
    variableErrors = errors;
    if (Object.keys(errors).length > 0) {
      sendStatus = "idle";
      return null;
    }
    return nextValues;
  }

  function updateSelectedVariable(name: string, value: string) {
    variableValues = { ...variableValues, [name]: value };
    const nextErrors = { ...variableErrors };
    delete nextErrors[name];
    variableErrors = nextErrors;
  }

  async function browsePinnedRepo() {
    const selectedPath = await open({ directory: true, title: "Select Library Repo" });
    if (typeof selectedPath === "string") {
      repoDraft = selectedPath;
    }
  }

  async function addPinnedRepo(path: string) {
    const trimmed = path.trim();
    if (!trimmed || sources.some((source) => source.kind === "localRepo" && source.path === trimmed)) {
      repoDraft = "";
      return;
    }
    await saveSources([
      ...sources,
      {
        id: "",
        kind: "localRepo",
        name: "",
        enabled: true,
        order: sources.length,
        path: trimmed,
        url: null,
        branch: null,
      },
    ]);
    repoDraft = "";
  }

  async function addGitSource() {
    const url = gitUrlDraft.trim();
    if (!url) return;
    await saveSources([
      ...sources,
      {
        id: "",
        kind: "gitRepo",
        name: gitNameDraft.trim(),
        enabled: true,
        order: sources.length,
        path: null,
        url,
        branch: gitBranchDraft.trim() || "main",
      },
    ]);
    gitNameDraft = "";
    gitUrlDraft = "";
    gitBranchDraft = "main";
  }

  async function removeSource(id: string) {
    await saveSources(sources.filter((source) => source.id !== id));
  }

  async function toggleSource(source: LibrarySource) {
    await saveSources(sources.map((item) => (item.id === source.id ? { ...item, enabled: !item.enabled } : item)));
  }

  async function moveSource(index: number, delta: -1 | 1) {
    const nextIndex = index + delta;
    if (nextIndex < 0 || nextIndex >= sources.length) return;
    const next = [...sources];
    const [source] = next.splice(index, 1);
    next.splice(nextIndex, 0, source);
    await saveSources(next);
  }

  async function saveSources(next: LibrarySource[]) {
    const previousSources = sources;
    error = null;
    try {
      sources = await setLibrarySources(next.map((source, index) => ({ ...source, order: index })));
      await refresh();
    } catch (e) {
      sources = previousSources;
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function pinActiveRepo() {
    if (!activeRepo) return;
    await addPinnedRepo(activeRepo);
  }

  function layerLabel(item: LibraryItem): string {
    if (item.sourceLayer === "activeRepo") return "active";
    if (item.sourceLayer === "localRepo" || item.sourceLayer === "gitRepo") return item.sourceLabel;
    return "global";
  }

  function itemCountForLayer(layer: "global" | "activeRepo" | "localRepo" | "gitRepo", sourceId?: string | null): number {
    return items.filter((item) => {
      if (item.sourceLayer !== layer) return false;
      return sourceId ? item.sourceId === sourceId : true;
    }).length;
  }

  function itemCountForType(itemType: "prompt" | "skill"): number {
    return items.filter((item) => item.itemType === itemType).length;
  }

  function shortRepo(path: string | null): string {
    if (!path) return "";
    const segments = path.split("/").filter(Boolean);
    const forgeHosts = new Set(["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"]);
    for (let i = 0; i < segments.length - 2; i++) {
      if (forgeHosts.has(segments[i])) return `${segments[i + 1]}/${segments[i + 2]}`;
    }
    if (segments.length >= 2) return `${segments[segments.length - 2]}/${segments[segments.length - 1]}`;
    return segments[0] ?? path;
  }

  function libraryPathForRepo(repo: string): string {
    return `${repo}/.roux/library`;
  }

  function sourceSubtitle(source: LibrarySource): string {
    if (source.kind === "localRepo") return libraryPathForRepo(source.path ?? "");
    return `${source.url ?? ""}${source.branch ? ` · ${source.branch}` : ""}`;
  }

  async function cloneSource(source: LibrarySource) {
    busySourceId = source.id;
    error = null;
    try {
      await cloneLibrarySource(source.id);
      await refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busySourceId = null;
    }
  }

  async function syncSource(source: LibrarySource) {
    busySourceId = source.id;
    error = null;
    try {
      const status = await syncLibrarySource(source.id);
      gitStatuses = { ...gitStatuses, [status.sourceId]: status };
      await refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busySourceId = null;
    }
  }

  function sourceStatus(source: LibrarySource): LibraryGitStatus | null {
    return source.kind === "gitRepo" ? (gitStatuses[source.id] ?? null) : null;
  }

  function gitStatusTitle(status: LibraryGitStatus): string {
    if (status.error) return status.error;
    if (!status.checkedOut) return "Not cloned";
    if (status.dirty) return "Uncommitted local changes";
    if (status.remoteState === "behind") return `${status.behind} commit${status.behind === 1 ? "" : "s"} behind ${status.trackingBranch ?? "remote"}`;
    if (status.remoteState === "ahead") return `${status.ahead} commit${status.ahead === 1 ? "" : "s"} ahead of ${status.trackingBranch ?? "remote"}`;
    if (status.remoteState === "diverged") return `Diverged from ${status.trackingBranch ?? "remote"}`;
    if (status.remoteState === "upToDate") return `Up to date with ${status.trackingBranch ?? "remote"}`;
    return "Git status unknown";
  }

  $effect(() => {
    void visible;
    void sessionId;
    if (visible) void refresh();
  });

  $effect(() => {
    if (visible && !wasVisible) {
      view = "items";
      requestAnimationFrame(() => filterInput?.focus());
    }
    wasVisible = visible;
  });
</script>

<div class="flex h-full min-h-0 flex-col bg-bg-deep" class:hidden={!visible}>
  <div class="flex h-10 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3">
    <div class="flex min-w-0 items-center gap-2">
      <span class="text-[12px] font-bold uppercase tracking-[0.13em] text-text-primary">Library</span>
      <span class="rounded bg-green/15 px-2 py-0.5 font-mono text-[10px] font-semibold text-green">{items.length}</span>
    </div>
    <div class="flex shrink-0 items-center gap-1">
      <button
        type="button"
        class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
        title="Refresh library"
        aria-label="Refresh library"
        disabled={loading}
        onclick={refresh}
      >
        <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
      </button>
      <button
        type="button"
        class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
        title="Manage library"
        aria-label="Manage library"
        onclick={openLibraryWindow}
      >
        <Settings size={14} />
      </button>
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <CollapseSidebarButton onclick={onclose} label="Collapse library sidebar" title="Collapse library sidebar" />
    </div>
  </div>

  <div
    class="flex h-6 shrink-0 items-center gap-2 border-b border-hairline bg-bg-surface/20 px-3"
    title={activeRepo ? libraryPathForRepo(activeRepo) : "Global library"}
  >
    <span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Scope</span>
    <span class="truncate font-mono text-[10px] text-text-secondary">
      {activeRepo ? shortRepo(activeRepo) : "global"}
    </span>
  </div>

  <div class="flex shrink-0 border-b border-hairline bg-bg-surface/20 text-[11px]">
    {#each [
      { id: "items", label: "Items", count: items.length },
      { id: "sources", label: "Sources", count: sources.length },
    ] as const as tab}
      <button
        type="button"
        class="cursor-pointer px-3 py-2 transition-colors {view === tab.id ? 'border-b-2 border-accent text-text-primary' : 'text-text-secondary hover:bg-bg-hover'}"
        onclick={() => (view = tab.id)}
      >
        {tab.label}
        {#if tab.count > 0}
          <span class="ml-1 rounded bg-bg-active px-1 text-[9px] font-semibold text-text-muted">{tab.count}</span>
        {/if}
      </button>
    {/each}
  </div>

  <div class="app-scrollbar flex min-h-0 flex-1 flex-col overflow-y-auto">
    {#if error}
      <div class="mx-3 mt-3 border border-red/30 bg-red/10 px-3 py-2 text-xs text-red">{error}</div>
    {/if}

    {#if view === "items"}
      <div class="flex min-h-0 flex-col gap-3 p-3">
        <div class="flex items-center justify-between gap-2">
          <span class="text-[10px] uppercase tracking-wider text-text-muted">
            {filteredItems.length}
            {filteredItems.length === 1 ? "item" : "items"}
          </span>
          <div class="flex shrink-0 items-center gap-1">
            <button
              type="button"
              class="flex cursor-pointer items-center gap-1 rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-primary hover:border-accent hover:text-accent"
              onclick={() => newItem("prompt")}
            >
              <Plus size={12} />
              Prompt
            </button>
            <button
              type="button"
              class="flex cursor-pointer items-center gap-1 rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-primary hover:border-accent hover:text-accent"
              onclick={() => newItem("skill")}
            >
              <Plus size={12} />
              Skill
            </button>
          </div>
        </div>

        <input
          bind:this={filterInput}
          class="w-full rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary placeholder:text-text-muted outline-none focus:border-accent-dim"
          placeholder="Filter library..."
          bind:value={filter}
        />

        <div class="flex overflow-hidden rounded border border-border bg-bg-deep text-[11px]">
          {#each [
            { id: "all", label: "All", count: items.length },
            { id: "prompt", label: "Prompts", count: itemCountForType("prompt") },
            { id: "skill", label: "Skills", count: itemCountForType("skill") },
          ] as const as type}
            <button
              type="button"
              class="flex-1 cursor-pointer px-2 py-1 transition-colors {typeFilter === type.id ? 'bg-accent-dim text-text-primary' : 'text-text-secondary hover:bg-bg-hover'}"
              onclick={() => (typeFilter = type.id)}
            >
              {type.label}
              {#if type.count > 0}
                <span class="ml-1 font-mono text-[9px] text-text-muted">{type.count}</span>
              {/if}
            </button>
          {/each}
        </div>

        {#if loading && items.length === 0}
          <p class="rounded border border-border-subtle bg-bg-surface/30 p-3 text-sm text-text-muted">Loading...</p>
        {:else if filteredItems.length === 0}
          <p class="rounded border border-border-subtle bg-bg-surface/30 p-3 text-sm text-text-muted">No prompts or skills found</p>
        {:else}
          <ul class="flex flex-col gap-2">
            {#each filteredItems as item (item.id)}
              <li>
                <div
                  role="presentation"
                  class="group flex items-center gap-2 rounded border border-border-subtle bg-bg-surface/30 p-2 transition-colors {item.itemType === 'prompt' ? 'cursor-grab active:cursor-grabbing' : ''} {selected?.item.id === item.id ? 'border-accent-dim/50 bg-accent-dim/15' : 'hover:bg-bg-hover'}"
                  title={item.sourcePath}
                  draggable={item.itemType === "prompt"}
                  ondragstart={(event) => onItemDragStart(event, item)}
                  ondragend={clearDraggedLibraryPrompt}
                >
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
                    onclick={() => selectItem(item)}
                  >
                    <span class="rounded bg-accent-dim/20 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-accent">
                      {item.itemType}
                    </span>
                    <span class="min-w-0 flex-1">
                      <span class="block truncate text-xs font-semibold text-text-primary">{item.title}</span>
                      <span class="mt-0.5 block truncate font-mono text-[10px] text-text-muted">{item.id} · {layerLabel(item)}</span>
                    </span>
                  </button>
                  <button
                    type="button"
                    class="shrink-0 rounded border border-accent-dim/40 bg-accent-dim/15 px-2 py-0.5 text-[10px] font-semibold text-accent opacity-0 transition-opacity hover:bg-accent-dim/30 focus:opacity-100 group-hover:opacity-100"
                    aria-label={`Send ${item.title}`}
                    onclick={(e) => void sendItem(item, e)}
                  >
                    Send
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}

        {#if selected}
          <div class="rounded border border-border-subtle bg-bg-surface/20 p-3">
            <div class="mb-3 flex items-start justify-between gap-3">
              <div class="min-w-0">
                <div class="truncate text-sm font-semibold text-text-primary">{selected.item.title}</div>
                <div class="mt-1 truncate font-mono text-[10px] text-text-muted">{selected.item.sourcePath}</div>
              </div>
              <div class="flex shrink-0 gap-1">
                <button
                  type="button"
                  class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-text-primary"
                  onclick={editSelected}
                >
                  Edit
                </button>
                <button
                  type="button"
                  class="rounded border border-accent-dim/40 bg-accent-dim/20 px-2 py-0.5 text-[10px] text-accent hover:bg-accent-dim/40"
                  onclick={sendSelected}
                >
                  {selected.item.itemType === "prompt" ? "Send" : "Send context"}
                </button>
              </div>
            </div>

            {#if selected.item.variables.length > 0}
              <div class="mb-3 space-y-2 rounded border border-border-subtle bg-bg-surface/40 p-2">
                {#each selected.item.variables as variable (variable.name)}
                  <label class="block">
                    <span class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-muted">{variable.label ?? variable.name}</span>
                    {#if libraryVariableType(variable) === "select"}
                      <select
                        class="w-full rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim {variableErrors[variable.name] ? 'border-red/50' : ''}"
                        value={variableValues[variable.name] ?? ""}
                        onchange={(e) => updateSelectedVariable(variable.name, e.currentTarget.value)}
                      >
                        {#if !variable.required}
                          <option value="">None</option>
                        {/if}
                        {#each variable.options ?? [] as option}
                          <option value={option}>{option}</option>
                        {/each}
                      </select>
                    {:else}
                      <input
                        class="w-full rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim {variableErrors[variable.name] ? 'border-red/50' : ''}"
                        type={libraryVariableType(variable) === "int" || libraryVariableType(variable) === "float" ? "number" : "text"}
                        step={libraryVariableType(variable) === "int" ? "1" : libraryVariableType(variable) === "float" ? "any" : undefined}
                        value={variableValues[variable.name] ?? ""}
                        oninput={(e) => updateSelectedVariable(variable.name, e.currentTarget.value)}
                        placeholder={variable.required ? "Required" : ""}
                      />
                    {/if}
                    {#if variableErrors[variable.name]}
                      <div class="mt-1 text-[11px] text-red">{variableErrors[variable.name]}</div>
                    {/if}
                  </label>
                {/each}
              </div>
            {/if}

            {#if sendStatus === "sent"}
              <div class="mb-3 rounded border border-green/25 bg-green/10 px-2 py-1.5 text-xs text-green">Sent to agent</div>
            {/if}

            <div class="library-prose rounded border border-border-subtle bg-bg-deep/50 p-3">
              {@html renderedHtml}
            </div>
          </div>
        {:else}
          <div class="rounded border border-border-subtle bg-bg-surface/30 p-3 text-sm text-text-muted">Select a prompt or skill</div>
        {/if}
      </div>
    {:else}
      <div class="space-y-3 p-3">
        <div class="rounded border border-border-subtle bg-bg-surface/30 p-3">
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0">
              <div class="text-sm font-semibold text-text-primary">Global Library</div>
              <div class="mt-1 text-xs text-text-secondary">Shared prompts and skills from your Roux vault.</div>
              <div class="mt-2 font-mono text-[10px] text-text-muted">library/prompts · library/skills</div>
            </div>
            <span class="shrink-0 rounded bg-bg-active px-2 py-0.5 font-mono text-[10px] font-semibold text-text-muted">
              {itemCountForLayer("global")}
            </span>
          </div>
        </div>

        <div class="rounded border border-border-subtle bg-bg-surface/30 p-3">
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0">
              <div class="text-sm font-semibold text-text-primary">Active Repo Library</div>
              {#if activeRepo}
                <div class="mt-1 truncate font-mono text-[10px] text-text-muted">{libraryPathForRepo(activeRepo)}</div>
              {:else}
                <div class="mt-1 text-xs text-text-muted">No active repo session</div>
              {/if}
            </div>
            <span class="shrink-0 rounded bg-bg-active px-2 py-0.5 font-mono text-[10px] font-semibold text-text-muted">
              {itemCountForLayer("activeRepo")}
            </span>
          </div>
          {#if activeRepo}
            <button type="button" class="mt-3 rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:border-accent hover:text-accent" onclick={pinActiveRepo}>
              Pin active repo
            </button>
          {/if}
        </div>

        <div class="rounded border border-border-subtle bg-bg-surface/30 p-3">
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="min-w-0 flex-1">
              <div class="text-sm font-semibold text-text-primary">Skill sync</div>
              <div class="mt-1 text-xs text-text-secondary">
                Mirror Library skills into <code class="font-mono text-[11px]">.claude/skills/</code> so Claude can load them.
                Off by default.
              </div>
            </div>
            <select
              class="shrink-0 rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-primary outline-none focus:border-accent-dim"
              value={$settings.librarySkillSyncDefault ?? "off"}
              onchange={(event) => {
                const next = (event.currentTarget as HTMLSelectElement).value as SkillSyncMode;
                updateSetting("librarySkillSyncDefault", next);
              }}
              aria-label="Default skill sync mode"
            >
              <option value="off">Off</option>
              <option value="copy">Copy</option>
              <option value="symlink">Symlink</option>
            </select>
          </div>
          <button
            type="button"
            class="mt-2 rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:border-accent hover:text-accent disabled:opacity-30"
            disabled={!skillSyncEnabledForAnySource($settings)}
            onclick={async () => {
              try {
                await librarySkillSyncRun(sessionId);
              } catch (e) {
                console.error("library skill sync failed", e);
              }
            }}
          >
            Sync now
          </button>
        </div>

        <div class="rounded border border-border-subtle bg-bg-surface/30 p-3">
          <div class="mb-3 flex items-center justify-between gap-3">
            <div>
              <div class="text-sm font-semibold text-text-primary">Library Sources</div>
              <div class="mt-1 text-xs text-text-secondary">Ordered layers. Later sources override earlier sources.</div>
            </div>
            <span class="rounded bg-bg-active px-2 py-0.5 font-mono text-[10px] font-semibold text-text-muted">{sources.length}</span>
          </div>

          {#if sources.length === 0}
            <p class="mb-3 rounded border border-border-subtle bg-bg-deep/50 p-3 text-sm text-text-muted">No library sources</p>
          {:else}
            <ul class="mb-3 flex flex-col gap-2">
              {#each sources as source, index (source.id)}
                <li class="rounded border border-border-subtle bg-bg-deep/50 p-2">
                  <div class="flex flex-wrap items-center gap-2">
                  <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-bg-active font-mono text-[10px] font-semibold text-text-muted">{index + 1}</span>
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-[11px] font-semibold text-text-primary">{source.name || source.path || source.url}</span>
                    <span class="mt-0.5 block truncate font-mono text-[9px] text-text-muted">{sourceSubtitle(source)}</span>
                  </span>
                  {#if source.kind === "gitRepo"}
                    {@const status = sourceStatus(source)}
                    <span class="flex shrink-0 items-center gap-1 text-text-muted">
                      {#if busySourceId === source.id}
                        <RefreshCw size={14} class="animate-spin" aria-label="Syncing source" />
                      {:else if status?.error}
                        <CloudOff size={14} class="text-red" aria-label="Git source error" title={status.error} />
                      {:else if !status?.checkedOut}
                        <GitBranch size={14} class="text-text-muted" aria-label="Not cloned" title="Not cloned" />
                      {:else}
                        {#if status.dirty}
                          <FilePenLine size={14} class="text-yellow" aria-label="Dirty checkout" title="Uncommitted local changes" />
                        {/if}
                        {#if status.remoteState === "upToDate"}
                          <Check size={14} class="text-green" aria-label="Up to date" title={gitStatusTitle(status)} />
                        {:else if status.remoteState === "behind"}
                          <ArrowDown size={14} class="text-blue" aria-label="Behind remote" title={gitStatusTitle(status)} />
                        {:else if status.remoteState === "ahead"}
                          <ArrowUp size={14} class="text-blue" aria-label="Ahead of remote" title={gitStatusTitle(status)} />
                        {:else if status.remoteState === "diverged"}
                          <GitCompareArrows size={14} class="text-red" aria-label="Diverged from remote" title={gitStatusTitle(status)} />
                        {:else}
                          <GitBranch size={14} aria-label="Unknown Git status" title={gitStatusTitle(status)} />
                        {/if}
                        {#if status.behindDefault}
                          <span class="flex items-center gap-0.5 text-yellow" title={`${status.behindDefault} behind ${status.defaultBranch ?? "default branch"}`}>
                            <History size={14} aria-label="Behind default branch" />
                            <span class="font-mono text-[9px]">{status.behindDefault}</span>
                          </span>
                        {/if}
                      {/if}
                    </span>
                  {/if}
                  <span class="shrink-0 rounded bg-bg-active px-1.5 py-0.5 font-mono text-[10px] text-text-muted">
                    {itemCountForLayer(source.kind, source.id)}
                  </span>
                  </div>
                  <div class="mt-2 flex flex-wrap items-center gap-1 pl-7">
                    {#if source.kind === "gitRepo"}
                      {#if sourceStatus(source)?.checkedOut}
                        <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-text-primary disabled:opacity-30" disabled={busySourceId === source.id} onclick={() => syncSource(source)}>Sync</button>
                      {:else}
                        <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-text-primary disabled:opacity-30" disabled={busySourceId === source.id} onclick={() => cloneSource(source)}>Clone</button>
                      {/if}
                    {/if}
                    <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-text-primary" onclick={() => toggleSource(source)}>{source.enabled ? "Disable" : "Enable"}</button>
                    <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-text-primary disabled:opacity-30" disabled={index === 0} onclick={() => moveSource(index, -1)}>Up</button>
                    <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-text-primary disabled:opacity-30" disabled={index === sources.length - 1} onclick={() => moveSource(index, 1)}>Down</button>
                    <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:border-red/50 hover:text-red" onclick={() => removeSource(source.id)}>Remove</button>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}

          <div class="mb-2 flex gap-1">
            <input
              class="min-w-0 flex-1 rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
              placeholder="/path/to/repo"
              bind:value={repoDraft}
            />
            <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 text-[10px] text-text-secondary hover:text-text-primary" onclick={browsePinnedRepo}>Browse</button>
            <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 text-[10px] text-text-secondary hover:text-text-primary" onclick={() => addPinnedRepo(repoDraft)}>Add Local</button>
          </div>

          <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1.25fr)] gap-1 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.25fr)_72px_auto]">
            <input
              class="min-w-0 rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
              placeholder="Name"
              bind:value={gitNameDraft}
            />
            <input
              class="min-w-0 rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
              placeholder="Git URL"
              bind:value={gitUrlDraft}
            />
            <input
              class="min-w-0 rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
              placeholder="main"
              bind:value={gitBranchDraft}
            />
            <button type="button" class="rounded border border-border-subtle bg-bg-elevated px-2 text-[10px] text-text-secondary hover:text-text-primary" onclick={addGitSource}>Add Git</button>
          </div>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .library-prose :global(h1),
  .library-prose :global(h2),
  .library-prose :global(h3) {
    margin: 0.75rem 0 0.4rem;
    font-weight: 700;
    color: var(--color-text-primary);
  }
  .library-prose :global(p),
  .library-prose :global(li) {
    font-size: 0.8125rem;
    line-height: 1.6;
    color: var(--color-text-primary);
  }
  .library-prose :global(pre) {
    overflow-x: auto;
    border: 1px solid var(--color-border-subtle);
    background: var(--color-bg-deep);
    padding: 0.75rem;
  }
  .library-prose :global(code) {
    font-family: var(--font-mono);
    font-size: 0.75rem;
  }
</style>
