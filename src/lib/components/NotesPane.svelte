<script lang="ts">
  import type { NotesScope } from "$lib/tauri";
  import { updateInstance } from "$lib/panes/instances";
  import NotesContent from "./NotesContent.svelte";

  interface Props {
    paneId: string;
    sessionId: string;
    projectId: string | null;
    projectName: string | null;
    repoRoot: string | null;
    scope: NotesScope;
    viewMode: "edit" | "read";
  }

  let {
    paneId,
    sessionId,
    projectId,
    projectName,
    repoRoot,
    scope,
    viewMode,
  }: Props = $props();

  function handleScopeChange(next: NotesScope) {
    updateInstance(paneId, { notesScope: next });
  }

  function handleViewModeChange(mode: "edit" | "read") {
    updateInstance(paneId, { notesViewMode: mode });
  }
</script>

<div class="flex h-full flex-col overflow-hidden bg-bg-deep">
  <NotesContent
    {sessionId}
    {projectId}
    {projectName}
    {repoRoot}
    {scope}
    {viewMode}
    onScopeChange={handleScopeChange}
    onViewModeChange={handleViewModeChange}
  />
</div>
