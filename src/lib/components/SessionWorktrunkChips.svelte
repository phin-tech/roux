<script lang="ts">
  import { worktreeMetadataFor } from "$lib/stores/worktreeMetadata";
  import { ciChipFor } from "$lib/ciIcon";

  interface Props {
    /**
     * Absolute path of the session's worktree. Empty string / null is
     * tolerated — the component renders nothing when metadata can't be
     * looked up.
     */
    worktreePath: string | null | undefined;
  }

  let { worktreePath }: Props = $props();

  let metadata = $derived(
    worktreePath ? worktreeMetadataFor(worktreePath) : null,
  );
  let m = $derived(metadata ? $metadata : null);
  let ciChip = $derived(ciChipFor(m?.ciStatus ?? null));
</script>

{#if m}
  {#if m.dirty}
    <span
      data-testid="session-wt-dirty"
      class="text-[10px] leading-none text-yellow"
      title="Uncommitted changes"
      aria-label="Dirty">●</span
    >
  {/if}

  {#if m.ahead > 0 || m.behind > 0}
    <span
      data-testid="session-wt-ahead-behind"
      class="font-mono text-[10px] leading-none text-text-muted"
      title={`${m.ahead} ahead, ${m.behind} behind main`}
    >
      {#if m.ahead > 0}↑{m.ahead}{/if}
      {#if m.ahead > 0 && m.behind > 0}&nbsp;{/if}
      {#if m.behind > 0}↓{m.behind}{/if}
    </span>
  {/if}

  {#if m.locked}
    <span
      data-testid="session-wt-locked"
      class="text-[10px] leading-none text-red"
      title={m.lockReason ? `Locked: ${m.lockReason}` : "Locked"}
      aria-label="Locked">🔒</span
    >
  {/if}

  {#if ciChip}
    {@const Icon = ciChip.icon}
    {@const running = m.ciStatus === "running"}
    <span
      data-testid="session-wt-ci"
      class={`inline-flex items-center leading-none ${ciChip.color} ${m.ciStale ? "opacity-60" : ""}`}
      title={`CI: ${ciChip.label}${m.ciStale ? " (stale)" : ""}`}
      aria-label={`CI ${ciChip.label}`}
    >
      <Icon size={11} class={running ? "animate-spin" : ""} />
    </span>
  {/if}

  {#if m.devServerUrl}
    <a
      data-testid="session-wt-dev-server"
      href={m.devServerUrl}
      target="_blank"
      rel="noreferrer"
      class="font-mono text-[10px] leading-none text-blue underline"
      onclick={(e) => e.stopPropagation()}
      title={`Dev server: ${m.devServerUrl}`}>url</a
    >
  {/if}
{/if}
