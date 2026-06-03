<script lang="ts">
  import { notesRead, notesWrite, type NotesScope } from "$lib/tauri";
  import { onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    sessionId: string;
    projectId: string | null;
    projectName: string | null;
    repoRoot: string | null;
    scope: NotesScope;
    viewMode: "edit" | "read";
    onScopeChange: (scope: NotesScope) => void;
    onViewModeChange: (mode: "edit" | "read") => void;
  }

  let {
    sessionId,
    projectId,
    projectName,
    repoRoot,
    scope,
    viewMode,
    onScopeChange,
    onViewModeChange,
  }: Props = $props();

  let content = $state("");
  let loadedKey = $state<string | null>(null);
  let loadedPath = $state<string | null>(null);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const projectEnabled = $derived(!!projectId);
  const repoEnabled = $derived(!!repoRoot);

  // Include all fields that affect note resolution to ensure refetch on any change
  const fetchKey = $derived(
    `${sessionId}::${scope}::${repoRoot ?? ""}::${projectId ?? ""}`,
  );
  const blocks = $derived(parseEntries(content));

  function scopeHeaderLabel(): string {
    switch (scope) {
      case "global":
        return "Global notes";
      case "project":
        return projectName ? `${projectName} — Project notes` : "Project notes";
      case "repo":
        return "Repo notes";
      case "session":
        return "Session notes";
    }
  }

  function toggleViewMode() {
    onViewModeChange(viewMode === "read" ? "edit" : "read");
  }

  // Load on (session, scope, repoRoot, projectId) change.
  $effect(() => {
    if (!fetchKey) return;
    if (fetchKey === loadedKey) return;

    // Capture key at request time to detect stale responses
    const requestKey = fetchKey;
    const target = {
      scope,
      sessionId,
      topic: null as string | null,
      overrideSlug: null as string | null,
    };
    notesRead(target)
      .then((read) => {
        // Discard stale response if scope/session changed while request was in flight
        if (requestKey !== fetchKey) return;
        content = stripFrontmatter(read.content);
        loadedKey = requestKey;
        loadedPath = read.path;
      })
      .catch((err) => {
        console.error("[NotesContent] Failed to load notes:", err);
      });
  });

  // Live reload: backend emits `notes-changed` after any Tauri/CLI write.
  // Re-fetch the file if the event references the one we're displaying.
  onMount(() => {
    let unlistenChange: UnlistenFn | null = null;
    let mounted = true;

    listen<{ path: string }>("notes-changed", async (ev) => {
      if (!loadedPath || ev.payload.path !== loadedPath) return;
      const target = {
        scope,
        sessionId,
        topic: null as string | null,
        overrideSlug: null as string | null,
      };
      try {
        const read = await notesRead(target);
        if (mounted) {
          content = stripFrontmatter(read.content);
        }
      } catch (err) {
        console.error("[NotesContent] Failed to reload notes:", err);
      }
    }).then((fn) => {
      if (mounted) {
        unlistenChange = fn;
      } else {
        // Component unmounted before listener was ready - clean up immediately
        fn();
      }
    });

    return () => {
      mounted = false;
      if (unlistenChange) unlistenChange();
    };
  });

  function selectScope(next: NotesScope) {
    if (next === "project" && !projectEnabled) return;
    if (next === "repo" && !repoEnabled) return;
    onScopeChange(next);
  }

  function onInput(e: Event) {
    const value = (e.target as HTMLTextAreaElement).value;
    content = value;
    const currentScope = scope;
    const currentSession = sessionId;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      notesWrite(
        {
          scope: currentScope,
          sessionId: currentSession,
          topic: null,
          overrideSlug: null,
        },
        value,
        [],
      ).catch((err) => {
        console.error("[NotesContent] Failed to save notes:", err);
      });
    }, 500);
  }

  function stripFrontmatter(raw: string): string {
    // Normalize CRLF to LF for consistent parsing
    const normalized = raw.replace(/\r\n/g, "\n");
    if (!normalized.startsWith("---\n")) return normalized;
    const rest = normalized.slice(4);
    const idx = rest.indexOf("\n---\n");
    if (idx < 0) return normalized;
    return rest.slice(idx + "\n---\n".length);
  }

  type Block =
    | { kind: "prose"; text: string }
    | { kind: "entry"; timestamp: string; body: string; id: string | null };

  /**
   * Parse the body into a sequence of blocks for the read view.
   * Recognizes the append-with-timestamp shape (HTML anchor + `## YYYY-MM-DD HH:MM`
   * + body + `^entry-<id>` block ref) and collapses each into a single
   * structured block, hiding both markers. Anything between or outside
   * entries is carried through as-is in a "prose" block.
   */
  function parseEntries(src: string): Block[] {
    if (!src.trim()) return [];
    const entryRe =
      /(?:\n{0,2}<a id="entry-[a-f0-9]{6,32}"><\/a>\n\n)?## (\d{4}-\d{2}-\d{2} \d{2}:\d{2})\n\n([\s\S]*?)\n\n\^entry-([a-f0-9]{6,32})\n?/g;

    const out: Block[] = [];
    let lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = entryRe.exec(src)) !== null) {
      if (m.index > lastIndex) {
        const prose = src.slice(lastIndex, m.index);
        if (prose.trim()) out.push({ kind: "prose", text: prose.trim() });
      }
      out.push({ kind: "entry", timestamp: m[1], body: m[2], id: m[3] });
      lastIndex = m.index + m[0].length;
    }
    if (lastIndex < src.length) {
      const tail = src.slice(lastIndex);
      if (tail.trim()) out.push({ kind: "prose", text: tail.trim() });
    }
    return out;
  }
