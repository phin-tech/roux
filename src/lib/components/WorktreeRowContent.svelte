<script lang="ts">
  import type { Worktree } from "$lib/types";
  import { ciChipFor } from "$lib/ciIcon";
  import { safeHref } from "$lib/safeUrl";
  import {
    prLookupFor,
    lookupPrForRepoBranch,
  } from "$lib/stores/sessionPrLookup";
  import PrStatusPopover from "./PrStatusPopover.svelte";

  interface Props {
    wt: Worktree;
    showPath?: boolean;
    /**
     * Repo root for the worktree's parent repository. When supplied
     * (and the worktree carries a branch), the row triggers a PR
     * lookup on first hover and shows the same Checks/Reviews popover
     * the bottom status bar uses.
     */
    repoRoot?: string | null;
  }

  let { wt, showPath = true, repoRoot = null }: Props = $props();

  let metadata = $derived(wt.worktrunk);
  let ciChip = $derived(ciChipFor(metadata?.ciStatus ?? null));
  let ciHref = $derived(safeHref(metadata?.ciUrl));
  let devServerHref = $derived(safeHref(metadata?.devServerUrl));

  let prStore = $derived(prLookupFor(repoRoot, wt.branch));
  let prInfo = $derived($prStore ?? null);
  let checkRuns = $derived(prInfo?.checkRuns ?? []);
  let reviewDetails = $derived(prInfo?.reviewDetails ?? []);
  let hasPopover = $derived(checkRuns.length > 0 || reviewDetails.length > 0);

  // Lazy lookup: first hover on the chip triggers a PR fetch if the
  // (repoRoot, branch) hasn't been resolved yet. Cached hits short-
  // circuit at the lookup layer, so re-hovers are free.
  let lookupAttempted = $state(false);
  function maybeLookup() {
    if (lookupAttempted) return;
    if (!repoRoot || !wt.branch || wt.isMain) return;
    lookupAttempted = true;
    void lookupPrForRepoBranch(repoRoot, wt.branch).catch(() => {
      // Errors are surfaced via prLookupErrorFor elsewhere; the popover
      // simply stays empty.
    });
  }
</script>

{#if wt.isMain}
  <span
    data-testid="wt-main-badge"
    class="rounded bg-green/10 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-green"
    >main</span
  >
{/if}

{#if metadata?.isCurrent}
  <span
    data-testid="wt-current-badge"
    class="rounded bg-blue/10 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-blue"
    title="Current worktree"
    >current</span
  >
{:else if metadata?.isPrevious}
  <span
    data-testid="wt-previous-badge"
    class="rounded bg-bg-active px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-text-muted"
    title="Previous worktree"
    >prev</span
  >
{/if}

<span class="font-mono text-xs text-accent">{wt.branch}</span>

{#if metadata?.dirty}
  <span
    data-testid="wt-dirty-dot"
    class="text-[10px] text-yellow"
    title="Uncommitted changes in worktree"
    aria-label="Dirty"
    >●</span
  >
{/if}

{#if metadata && (metadata.ahead > 0 || metadata.behind > 0)}
  <span
    data-testid="wt-ahead-behind"
    class="text-[10px] text-text-muted"
    title={`${metadata.ahead} ahead, ${metadata.behind} behind main`}
  >
    {#if metadata.ahead > 0}↑{metadata.ahead}{/if}
    {#if metadata.ahead > 0 && metadata.behind > 0}&nbsp;{/if}
    {#if metadata.behind > 0}↓{metadata.behind}{/if}
  </span>
{/if}

{#if metadata?.locked}
  <span
    data-testid="wt-locked"
    class="text-[10px] text-red"
    title={metadata.lockReason ? `Locked: ${metadata.lockReason}` : "Locked"}
    aria-label="Locked">🔒</span
  >
{/if}

{#if metadata?.prunable}
  <span
    data-testid="wt-prunable"
    class="rounded bg-red/10 px-1 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-red"
    title={metadata.prunableReason ? `Prunable: ${metadata.prunableReason}` : "Prunable"}
    >prunable</span
  >
{/if}

{#if metadata?.mainState === "integrated"}
  <span
    data-testid="wt-merged-badge"
    class="rounded bg-green/10 px-1 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-green"
    title="Branch is already merged into the default branch — safe to clean up"
    >merged</span
  >
{/if}

{#if ciChip && metadata}
  {@const stale = metadata.ciStale}
  {@const Icon = ciChip.icon}
  {@const running = metadata.ciStatus === "running"}
  <span
    class="group relative inline-flex items-center"
    onmouseenter={maybeLookup}
    onfocusin={maybeLookup}
    role="presentation"
  >
    {#if ciHref}
      <a
        data-testid="wt-ci"
        href={ciHref}
        target="_blank"
        rel="noopener noreferrer"
        class={`inline-flex items-center gap-0.5 text-[10px] ${ciChip.color} ${stale ? "opacity-60" : ""}`}
        onclick={(e) => e.stopPropagation()}
        title={hasPopover
          ? undefined
          : `CI: ${ciChip.label}${stale ? " (stale — unpushed changes)" : ""}`}
      >
        <Icon size={11} class={running ? "animate-spin" : ""} />
        <span>ci</span>
      </a>
    {:else}
      <span
        data-testid="wt-ci"
        class={`inline-flex items-center gap-0.5 text-[10px] ${ciChip.color} ${stale ? "opacity-60" : ""}`}
        title={hasPopover
          ? undefined
          : `CI: ${ciChip.label}${stale ? " (stale — unpushed changes)" : ""}`}
      >
        <Icon size={11} class={running ? "animate-spin" : ""} />
        <span>ci</span>
      </span>
    {/if}
    <PrStatusPopover
      data-testid="wt-ci-popover"
      {checkRuns}
      {reviewDetails}
      position="bottom"
    />
  </span>
{/if}

{#if showPath}
  <span class="ml-auto max-w-40 truncate font-mono text-[10px] text-text-muted"
    >{wt.path}</span
  >
{/if}

{#if devServerHref}
  <a
    data-testid="wt-dev-server"
    href={devServerHref}
    target="_blank"
    rel="noopener noreferrer"
    class="text-[10px] text-blue underline"
    onclick={(e) => e.stopPropagation()}
    title={`Dev server: ${devServerHref}`}>url</a
  >
{/if}
