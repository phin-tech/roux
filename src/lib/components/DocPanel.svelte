<script lang="ts">
  import { onDestroy } from "svelte";
  import { marked } from "marked";
  import { listDocs, readFile, type DocFile } from "$lib/tauri";
  import { activeSession } from "$lib/stores/sessions";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();

  let docs = $state<DocFile[]>([]);
  let selectedDoc = $state<DocFile | null>(null);
  let renderedHtml = $state("");
  let loading = $state(false);
  let pollInterval: ReturnType<typeof setInterval> | null = null;

  async function refreshDocs() {
    const session = $activeSession;
    if (!session) {
      docs = [];
      return;
    }
    try {
      docs = await listDocs(session.worktreePath);
    } catch {
      docs = [];
    }
  }

  async function selectDoc(doc: DocFile) {
    selectedDoc = doc;
    loading = true;
    try {
      const content = await readFile(doc.path);
      renderedHtml = await marked(content);
    } catch (e) {
      renderedHtml = `<p class="text-red">Failed to read file: ${e}</p>`;
    }
    loading = false;
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

  // Start/stop polling when visibility or session changes
  $effect(() => {
    if (visible) {
      refreshDocs();
      pollInterval = setInterval(refreshDocs, 5000);
    } else {
      if (pollInterval) {
        clearInterval(pollInterval);
        pollInterval = null;
      }
    }
  });

  // Refresh when active session changes
  $effect(() => {
    // Read the session to track it
    void $activeSession;
    if (visible) {
      selectedDoc = null;
      renderedHtml = "";
      refreshDocs();
    }
  });

  onDestroy(() => {
    if (pollInterval) clearInterval(pollInterval);
  });
</script>

<div
  class="absolute top-0 right-0 bottom-0 z-50 flex w-[480px] flex-col border-l border-white/8 bg-bg-surface shadow-[-18px_0_48px_rgba(2,6,23,0.45)] transition-transform duration-250
    {visible ? 'translate-x-0' : 'translate-x-full'}"
>
  <!-- Header -->
  <div class="flex items-center justify-between border-b border-white/6 bg-slate-800/50 px-5 py-4 backdrop-blur-sm">
    <div class="flex items-center gap-2">
      <span class="text-sm font-semibold tracking-tight">Documents</span>
      <span class="rounded-full border border-white/8 bg-bg-elevated px-2 py-0.5 text-[10px] font-medium text-text-muted">
        {docs.length}
      </span>
    </div>
    <div class="flex items-center gap-2">
      <button
        class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-xs text-text-muted hover:border-white/8 hover:bg-bg-hover hover:text-text-primary"
        onclick={refreshDocs}
        title="Refresh"
      >&#8635;</button>
      <button
        class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-base text-text-muted hover:border-white/8 hover:bg-bg-hover hover:text-text-primary"
        onclick={onclose}
      >&times;</button>
    </div>
  </div>

  {#if !$activeSession}
    <div class="flex-1 flex items-center justify-center text-text-muted text-sm">
      No active session
    </div>
  {:else}
    <!-- File list -->
    <div class="app-scrollbar max-h-[220px] overflow-y-auto border-b border-white/6">
      {#if docs.length === 0}
        <div class="px-5 py-4 text-xs text-text-muted">
          No markdown files found in project
        </div>
      {:else}
        {#each docs as doc (doc.path)}
          <button
            class="flex w-full cursor-pointer items-center gap-2 px-4 py-2 text-left text-xs transition-colors
              {selectedDoc?.path === doc.path
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

    <!-- Rendered content -->
    <div class="app-scrollbar flex-1 overflow-y-auto px-6 py-5">
      {#if loading}
        <div class="text-xs text-text-muted">Loading...</div>
      {:else if !selectedDoc}
        <div class="flex h-full flex-col items-center justify-center gap-4 text-center text-text-muted">
          <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-white/8 bg-slate-900/80 text-sky-300 shadow-[0_18px_44px_rgba(2,6,23,0.35)]">
            <span class="text-3xl">&#128196;</span>
          </div>
          <div class="space-y-1">
            <p class="text-base font-semibold tracking-tight text-text-primary">Select a session document</p>
            <p class="text-sm text-text-secondary">Preview markdown notes, specs, and scratch files beside the terminal.</p>
          </div>
          <span class="text-[11px] font-medium uppercase tracking-[0.22em] opacity-70">Cmd+B to toggle</span>
        </div>
      {:else}
        <div class="doc-prose">
          {@html renderedHtml}
        </div>
      {/if}
    </div>
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
  .doc-prose :global(em) {
    font-style: italic;
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
