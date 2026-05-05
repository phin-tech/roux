<script lang="ts">
  import type { PrCheckDetails, PrCheckStatus, PrReviewDetails } from "$lib/tauri";

  interface Props {
    /** Per-check rows from the PR's `statusCheckRollup`. */
    checkRuns?: PrCheckDetails[];
    /** Latest review per reviewer from `latestReviews`. */
    reviewDetails?: PrReviewDetails[];
    /** Optional id for `aria-describedby`. */
    id?: string;
    /** Anchor side; the popover sticks to the opposite edge. */
    position?: "top" | "bottom";
    /**
     * When true, render the popover unconditionally. Defaults to the
     * `group-hover:block group-focus-within:block` pattern used inside
     * a `.group relative` parent.
     */
    forceOpen?: boolean;
    /** Optional testid for the popover root. */
    "data-testid"?: string;
  }

  let {
    checkRuns = [],
    reviewDetails = [],
    id,
    position = "bottom",
    forceOpen = false,
    "data-testid": testId,
  }: Props = $props();

  let approvalCount = $derived(
    reviewDetails.filter(
      (review) => normalizedReviewState(review.state) === "approved",
    ).length,
  );

  let hasContent = $derived(checkRuns.length > 0 || reviewDetails.length > 0);

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

{#if hasContent}
  <div
    {id}
    data-testid={testId}
    role="tooltip"
    class={`absolute left-1/2 z-50 max-h-80 min-w-72 max-w-96 -translate-x-1/2 overflow-y-auto rounded border border-border bg-bg-elevated p-2 text-[11px] text-text-primary shadow-lg ${forceOpen ? "block" : "hidden group-hover:block group-focus-within:block"} ${position === "top" ? "top-full mt-2" : "bottom-full mb-2"}`}
  >
    {#if checkRuns.length > 0}
      <div class="mb-1 text-[10px] font-semibold uppercase text-text-muted">Checks</div>
      <div class="space-y-1">
        {#each checkRuns as check}
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
    {#if reviewDetails.length > 0}
      <div class={checkRuns.length > 0 ? "mt-3" : ""}>
        <div class="mb-1 flex items-center justify-between gap-3 text-[10px] font-semibold uppercase text-text-muted">
          <span>Reviews</span>
          <span>Approvals {approvalCount}/{reviewDetails.length}</span>
        </div>
        <div class="space-y-1">
          {#each reviewDetails as review}
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
