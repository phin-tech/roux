<script lang="ts">
  interface Props {
    error: string;
    workingDir?: string;
    onRetry: () => void;
    onClose: () => void;
  }

  let { error, workingDir, onRetry, onClose }: Props = $props();
</script>

<div class="flex h-full w-full flex-col items-center justify-center gap-4 bg-bg-deep px-6 text-center">
  <div class="flex flex-col gap-3 w-full max-w-sm">
    <p class="text-[11px] uppercase tracking-wider text-text-muted/60">Shell failed to restore</p>

    {#if workingDir}
      <div class="flex flex-col gap-0.5 text-left">
        <span class="text-[10px] uppercase tracking-wider text-text-muted/50">Working directory</span>
        <span class="font-mono text-[11px] text-text-secondary break-all">{workingDir}</span>
      </div>
    {/if}

    <div class="flex flex-col gap-0.5 text-left">
      <span class="text-[10px] uppercase tracking-wider text-text-muted/50">Error</span>
      <span class="font-mono text-[11px] text-red/80 break-all">{error}</span>
    </div>

    <div class="flex gap-2 pt-1">
      <button
        class="flex-1 rounded border border-hairline bg-bg-surface px-3 py-1.5 font-mono text-[11px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={onRetry}
      >
        Retry
      </button>
      <button
        class="flex-1 rounded border border-hairline bg-bg-surface px-3 py-1.5 font-mono text-[11px] text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={onClose}
      >
        Close pane
      </button>
    </div>
  </div>
</div>
