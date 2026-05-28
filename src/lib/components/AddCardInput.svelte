<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";

  interface Props {
    onCreate: () => void | Promise<void>;
  }

  let { onCreate }: Props = $props();
</script>

<button
  type="button"
  class="inline-flex h-7 w-full items-center gap-1.5 rounded px-2 text-left text-[11px] text-text-muted/70 transition-colors hover:bg-surface-2 hover:text-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
  onclick={onCreate}
  aria-label="Add card"
>
  <Plus size={12} strokeWidth={2.2} />
  <span>Add card</span>
</button>
