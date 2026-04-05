<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { scale } from "svelte/transition";
  import { marked } from "marked";
  import { listDocs, readFile, type DocFile } from "$lib/tauri";
  import { activeSession } from "$lib/stores/sessions";

  interface Props {
    docPath: string;
    onClose: () => void;
  }

  let { docPath, onClose }: Props = $props();

  let docs = $state<DocFile[]>([]);
  let currentPath = $state("");
  let renderedHtml = $state("");
  let loading = $state(true);
  let hovering = $state(false);
  let showPicker = $state(false);
  let refreshTimer: ReturnType<typeof setInterval> | null = null;
  let lastPropPath = $state("");

  async function loadContent() {
    if (!currentPath) return;
    try {
      const content = await readFile(currentPath);
      renderedHtml = await marked(content);
    } catch (e) {
      renderedHtml = `<p class="text-red">Failed to read file: ${e}</p>`;
    }
    loading = false;
  }

  async function refreshDocs() {
    const session = $activeSession;
    if (!session) {
      docs = [];
      loading = false;
      return;
    }
    try {
      docs = await listDocs(session.worktreePath);
    } catch {
      docs = [];
    }
  }

  function selectDoc(doc: DocFile) {
    currentPath = doc.path;
    showPicker = false;
    loading = true;
  }

  function selectFirstDoc() {
    if (docs[0]) {
      selectDoc(docs[0]);
    }
  }

  function formatTime(epoch: number): string {
    if (!epoch) return "";
    const d = new Date(epoch * 1000);
    const now = Date.now();
    const diff = now - d.getTime();
    if (diff < 60_000) return "just now";
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return d.toLocaleDateString();
  }

  function currentFileName(): string {
    if (!currentPath) return "No file";
    const parts = currentPath.split("/");
    return parts[parts.length - 1];
  }

  function relativePath(path: string): string {
    const match = docs.find((doc) => doc.path === path);
    return match?.relativePath ?? currentFileName();
  }

  function fileDirectory(path: string): string {
    const rel = relativePath(path);
    const parts = rel.split("/");
    parts.pop();
    return parts.join("/") || "workspace root";
  }

  onMount(() => {
    refreshDocs();
    refreshTimer = setInterval(() => {
      if (currentPath) void loadContent();
    }, 3000);
  });

  // React to docPath prop changes
  $effect(() => {
    if (docPath && docPath !== lastPropPath) {
      lastPropPath = docPath;
      currentPath = docPath;
      loading = true;
    }
  });

  $effect(() => {
    if (!currentPath) return;
    loading = true;
    void loadContent();
  });

  $effect(() => {
    if (!currentPath) {
      loading = false;
    }
  });

  onDestroy(() => {
    if (refreshTimer) clearInterval(refreshTimer);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="relative flex h-full w-full flex-col bg-zinc-950/96"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
>
  <!-- Header bar -->
  <div class="flex shrink-0 items-start justify-between border-b border-zinc-800/50 bg-zinc-950/90 px-4 py-3 backdrop-blur-sm">
    <div class="relative">
      <button
        class="flex cursor-pointer items-start gap-2 rounded-xl border border-zinc-800/70 bg-zinc-900/80 px-3 py-2 text-left transition-colors hover:bg-white/[0.05] hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
        onclick={() => { showPicker = !showPicker; if (showPicker) refreshDocs(); }}
      >
        <span class="pt-0.5 opacity-45">&#128196;</span>
        <span class="min-w-0 flex-1">
          <span class="block max-w-[220px] truncate text-[12px] font-medium {currentPath ? 'text-zinc-100' : 'text-zinc-500'}">{currentFileName()}</span>
          <span class="mt-0.5 block max-w-[220px] truncate font-mono text-[10px] text-zinc-600">{currentPath ? fileDirectory(currentPath) : "Select a document"}</span>
        </span>
        <span class="ml-1 pt-1 text-[10px] text-zinc-600">&#9662;</span>
      </button>

      {#if showPicker}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="app-scrollbar absolute left-0 top-full z-20 mt-2 max-h-[320px] w-[340px] overflow-y-auto rounded-2xl border border-zinc-800/70 bg-zinc-950/96 p-1 shadow-[0_20px_48px_rgba(2,6,23,0.5)] backdrop-blur-md"
          onclick={(e) => e.stopPropagation()}
          transition:scale={{ duration: 120, start: 0.98 }}
        >
          {#if docs.length === 0}
            <div class="px-3 py-3 text-xs text-zinc-600">No documents found</div>
          {:else}
            {#each docs as doc (doc.path)}
              <button
                class="flex w-full cursor-pointer items-start gap-3 rounded-xl border border-transparent px-3 py-2 text-left transition-colors
                  {currentPath === doc.path
                    ? 'bg-white/[0.05] text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]'
                    : 'bg-transparent text-zinc-300 hover:bg-white/[0.05] hover:text-zinc-100'}"
                onclick={() => selectDoc(doc)}
              >
                <span class="pt-0.5 opacity-45">&#128196;</span>
                <div class="min-w-0 flex-1">
                  <div class="truncate text-[12px] font-medium text-zinc-100">{doc.relativePath.split("/").pop() ?? doc.relativePath}</div>
                  <div class="mt-0.5 truncate font-mono text-[10px] text-zinc-600">{fileDirectory(doc.path)}</div>
                </div>
                <span class="shrink-0 pt-0.5 text-[10px] text-zinc-600">{formatTime(doc.modified)}</span>
              </button>
            {/each}
          {/if}
        </div>
      {/if}
    </div>

    <div class="flex items-center gap-1">
      <button
        class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-xs text-zinc-600 hover:bg-white/[0.05] hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
        onclick={loadContent}
        title="Refresh"
      >&#8635;</button>
    </div>
  </div>

  <!-- Content -->
  <div class="app-scrollbar flex-1 overflow-y-auto px-6 py-5">
    {#if loading}
      <div class="text-xs text-zinc-600">Loading...</div>
    {:else if !currentPath}
      <div class="flex h-full flex-col items-center justify-center gap-4 text-center">
        <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-zinc-800/70 bg-zinc-900/80 text-sky-300 shadow-[0_18px_44px_rgba(2,6,23,0.35)]">
          <svg viewBox="0 0 24 24" class="h-8 w-8 fill-none stroke-current stroke-[1.4]">
            <path d="M7 4.75h7.5l4.5 4.5v10A1.75 1.75 0 0 1 17.25 21h-10.5A1.75 1.75 0 0 1 5 19.25v-12.5A1.75 1.75 0 0 1 6.75 5Z" />
            <path d="M14.5 4.75v4.5H19" />
            <path d="M8.5 13h7" />
            <path d="M8.5 16.25h4.5" />
          </svg>
        </div>
        <div class="space-y-1">
          <p class="text-base font-semibold tracking-tight text-zinc-100">Select a document to preview</p>
          <p class="text-sm text-zinc-500">Choose a markdown file from this session to open the documentation pane.</p>
        </div>
        <button
          class="rounded-full border border-sky-400/20 bg-sky-500/10 px-4 py-2 text-xs font-medium text-sky-200 transition-colors hover:bg-sky-500/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
          onclick={() => { refreshDocs(); showPicker = true; }}
        >
          Browse docs
        </button>
        {#if docs.length > 0}
          <button
            class="text-xs font-medium text-zinc-600 underline underline-offset-4 hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
            onclick={selectFirstDoc}
          >
            Open latest available file
          </button>
        {/if}
      </div>
    {:else}
      <div class="doc-prose">
        {@html renderedHtml}
      </div>
    {/if}
  </div>

  <!-- Close button on hover -->
  {#if hovering}
    <button
      class="absolute right-2 top-2 z-10 flex h-7 w-7 items-center justify-center rounded-full border border-zinc-800/70 bg-zinc-900/85 text-xs leading-none text-zinc-500 backdrop-blur-sm hover:bg-zinc-800 hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
      onclick={onClose}
      title="Close pane"
    >
      &times;
    </button>
  {/if}
</div>

<style>
  .doc-prose :global(h1) {
    font-size: 1.5rem;
    font-weight: 700;
    margin: 0 0 1rem 0;
    color: var(--color-text-primary);
    border-bottom: 1px solid var(--color-border-subtle);
    padding-bottom: 0.5rem;
  }
  .doc-prose :global(h2) {
    font-size: 1.2rem;
    font-weight: 600;
    margin: 1.5rem 0 0.75rem 0;
    color: var(--color-text-primary);
  }
  .doc-prose :global(h3) {
    font-size: 1rem;
    font-weight: 600;
    margin: 1.25rem 0 0.5rem 0;
    color: var(--color-text-primary);
  }
  .doc-prose :global(h4),
  .doc-prose :global(h5),
  .doc-prose :global(h6) {
    font-size: 0.875rem;
    font-weight: 600;
    margin: 1rem 0 0.5rem 0;
    color: var(--color-text-secondary);
  }
  .doc-prose :global(p) {
    margin: 0.5rem 0;
    line-height: 1.65;
    font-size: 0.8125rem;
    color: var(--color-text-primary);
  }
  .doc-prose :global(a) {
    color: var(--color-accent);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .doc-prose :global(a:hover) {
    color: var(--color-blue);
  }
  .doc-prose :global(strong) {
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .doc-prose :global(code) {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    background: var(--color-bg-elevated);
    padding: 0.15rem 0.35rem;
    border-radius: 3px;
    color: var(--color-amber);
  }
  .doc-prose :global(pre) {
    background: var(--color-bg-deep);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.75rem 1rem;
    overflow-x: auto;
    margin: 0.75rem 0;
  }
  .doc-prose :global(pre code) {
    background: none;
    padding: 0;
    font-size: 0.75rem;
    color: var(--color-text-primary);
  }
  .doc-prose :global(ul),
  .doc-prose :global(ol) {
    margin: 0.5rem 0;
    padding-left: 1.5rem;
    font-size: 0.8125rem;
    line-height: 1.65;
  }
  .doc-prose :global(li) {
    margin: 0.25rem 0;
  }
  .doc-prose :global(blockquote) {
    border-left: 3px solid var(--color-accent-dim);
    margin: 0.75rem 0;
    padding: 0.25rem 0.75rem;
    color: var(--color-text-secondary);
    background: var(--color-bg-elevated);
    border-radius: 0 4px 4px 0;
  }
  .doc-prose :global(hr) {
    border: none;
    border-top: 1px solid var(--color-border-subtle);
    margin: 1.5rem 0;
  }
  .doc-prose :global(table) {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.75rem;
    margin: 0.75rem 0;
  }
  .doc-prose :global(th),
  .doc-prose :global(td) {
    border: 1px solid var(--color-border);
    padding: 0.4rem 0.6rem;
    text-align: left;
  }
  .doc-prose :global(th) {
    background: var(--color-bg-elevated);
    font-weight: 600;
  }
  .doc-prose :global(img) {
    max-width: 100%;
    border-radius: 6px;
  }
</style>
