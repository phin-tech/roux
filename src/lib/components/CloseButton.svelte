<script lang="ts">
  import X from "@lucide/svelte/icons/x";

  interface Props {
    label?: string;
    title?: string;
    onclick: (event: MouseEvent) => void;
    size?: number;
    class?: string;
    tabindex?: number;
  }

  let {
    label = "Close",
    title = label,
    onclick,
    size = 14,
    class: className = "",
    tabindex,
  }: Props = $props();
</script>

<button
  type="button"
  class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 {className}"
  {title}
  aria-label={label}
  {tabindex}
  {onclick}
>
  <X {size} />
</button>
