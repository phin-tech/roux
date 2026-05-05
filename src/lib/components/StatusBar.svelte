<script lang="ts">
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";

  import { activeSession } from "$lib/stores/sessions";
  import { worktreeMetadataFor } from "$lib/stores/worktreeMetadata";
  import {
    prLookupErrorFor,
    prLookupForSession,
  } from "$lib/stores/sessionPrLookup";
  import { ciChipFor } from "$lib/ciIcon";
  import { checksChipFor, reviewChipFor } from "$lib/prChips";
  import { safeHref } from "$lib/safeUrl";
  import type { PrCheckStatus } from "$lib/tauri";
  import type { StatusBarPosition } from "$lib/types";

  interface Props {
    position?: StatusBarPosition;
  }
  let { position = "bottom" }: Props = $props();

  const statusDotClass: Record<string, string> = {
    idle: "bg-green",
    thinking: "bg-amber animate-pulse",
    generating: "bg-blue",
    error: "bg-red",
    disconnected: "bg-gray",
    attention: "bg-amber animate-pulse",
  };

  // Look up the active session's worktrunk metadata so we can surface
  // the PR/CI link right in the status bar.
  let sessionMetadata = $derived(
    $activeSession?.worktreePath
      ? worktreeMetadataFor($activeSession.worktreePath)
      : null,
  );
  let wtMeta = $derived(sessionMetadata ? $sessionMetadata : null);
  let ciChip = $derived(ciChipFor(wtMeta?.ciStatus ?? null));
  let wtCiHref = $derived(safeHref(wtMeta?.ciUrl));

  // Fallback: when worktrunk hasn't supplied a ciUrl, use the gh-derived
  // PR URL. Honors `pinnedPrUrl` first, then branch-based discovery.
  // Worktrunk wins overall because it carries CI status (spinner / stale
  // flag); gh only confirms the PR exists.
  let prLookupStore = $derived(prLookupForSession($activeSession ?? null));
  let prInfo = $derived(prLookupStore ? $prLookupStore : null);
  let ghPrHref = $derived(safeHref(prInfo?.url ?? null));

  let prErrorStore = $derived(prLookupErrorFor($activeSession ?? null));
  let prError = $derived(prErrorStore ? $prErrorStore : null);

  let ciHref = $derived(wtCiHref ?? ghPrHref);
  // True when we're rendering only the gh fallback — used to skip the
  // CI-status spinner / stale styling, which only worktrunk provides.
  let ghOnly = $derived(!wtCiHref && !!ghPrHref);

  // Tiny chips next to the PR link: aggregate check status + review
  // decision. Both come from the gh-derived `PrInfo`, independent of
  // worktrunk, so they render in both the worktrunk and gh-only paths.
  let checksChip = $derived(checksChipFor(prInfo?.checks));
  let checkRows = $derived(prInfo?.checkRuns ?? []);
  let reviewChip = $derived(reviewChipFor(prInfo?.reviewDecision));

  /** Extract a PR-style label from a GitHub/GitLab URL (e.g. "PR #42"). */
  function prLabel(url: string): string {
    const m = url.match(/\/(?:pull|pulls|merge_requests)\/(\d+)/);
    return m ? `PR #${m[1]}` : "PR";
  }

  function checkStatusLabel(status: PrCheckStatus): string {
    switch (status) {
      case "passing":
        return "passing";
      case "failing":
        return "failing";
      case "pending":
        return "pending";
    }
  }

  function checkStatusDotClass(status: PrCheckStatus): string {
    switch (status) {
      case "passing":
        return "bg-green";
      case "failing":
        return "bg-red";
      case "pending":
        return "bg-yellow";
    }
  }

  function checkStatusTextClass(status: PrCheckStatus): string {
    switch (status) {
      case "passing":
        return "text-green";
      case "failing":
        return "text-red";
      case "pending":
        return "text-yellow";
    }
  }
</script>

