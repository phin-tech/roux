<script lang="ts">
  import { externalToolRuns, restartExternalToolRun } from "$lib/stores/externalTools";
  import ExternalToolTerminalView from "./ExternalToolTerminalView.svelte";
  import ExternalToolWebView from "./ExternalToolWebView.svelte";

  interface Props {
    runId: string;
  }

  let { runId }: Props = $props();
  let run = $derived($externalToolRuns.get(runId) ?? null);
</script>

{#if !run}
  <div class="flex h-full items-center justify-center text-sm text-text-muted">
    External tool run no longer available
  </div>
{:else if run.surface === "web"}
  <ExternalToolWebView {run} />
{:else if run.status === "error"}
  <div class="flex h-full items-center justify-center bg-bg-deep p-6">
    <div class="w-full max-w-2xl rounded border border-red/30 bg-red/10 p-4">
      <div class="text-sm font-semibold text-red">Failed to launch {run.toolName}</div>
      <div class="mt-2 whitespace-pre-wrap break-words font-mono text-[11px] text-text-secondary">
        {run.error}
      </div>
      <button
        type="button"
        class="mt-4 rounded border border-border-subtle bg-bg-elevated px-3 py-1.5 text-xs text-text-primary hover:bg-bg-hover"
        onclick={() => void restartExternalToolRun(runId)}
      >
        Retry
      </button>
    </div>
  </div>
{:else}
  <ExternalToolTerminalView {run} />
{/if}
