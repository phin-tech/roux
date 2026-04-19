<script lang="ts">
  import type { NotesScope } from "$lib/tauri";
  import {
    notesUiState,
    setLastNotesScope,
    setNotesViewMode,
    type NotesViewMode,
  } from "$lib/stores/notesUi";
  import NotesContent from "./NotesContent.svelte";

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

  const viewMode = $derived<NotesViewMode>(
    sessionId ? ($notesUiState.viewModeBySession[sessionId] ?? "read") : "read"
  );

  const projectEnabled = $derived(!!projectId);
  const repoEnabled = $derived(!!repoRoot);

  // Scope is derived from the notesUi store so command-palette actions
  // ("Show Repo Notes", etc.) update the panel even while it's open.
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

  function handleScopeChange(next: NotesScope) {
    scope = next;
    if (sessionId) setLastNotesScope(sessionId, next);
  }

  function handleViewModeChange(mode: "edit" | "read") {
    if (sessionId) setNotesViewMode(sessionId, mode);
  }
</script>

<div
  style="right: {visible ? '0.5rem' : '-400px'}; visibility: {visible ? 'visible' : 'hidden'};"
  class="absolute top-2 bottom-2 z-50 flex w-[380px] flex-col rounded-2xl border border-hairline bg-bg-deep shadow-[-8px_8px_48px_rgba(2,6,23,0.55),0_0_0_1px_rgba(255,255,255,0.04)] transition-[right] duration-250"
>
  <!-- Sidebar-specific close button row -->
  <div class="flex h-9 shrink-0 items-center justify-end border-b border-hairline bg-bg-surface/30 px-3 rounded-t-2xl">
    <button
      class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-base text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
      onclick={onclose}
      aria-label="Close notes"
    >&times;</button>
  </div>

  {#if sessionId}
    <div class="flex flex-1 min-h-0 flex-col overflow-hidden">
      <NotesContent
        {sessionId}
        {projectId}
        {projectName}
        {repoRoot}
        {scope}
        viewMode={viewMode}
        onScopeChange={handleScopeChange}
        onViewModeChange={handleViewModeChange}
      />
    </div>
  {:else}
    <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-text-secondary">
      Open a session to use notes.
    </div>
  {/if}
</div>
