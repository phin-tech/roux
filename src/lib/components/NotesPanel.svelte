<script lang="ts">
  import type { NotesScope } from "$lib/tauri";
  import {
    notesUiState,
    setLastNotesScope,
    setNotesViewMode,
    type NotesViewMode,
  } from "$lib/stores/notesUi";
  import NotesContent from "./NotesContent.svelte";

  import PinButton from "./PinButton.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";

  interface Props {
    visible: boolean;
    sessionId: string | null;
    projectId: string | null;
    projectName: string | null;
    repoRoot: string | null;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let {
    visible,
    sessionId,
    projectId,
    projectName,
    repoRoot,
    onclose,
    pinned = false,
    onTogglePin,
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
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <!-- Sidebar-specific close button row -->
  <div class="flex h-9 shrink-0 items-center justify-end gap-1 border-b border-hairline bg-bg-surface/30 px-3">
    {#if onTogglePin}
      <PinButton {pinned} ontoggle={onTogglePin} />
    {/if}
    <CollapseSidebarButton
      onclick={onclose}
      label="Collapse notes sidebar"
      title="Collapse notes sidebar"
    />
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
