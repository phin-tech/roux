<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    actions?: Snippet;
  }

  let { title, actions }: Props = $props();
</script>

<div
  class="flex h-9 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3"
>
  <span class="text-sm font-semibold tracking-tight text-text-primary"
    >{title}</span
  >
  {#if actions}
    <div class="flex items-center gap-1">
      {@render actions()}
    </div>
  {/if}
</div>
