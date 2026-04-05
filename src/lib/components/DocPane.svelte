<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { marked } from "marked";
  import { listDocs, readFile, type DocFile } from "$lib/tauri";
  import { activeSession } from "$lib/stores/sessions";

  interface Props {
    docPath: string;
    onClose: () => void;
  }

  let { docPath, onClose }: Props = $props();

  let docs = $state<DocFile[]>([]);
  let currentPath = $state(docPath);
  let renderedHtml = $state("");
  let loading = $state(true);
  let hovering = $state(false);
  let showPicker = $state(false);
  let refreshTimer: ReturnType<typeof setInterval> | null = null;

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
    if (!session) return;
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
    loadContent();
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

  onMount(() => {
    // Ensure currentPath is set from prop
    if (!currentPath && docPath) {
      currentPath = docPath;
    }
    if (currentPath) {
      loadContent();
    }
    refreshDocs();
    refreshTimer = setInterval(() => {
      if (currentPath) loadContent();
    }, 3000);
  });

  // React to docPath prop changes
  $effect(() => {
    if (docPath && docPath !== currentPath) {
      currentPath = docPath;
      loading = true;
      loadContent();
    }
  });

  onDestroy(() => {
    if (refreshTimer) clearInterval(refreshTimer);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="relative w-full h-full flex flex-col bg-bg-surface"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
>
  <!-- Header bar -->
  <div class="px-3 py-2 border-b border-border-subtle flex items-center justify-between shrink-0">
    <div class="relative">
      <button
        class="flex items-center gap-1.5 bg-bg-elevated border border-border-subtle rounded px-2 py-1 text-xs text-text-secondary hover:text-text-primary hover:bg-bg-hover cursor-pointer font-mono transition-colors"
        onclick={() => { showPicker = !showPicker; if (showPicker) refreshDocs(); }}
      >
        <span class="opacity-50">&#128196;</span>
        <span class="truncate max-w-[200px]">{currentFileName()}</span>
        <span class="text-[10px] text-text-muted ml-1">&#9662;</span>
      </button>

      {#if showPicker}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="absolute top-full left-0 mt-1 w-[320px] bg-bg-surface border border-border rounded-md shadow-xl z-20 max-h-[300px] overflow-y-auto scrollbar-thin"
          onclick={(e) => e.stopPropagation()}
        >
          {#if docs.length === 0}
            <div class="px-3 py-2 text-text-muted text-xs">No documents found</div>
          {:else}
            {#each docs as doc (doc.path)}
              <button
                class="w-full text-left px-3 py-2 flex items-center gap-2 text-xs border-none cursor-pointer transition-colors
                  {currentPath === doc.path
                    ? 'bg-bg-active text-text-primary'
                    : 'bg-transparent text-text-secondary hover:bg-bg-hover hover:text-text-primary'}"
                onclick={() => selectDoc(doc)}
              >
                <span class="opacity-50">&#128196;</span>
                <span class="truncate flex-1 font-mono">{doc.relativePath}</span>
                <span class="text-text-muted text-[10px] shrink-0">{formatTime(doc.modified)}</span>
              </button>
            {/each}
          {/if}
        </div>
      {/if}
    </div>

    <div class="flex items-center gap-1">
      <button
        class="bg-transparent border-none text-text-muted cursor-pointer text-xs p-1 rounded hover:text-text-primary hover:bg-bg-hover font-mono"
        onclick={loadContent}
        title="Refresh"
      >&#8635;</button>
    </div>
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-y-auto px-5 py-4 scrollbar-thin">
    {#if loading}
      <div class="text-text-muted text-xs">Loading...</div>
    {:else}
      <div class="doc-prose">
        {@html renderedHtml}
      </div>
    {/if}
  </div>

  <!-- Close button on hover -->
  {#if hovering}
    <button
      class="absolute top-1 right-1 z-10 w-5 h-5 flex items-center justify-center rounded bg-bg-surface/80 text-text-muted hover:text-text-primary hover:bg-bg-surface text-xs leading-none"
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
