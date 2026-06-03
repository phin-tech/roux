<script lang="ts">
  import { onMount } from "svelte";
  import Server from "@lucide/svelte/icons/server";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";

  import { getDaemonStatus, type DaemonStatus } from "$lib/tauri";
  import { activeSession } from "$lib/stores/sessions";
  import { worktreeMetadataFor } from "$lib/stores/worktreeMetadata";
  import {
    prLookupErrorFor,
    prLookupForSession,
  } from "$lib/stores/sessionPrLookup";
  import { ciChipFor } from "$lib/ciIcon";
  import { checksChipFor, reviewChipFor } from "$lib/prChips";
  import { safeHref } from "$lib/safeUrl";
  import {
    closePrStatusDetails,
    prStatusDetailsOpen,
  } from "$lib/stores/prStatusDetails";
  import PrStatusPopover from "./PrStatusPopover.svelte";
  import type { StatusBarPosition } from "$lib/types";

  interface Props {
    position?: StatusBarPosition;
  }
  let { position = "bottom" }: Props = $props();

  let daemonStatus = $state<DaemonStatus | null>(null);

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
  let prLinkColor = $derived(
    prStatusChip?.color ?? "text-text-muted hover:text-text-primary",
  );
  let hasPrPopover = $derived(checkRows.length > 0 || reviewRows.length > 0);

  $effect(() => {
    if ((!$activeSession || !hasPrPopover) && $prStatusDetailsOpen) {
      closePrStatusDetails();
    }
  });

  onMount(() => {
    let cancelled = false;
    void getDaemonStatus()
      .then((status) => {
        if (!cancelled) daemonStatus = status;
      })
      .catch(() => {
        if (!cancelled) daemonStatus = null;
      });
    return () => {
      cancelled = true;
    };
  });

  /** Extract a PR-style label from a GitHub/GitLab URL (e.g. "PR #42"). */
  function prLabel(url: string): string {
    const m = url.match(/\/(?:pull|pulls|merge_requests)\/(\d+)/);
    return m ? `PR #${m[1]}` : "PR";
  }
</script>

{#snippet prStatusLink(
  href: string,
  stale: boolean = false,
  title: string = "Open PR for this branch",
)}
  <span class="group relative inline-flex items-center">
    {#if prStatusChip}
      {@const StatusIcon = prStatusChip.icon}
      <a
        data-testid="status-bar-pr-link"
        {href}
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
        {href}
        target="_blank"
        rel="noopener noreferrer"
        aria-describedby={hasPrPopover ? "status-bar-pr-popover" : undefined}
        class={`inline-flex items-center gap-1 underline ${prLinkColor} ${stale ? "opacity-60" : ""}`}
        title={hasPrPopover ? undefined : title}
      >
        <span>{prLabel(href)}</span>
      </a>
    {/if}
    <PrStatusPopover
      id="status-bar-pr-popover"
      data-testid="status-bar-pr-popover"
      checkRuns={checkRows}
      reviewDetails={reviewRows}
      position={position === "top" ? "top" : "bottom"}
      forceOpen={$prStatusDetailsOpen}
    />
  </span>
{/snippet}

<div
  class="flex h-8 items-center gap-3 bg-bg-base px-3 text-[12px] text-text-muted"
  class:border-t={position === "bottom"}
  class:border-b={position === "top"}
  class:border-border-subtle={true}
>
  {#if $activeSession}
    <div class="flex items-center gap-2">
      <div
        class="w-2.5 h-2.5 rounded-full {statusDotClass[
          $activeSession.status
        ] ?? 'bg-gray'}"
      ></div>
      <span class="text-[14px] font-semibold tracking-tight text-text-primary"
        >{$activeSession.name}</span
      >
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
      {$activeSession.cost != null
        ? `$${$activeSession.cost.toFixed(2)}`
        : "--"}
    </span>
  {:else}
    <span>No active session</span>
  {/if}
  {#if daemonStatus}
    <span
      data-testid="status-bar-daemon-indicator"
      class="ml-auto inline-flex items-center gap-1.5 text-green"
      title={`Connected to roux daemon pid=${daemonStatus.pid} socket=${daemonStatus.socket}`}
      aria-label={`Connected to roux daemon pid ${daemonStatus.pid}`}
    >
      <Server size={13} />
      <span class="font-medium">Daemon</span>
      {#if daemonStatus.processCount != null}
        <span class="text-text-muted">{daemonStatus.processCount} proc</span>
      {/if}
    </span>
  {/if}
</div>
