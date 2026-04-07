<script lang="ts">
  import { onMount } from "svelte";

  interface Props {
    docPath?: string;
  }

  let { docPath = "" }: Props = $props();

  let MarkdownPaneComponent = $state<any>(null);
  let loadError = $state<string | null>(null);

  onMount(async () => {
    try {
      MarkdownPaneComponent = (await import("./MarkdownPane.svelte")).default;
    } catch (error) {
      loadError = error instanceof Error ? error.message : String(error);
    }
  });
</script>

{#if MarkdownPaneComponent}
  <MarkdownPaneComponent {docPath} />
{:else if loadError}
  <div class="flex h-full w-full items-center justify-center p-4">
    <div class="max-w-sm rounded-xl border border-red/20 bg-red/8 px-4 py-3 text-sm text-red">
      Failed to load the markdown editor.
      {loadError}
    </div>
  </div>
{:else}
  <div class="flex h-full w-full items-center justify-center p-4">
    <div class="rounded-xl border border-border-subtle bg-bg-surface/60 px-4 py-3 text-sm text-text-muted">
      Loading editor...
    </div>
  </div>
{/if}
