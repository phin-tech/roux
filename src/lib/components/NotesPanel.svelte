<script lang="ts">
  import { getProjectNotes, setProjectNotes } from "$lib/tauri";

  interface Props {
    visible: boolean;
    projectId: string | null;
    projectName: string | null;
    onclose: () => void;
  }

  let { visible, projectId, projectName, onclose }: Props = $props();

  let content = $state("");
  let loadedProjectId = $state<string | null>(null);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    if (visible && projectId && projectId !== loadedProjectId) {
      const id = projectId;
      getProjectNotes(id).then((notes) => {
        content = notes;
        loadedProjectId = id;
      });
    }
    if (!visible) {
      loadedProjectId = null;
    }
  });

  function onInput(e: Event) {
    const value = (e.target as HTMLTextAreaElement).value;
    content = value;
    if (!projectId) return;
    const id = projectId;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      setProjectNotes(id, value);
    }, 500);
  }
</script>

<div
  style="right: {visible ? '0.5rem' : '-400px'}; visibility: {visible ? 'visible' : 'hidden'};"
  class="absolute top-2 bottom-2 z-50 flex w-[380px] flex-col rounded-2xl border border-hairline bg-bg-deep shadow-[-8px_8px_48px_rgba(2,6,23,0.55),0_0_0_1px_rgba(255,255,255,0.04)] transition-[right] duration-250"
>
  <div class="flex h-9 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3 rounded-t-2xl">
    <span class="text-sm font-semibold tracking-tight">{projectName ? `${projectName} — Notes` : 'Notes'}</span>
    <button
      class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-base text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
      onclick={onclose}
    >&times;</button>
  </div>

  {#if projectId}
    <textarea
      class="flex-1 resize-none border-none bg-bg-deep px-4 py-3 font-mono text-sm text-text-primary outline-none placeholder:text-text-muted/50"
      placeholder="Write notes here..."
      value={content}
      oninput={onInput}
    ></textarea>
  {:else}
    <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-text-secondary">
      Assign a project to this session to use notes.
    </div>
  {/if}
</div>
