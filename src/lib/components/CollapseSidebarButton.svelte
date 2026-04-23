<script lang="ts">
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import { sidebarLayout } from "$lib/stores/sidebarLayout";

  interface Props {
    label?: string;
    title?: string;
    onclick: (event: MouseEvent) => void;
    class?: string;
  }

  let {
    label = "Collapse sidebar",
    title = label,
    onclick,
    class: className = "",
  }: Props = $props();
</script>

<button
  type="button"
  class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 {className}"
  {title}
  aria-label={label}
  onclick={onclick}
>
  {#if $sidebarLayout.railSide === "right"}
    <ChevronsRight size={14} />
  {:else}
    <ChevronsLeft size={14} />
  {/if}
</button>
