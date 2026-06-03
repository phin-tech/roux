<script lang="ts">
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";

  interface Props {
    pinned: boolean;
    ontoggle: () => void;
  }

  let { pinned, ontoggle }: Props = $props();
</script>

<button
  type="button"
  class="cursor-pointer rounded border border-transparent bg-transparent p-1 transition-colors hover:border-border-subtle hover:bg-bg-hover {pinned
    ? 'text-accent hover:text-accent'
    : 'text-text-muted hover:text-text-primary'}"
  title={pinned
    ? "Unpin — panel will close when another opens"
    : "Pin — keep this visible alongside another panel"}
  aria-label={pinned ? "Unpin panel" : "Pin panel"}
  aria-pressed={pinned}
  onclick={ontoggle}
>
  {#if pinned}
    <PinOff size={14} />
  {:else}
    <Pin size={14} />
  {/if}
</button>