{#snippet prChecksChip()}
  {#if checksChip}
    {@const Icon = checksChip.icon}
    <span class="group relative inline-flex items-center">
      <span
        data-testid="status-bar-pr-checks"
        class={`inline-flex items-center ${checksChip.color}`}
        aria-label={checksChip.label}
        role="img"
        title={checkRows.length > 0 ? undefined : checksChip.label}
      >
        <Icon size={12} class={checksChip.spin ? "animate-spin" : ""} />
      </span>
      {#if checkRows.length > 0}
        <div
          data-testid="status-bar-pr-checks-popover"
          role="tooltip"
          class={`pointer-events-none absolute left-1/2 z-50 hidden max-h-72 min-w-64 max-w-80 -translate-x-1/2 overflow-y-auto rounded border border-border bg-bg-elevated p-2 text-[11px] text-text-primary shadow-lg group-hover:block ${position === "top" ? "top-full mt-2" : "bottom-full mb-2"}`}
        >
          <div class="mb-1 text-[10px] font-semibold uppercase text-text-muted">Checks</div>
          <div class="space-y-1">
            {#each checkRows as check}
              <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
                <span class="truncate" title={check.name}>{check.name}</span>
                <span class={`inline-flex items-center gap-1 ${checkStatusTextClass(check.status)}`}>
                  <span class={`h-1.5 w-1.5 rounded-full ${checkStatusDotClass(check.status)}`}></span>
                  <span>{checkStatusLabel(check.status)}</span>
                </span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </span>
  {/if}
{/snippet}

<div
  class="flex h-8 items-center gap-3 bg-bg-base px-3 text-[12px] text-text-muted"
  class:border-t={position === "bottom"}
  class:border-b={position === "top"}
  class:border-border-subtle={true}
>
  {#if $activeSession}
    <div class="flex items-center gap-2">
      <div class="w-2.5 h-2.5 rounded-full {statusDotClass[$activeSession.status] ?? 'bg-gray'}"></div>
      <span class="text-[14px] font-semibold tracking-tight text-text-primary">{$activeSession.name}</span>
    </div>
    {#if $activeSession.isGitRepo}
      <span class="text-text-secondary">&bull;</span>
      <span class="text-text-muted">&#9095; {$activeSession.branch}</span>
    {/if}
    {#if ciHref && ciChip && wtMeta && !ghOnly}
      {@const Icon = ciChip.icon}
      {@const running = wtMeta.ciStatus === "running"}
      <span class="text-text-secondary">&bull;</span>
      <a
        data-testid="status-bar-pr-link"
        href={ciHref}
        target="_blank"
        rel="noopener noreferrer"
        class={`inline-flex items-center gap-1 underline ${ciChip.color} ${wtMeta.ciStale ? "opacity-60" : ""}`}
        title={`CI: ${ciChip.label}${wtMeta.ciStale ? " (stale — unpushed changes)" : ""}`}
      >
        <Icon size={12} class={running ? "animate-spin" : ""} />
        <span>{prLabel(ciHref)}</span>
      </a>
      {@render prChecksChip()}
      {#if reviewChip}
        {@const Icon = reviewChip.icon}
        <span
          data-testid="status-bar-pr-review"
          class={`inline-flex items-center ${reviewChip.color}`}
          title={reviewChip.label}
        >
          <Icon size={12} />
        </span>
      {/if}
    {:else if ciHref && ghOnly}
      <span class="text-text-secondary">&bull;</span>
      <a
        data-testid="status-bar-pr-link"
        href={ciHref}
        target="_blank"
        rel="noopener noreferrer"
        class="inline-flex items-center gap-1 underline text-text-muted hover:text-text-primary"
        title={prInfo?.title ? `PR: ${prInfo.title}` : "Open PR for this branch"}
      >
        <span>{prLabel(ciHref)}</span>
      </a>
      {@render prChecksChip()}
      {#if reviewChip}
        {@const Icon = reviewChip.icon}
        <span
          data-testid="status-bar-pr-review"
          class={`inline-flex items-center ${reviewChip.color}`}
          title={reviewChip.label}
        >
          <Icon size={12} />
        </span>
      {/if}
    {:else if ciChip && wtMeta}
      {@const Icon = ciChip.icon}
      {@const running = wtMeta.ciStatus === "running"}
      <span class="text-text-secondary">&bull;</span>
      <span
        data-testid="status-bar-ci-chip"
        class={`inline-flex items-center gap-1 ${ciChip.color} ${wtMeta.ciStale ? "opacity-60" : ""}`}
        title={`CI: ${ciChip.label}${wtMeta.ciStale ? " (stale)" : ""}`}
      >
        <Icon size={12} class={running ? "animate-spin" : ""} />
        <span>ci</span>
      </span>
    {:else if prError && !ciHref}
      <span class="text-text-secondary">&bull;</span>
      <span
        data-testid="status-bar-pr-error"
        class="inline-flex items-center gap-1 text-amber"
        title={`PR lookup failed: ${prError}`}
      >
        <TriangleAlert size={12} />
        <span>PR?</span>
      </span>
    {/if}
    <span class="text-text-secondary">&bull;</span>
    <span class="text-text-muted">{$activeSession.model ?? "--"}</span>
    <span class="text-text-secondary">&bull;</span>
    <span class="text-text-muted">
      {$activeSession.cost != null ? `$${$activeSession.cost.toFixed(2)}` : "--"}
    </span>
  {:else}
    <span>No active session</span>
  {/if}
</div>
