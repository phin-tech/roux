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
  import { closePrStatusDetails, prStatusDetailsOpen } from "$lib/stores/prStatusDetails";
  import type { PrCheckStatus, PrReviewDetails } from "$lib/tauri";
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
  let reviewRows = $derived(prInfo?.reviewDetails ?? []);
  let prStatusChip = $derived(
    checksChip ??
      (ciChip
        ? {
            icon: ciChip.icon,
            color: ciChip.color,
            label: `CI: ${ciChip.label}`,
            spin: wtMeta?.ciStatus === "running",
          }
        : reviewChip),
  );
  let prLinkColor = $derived(prStatusChip?.color ?? "text-text-muted hover:text-text-primary");
  let hasPrPopover = $derived(checkRows.length > 0 || reviewRows.length > 0);
  let approvalCount = $derived(
    reviewRows.filter((review) => normalizedReviewState(review.state) === "approved").length,
  );

  $effect(() => {
    if ((!$activeSession || !hasPrPopover) && $prStatusDetailsOpen) {
      closePrStatusDetails();
    }
  });

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

  function normalizedReviewState(state: string): string {
    return state.trim().toLowerCase().replace(/_/g, " ");
  }

  function reviewStatusLabel(review: PrReviewDetails): string {
    const normalized = normalizedReviewState(review.state);
    return normalized || "unknown";
  }

  function reviewStatusTextClass(review: PrReviewDetails): string {
    switch (normalizedReviewState(review.state)) {
      case "approved":
        return "text-green";
      case "changes requested":
        return "text-red";
      case "review requested":
      case "pending":
        return "text-yellow";
      default:
        return "text-text-muted";
    }
  }

  function reviewStatusDotClass(review: PrReviewDetails): string {
    switch (normalizedReviewState(review.state)) {
      case "approved":
        return "bg-green";
      case "changes requested":
        return "bg-red";
      case "review requested":
      case "pending":
        return "bg-yellow";
      default:
        return "bg-text-muted";
    }
  }
</script>

{#snippet prStatusLink(href: string, stale: boolean = false, title: string = "Open PR for this branch")}
  <span class="group relative inline-flex items-center">
    {#if prStatusChip}
      {@const StatusIcon = prStatusChip.icon}
      <a
        data-testid="status-bar-pr-link"
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        aria-describedby={hasPrPopover ? "status-bar-pr-popover" : undefined}
        class={`inline-flex items-center gap-1 underline ${prLinkColor} ${stale ? "opacity-60" : ""}`}
        title={hasPrPopover ? undefined : title}
      >
        <StatusIcon size={12} class={prStatusChip.spin ? "animate-spin" : ""} />
        <span>{prLabel(href)}</span>
      </a>
    {:else}
      <a
        data-testid="status-bar-pr-link"
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        aria-describedby={hasPrPopover ? "status-bar-pr-popover" : undefined}
        class={`inline-flex items-center gap-1 underline ${prLinkColor} ${stale ? "opacity-60" : ""}`}
        title={hasPrPopover ? undefined : title}
      >
        <span>{prLabel(href)}</span>
      </a>
    {/if}
    {@render prPopover()}
  </span>
{/snippet}

{#snippet prPopover()}
  {#if hasPrPopover}
    <div
      id="status-bar-pr-popover"
      data-testid="status-bar-pr-popover"
      role="tooltip"
      class={`absolute left-1/2 z-50 max-h-80 min-w-72 max-w-96 -translate-x-1/2 overflow-y-auto rounded border border-border bg-bg-elevated p-2 text-[11px] text-text-primary shadow-lg ${$prStatusDetailsOpen ? "block" : "hidden group-hover:block group-focus-within:block"} ${position === "top" ? "top-full mt-2" : "bottom-full mb-2"}`}
    >
      {#if checkRows.length > 0}
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
      {/if}
      {#if reviewRows.length > 0}
        <div class={checkRows.length > 0 ? "mt-3" : ""}>
          <div class="mb-1 flex items-center justify-between gap-3 text-[10px] font-semibold uppercase text-text-muted">
            <span>Reviews</span>
            <span>Approvals {approvalCount}/{reviewRows.length}</span>
          </div>
          <div class="space-y-1">
            {#each reviewRows as review}
              <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
                <span class="truncate" title={review.reviewer}>{review.reviewer}</span>
                <span class={`inline-flex items-center gap-1 ${reviewStatusTextClass(review)}`}>
                  <span class={`h-1.5 w-1.5 rounded-full ${reviewStatusDotClass(review)}`}></span>
                  <span>{reviewStatusLabel(review)}</span>
                </span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
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
      <span class="text-text-secondary">&bull;</span>
      {@render prStatusLink(
        ciHref,
        wtMeta.ciStale,
        `CI: ${ciChip.label}${wtMeta.ciStale ? " (stale — unpushed changes)" : ""}`,
      )}
    {:else if ciHref && ghOnly}
      <span class="text-text-secondary">&bull;</span>
      {@render prStatusLink(
        ciHref,
        false,
        prInfo?.title ? `PR: ${prInfo.title}` : "Open PR for this branch",
      )}
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
