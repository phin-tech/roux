<script lang="ts">
  import { activeSession } from "$lib/stores/sessions";
  import { worktreeMetadataFor } from "$lib/stores/worktreeMetadata";
  import { prLookupFor } from "$lib/stores/sessionPrLookup";
  import { ciChipFor } from "$lib/ciIcon";
  import { safeHref } from "$lib/safeUrl";
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
  // PR URL from `sessionPrLookup`. Worktrunk wins because it carries CI
  // status (spinner / stale flag); gh only confirms the PR exists.
  let prLookupStore = $derived(
    prLookupFor($activeSession?.repoRoot, $activeSession?.branch),
  );
  let prInfo = $derived(prLookupStore ? $prLookupStore : null);
  let ghPrHref = $derived(safeHref(prInfo?.url ?? null));

  let ciHref = $derived(wtCiHref ?? ghPrHref);
  // True when we're rendering only the gh fallback — used to skip the
  // CI-status spinner / stale styling, which only worktrunk provides.
  let ghOnly = $derived(!wtCiHref && !!ghPrHref);

  /** Extract a PR-style label from a GitHub/GitLab URL (e.g. "PR #42"). */
  function prLabel(url: string): string {
    const m = url.match(/\/(?:pull|pulls|merge_requests)\/(\d+)/);
    return m ? `PR #${m[1]}` : "PR";
  }
</script>

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
