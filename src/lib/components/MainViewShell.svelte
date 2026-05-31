<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    subtitle?: string | null;
    closeLabel: string;
    onclose: () => void;
    children?: Snippet;
  }

  let { title, subtitle = null, closeLabel, onclose, children }: Props = $props();
  let root = $state<HTMLDivElement | null>(null);

  $effect(() => {
    root?.focus();
  });
</script>

<div
  bind:this={root}
  class="absolute inset-0 z-30 flex min-h-0 flex-col bg-bg-deep"
  data-main-view-root
  tabindex="-1"
>
  <div class="flex h-9 shrink-0 items-center justify-between gap-3 border-b border-hairline bg-bg-surface/30 px-3">
    <div class="flex min-w-0 items-baseline gap-2">
      <span class="truncate text-sm font-semibold tracking-tight text-text-primary">{title}</span>
      {#if subtitle}
        <span class="truncate text-[11px] text-text-muted">{subtitle}</span>
      {/if}
    </div>
    <button
      type="button"
      class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-text-muted transition-colors hover:bg-surface-2 hover:text-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
      onclick={onclose}
      aria-label={closeLabel}
      title={`${closeLabel} (Esc)`}
    >
      <X size={14} />
    </button>
  </div>

  <div class="min-h-0 flex-1 overflow-hidden">
    {@render children?.()}
  </div>
</div>