</script>

<div class="flex h-full flex-col overflow-hidden">
  <!-- Toolbar: scope header + view mode toggle -->
  <div
    class="flex h-9 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3"
  >
    <div class="flex items-center gap-2 min-w-0">
      <span class="text-sm font-semibold tracking-tight truncate"
        >{scopeHeaderLabel()}</span
      >
      <span
        class="shrink-0 rounded-full border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider text-amber-400"
        title="This feature is experimental. Vault layout, CLI flags, env var names, and frontmatter schema may change. See docs/features/notes.md."
        >Experimental</span
      >
    </div>
    <button
      class="cursor-pointer rounded-lg border border-transparent bg-transparent px-2 py-0.5 text-[11px] font-medium text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
      onclick={toggleViewMode}
      title={viewMode === "read"
        ? "Switch to raw editor (Edit)"
        : "Switch to rendered view (Read)"}
      >{viewMode === "read" ? "Edit" : "Read"}</button
    >
  </div>

  <!-- Scope selector pills -->
  <div
    role="tablist"
    class="flex shrink-0 gap-1 border-b border-hairline bg-bg-surface/20 px-2 py-1.5"
  >
    {@render pill("session", "Session", true)}
    {@render pill("repo", "Repo", repoEnabled, "No repo root for this session")}
    {@render pill(
      "project",
      "Project",
      projectEnabled,
      "No project assigned to this session",
    )}
    {@render pill("global", "Global", true)}
  </div>

  <!-- Content area -->
  {#if viewMode === "edit"}
    <textarea
      class="flex-1 resize-none border-none bg-bg-deep px-4 py-3 font-mono text-sm text-text-primary outline-none placeholder:text-text-muted/50"
      placeholder="Write notes here..."
      value={content}
      oninput={onInput}
    ></textarea>
  {:else if blocks.length === 0}
    <div
      class="flex flex-1 items-center justify-center px-6 text-center text-sm text-text-secondary"
    >
      <div>
        No notes yet.<br />
        <button
          class="mt-2 text-xs text-text-muted underline hover:text-text-primary"
          onclick={toggleViewMode}>Switch to editor</button
        >
        or append from a session with
        <code class="ml-1 rounded bg-bg-surface/60 px-1 py-0.5 text-[11px]"
          >roux notes {scope} append --timestamp</code
        >.
      </div>
    </div>
  {:else}
    <div class="flex-1 overflow-y-auto px-3 py-2 text-sm text-text-primary">
      {#each blocks as block}
        {#if block.kind === "entry"}
          <div
            class="mb-3 rounded-lg border border-hairline/60 bg-bg-surface/30 px-3 py-2"
          >
            <div
              class="mb-1 flex items-center gap-2 text-[11px] font-medium uppercase tracking-wider text-text-muted"
            >
              <span>{block.timestamp}</span>
            </div>
            <div
              class="whitespace-pre-wrap font-mono text-[13px] text-text-primary"
            >
              {block.body}
            </div>
          </div>
        {:else}
          <div
            class="mb-3 whitespace-pre-wrap font-mono text-[13px] text-text-primary"
          >
            {block.text}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

{#snippet pill(
  target: NotesScope,
  label: string,
  enabled: boolean,
  disabledReason: string = "",
)}
  <button
    role="tab"
    aria-selected={scope === target}
    disabled={!enabled}
    title={enabled ? undefined : disabledReason}
    class="cursor-pointer rounded-full px-2.5 py-1 text-xs font-medium transition-colors
      {scope === target
      ? 'bg-bg-hover text-text-primary border border-border-subtle'
      : 'border border-transparent text-text-muted hover:border-border-subtle hover:text-text-primary'}
      disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-transparent
      disabled:hover:text-text-muted"
    onclick={() => selectScope(target)}>{label}</button
  >
{/snippet}
