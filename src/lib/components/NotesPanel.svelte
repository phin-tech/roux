<script lang="ts">
  import { notesRead, notesWrite, type NotesScope } from "$lib/tauri";
  import { notesUiState, setLastNotesScope } from "$lib/stores/notesUi";

  interface Props {
    visible: boolean;
    sessionId: string | null;
    projectId: string | null;
    projectName: string | null;
    repoRoot: string | null;
    onclose: () => void;
  }

  let {
    visible,
    sessionId,
    projectId,
    projectName,
    repoRoot,
    onclose,
  }: Props = $props();

  let scope = $state<NotesScope>("session");
  let content = $state("");
  let loadedKey = $state<string | null>(null);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const projectEnabled = $derived(!!projectId);
  const repoEnabled = $derived(!!repoRoot);

  // Build a stable key (scope + session) so we reload the file when either
  // changes. We load via Tauri, which strips frontmatter on display below.
  const fetchKey = $derived(sessionId ? `${sessionId}::${scope}` : null);

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

  // Scope is derived from the notesUi store so that a command-palette
  // command like "Show Repo Notes" updates the panel's active scope
  // immediately — even while the panel is already open.
  $effect(() => {
    if (!sessionId) return;
    const raw = $notesUiState.lastScopeBySession[sessionId] ?? "session";
    const next: NotesScope =
      raw === "project" && !projectEnabled
        ? "session"
        : raw === "repo" && !repoEnabled
          ? "session"
          : raw;
    if (scope !== next) scope = next;
  });
  $effect(() => {
    if (!visible) loadedKey = null;
  });

  // Load the correct file whenever the (session, scope) pair changes.
  $effect(() => {
    if (!visible || !sessionId || !fetchKey) return;
    if (fetchKey === loadedKey) return;

    const target = {
      scope,
      sessionId,
      topic: null as string | null,
      overrideSlug: null as string | null,
    };
    notesRead(target).then((read) => {
      content = stripFrontmatter(read.content);
      loadedKey = fetchKey;
    });
  });

  function selectScope(next: NotesScope) {
    if (next === "project" && !projectEnabled) return;
    if (next === "repo" && !repoEnabled) return;
    scope = next;
    if (sessionId) setLastNotesScope(sessionId, next);
  }

  function onInput(e: Event) {
    const value = (e.target as HTMLTextAreaElement).value;
    content = value;
    if (!sessionId) return;
    const currentScope = scope;
    const currentSession = sessionId;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      notesWrite(
        { scope: currentScope, sessionId: currentSession, topic: null, overrideSlug: null },
        value,
        [],
      );
    }, 500);
  }

  /** Obsidian vault files carry YAML frontmatter; for the plain-text editor
   * we hide it so the user edits the body only. On save the backend
   * preserves frontmatter automatically. */
  function stripFrontmatter(raw: string): string {
    if (!raw.startsWith("---\n")) return raw;
    const rest = raw.slice(4);
    const idx = rest.indexOf("\n---\n");
    if (idx < 0) return raw;
    return rest.slice(idx + "\n---\n".length);
  }
</script>

<div
  style="right: {visible ? '0.5rem' : '-400px'}; visibility: {visible ? 'visible' : 'hidden'};"
  class="absolute top-2 bottom-2 z-50 flex w-[380px] flex-col rounded-2xl border border-hairline bg-bg-deep shadow-[-8px_8px_48px_rgba(2,6,23,0.55),0_0_0_1px_rgba(255,255,255,0.04)] transition-[right] duration-250"
>
  <div class="flex h-9 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3 rounded-t-2xl">
    <div class="flex items-center gap-2 min-w-0">
      <span class="text-sm font-semibold tracking-tight truncate">{scopeHeaderLabel()}</span>
      <span
        class="shrink-0 rounded-full border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider text-amber-400"
        title="This feature is experimental. Vault layout, CLI flags, env var names, and frontmatter schema may change. See docs/features/notes.md."
      >Experimental</span>
    </div>
    <button
      class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-base text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
      onclick={onclose}
      aria-label="Close notes"
    >&times;</button>
  </div>

  {#if sessionId}
    <div role="tablist" class="flex shrink-0 gap-1 border-b border-hairline bg-bg-surface/20 px-2 py-1.5">
      {@render pill("session", "Session", true)}
      {@render pill("repo", "Repo", repoEnabled, "No repo root for this session")}
      {@render pill("project", "Project", projectEnabled, "No project assigned to this session")}
      {@render pill("global", "Global", true)}
    </div>

    <textarea
      class="flex-1 resize-none border-none bg-bg-deep px-4 py-3 font-mono text-sm text-text-primary outline-none placeholder:text-text-muted/50"
      placeholder="Write notes here..."
      value={content}
      oninput={onInput}
    ></textarea>
  {:else}
    <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-text-secondary">
      Open a session to use notes.
    </div>
  {/if}
</div>

{#snippet pill(target: NotesScope, label: string, enabled: boolean, disabledReason: string = "")}
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
    onclick={() => selectScope(target)}
  >{label}</button>
{/snippet}
