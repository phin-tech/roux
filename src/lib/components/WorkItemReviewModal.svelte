<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import Bot from "@lucide/svelte/icons/bot";
  import Check from "@lucide/svelte/icons/check";
  import GitCompare from "@lucide/svelte/icons/git-compare";
  import MessageSquareWarning from "@lucide/svelte/icons/message-square-warning";
  import Play from "@lucide/svelte/icons/play";
  import Terminal from "@lucide/svelte/icons/terminal";
  import Wrench from "@lucide/svelte/icons/wrench";
  import X from "@lucide/svelte/icons/x";
  import type { WorkItem } from "$lib/bindings";
  import type { WorkItemReviewPackage } from "$lib/workItems/reviewPackage";
  import type { WorkItemOpenTarget } from "$lib/workItems/openTarget";

  interface Props {
    item: WorkItem;
    reviewPackage: WorkItemReviewPackage;
    openTarget?: WorkItemOpenTarget | null;
    reviewStageName?: string | null;
    acceptReviewText: string;
    open: boolean;
    onAccept: (id: string, item: WorkItem) => void;
    onRequestChanges: (
      id: string,
      item: WorkItem,
      note: string,
    ) => void | Promise<void>;
    onOpenSession?: (sessionId: string, ptyId?: string | null) => void;
    onOpenAgent?: (
      item: WorkItem,
      reviewPackage: WorkItemReviewPackage,
    ) => void | Promise<void>;
    onOpenWorktree?: (path: string) => void | Promise<void>;
    onRunStage?: (id: string, item: WorkItem) => void | Promise<void>;
    onFixCi?: (id: string, item: WorkItem) => void;
    onViewDiff?: (item: WorkItem) => void | Promise<void>;
    onClose: () => void;
    acceptPending?: boolean;
    requestChangesPending?: boolean;
    stagePending?: boolean;
    startPending?: boolean;
    openAgentPending?: boolean;
    viewDiffPending?: boolean;
    error?: string | null;
    canRunWorkflowStage?: boolean;
    canFixCi?: boolean;
    canOpenReviewAgent?: boolean;
    canOpenReviewWorktree?: boolean;
    canViewDiff?: boolean;
    workflowStageActionText?: string;
  }

  let {
    item,
    reviewPackage,
    openTarget = null,
    reviewStageName = null,
    acceptReviewText,
    open,
    onAccept,
    onRequestChanges,
    onOpenSession,
    onOpenAgent,
    onOpenWorktree,
    onRunStage,
    onFixCi,
    onViewDiff,
    onClose,
    acceptPending = false,
    requestChangesPending = false,
    stagePending = false,
    startPending = false,
    openAgentPending = false,
    viewDiffPending = false,
    error = null,
    canRunWorkflowStage = false,
    canFixCi = false,
    canOpenReviewAgent = false,
    canOpenReviewWorktree = false,
    canViewDiff = false,
    workflowStageActionText = "Run",
  }: Props = $props();

  let requestChangesOpen = $state(false);
  let requestChangesNote = $state("");

  function onKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && !acceptPending && !requestChangesPending) {
      event.preventDefault();
      onClose();
    }
  }

  function handleBackdropClick(event: MouseEvent): void {
    if (event.target === event.currentTarget) {
      onClose();
    }
  }

  async function handleSubmitRequestChanges(): Promise<void> {
    const note = requestChangesNote.trim();
    if (!note || requestChangesPending) return;
    try {
      await onRequestChanges(item.id, item, note);
      requestChangesNote = "";
      requestChangesOpen = false;
    } catch {
      // Parent surfaces the error on the card; keep the note editable.
    }
  }

  function handleOpenTerminal(): void {
    const sessionId =
      openTarget?.sessionId ?? reviewPackage.sessionId ?? null;
    if (sessionId) onOpenSession?.(sessionId, openTarget?.ptyId ?? null);
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-[90] flex items-center justify-center bg-black/60 px-4 backdrop-blur-sm"
    transition:fade={{ duration: 120 }}
    onkeydown={onKeydown}
    onclick={handleBackdropClick}
    role="dialog"
    aria-modal="true"
    aria-label="Review work item"
    tabindex="-1"
  >
    <div
      class="w-[min(520px,100%)] max-h-[85vh] flex flex-col rounded-2xl border border-border bg-bg-surface shadow-[0_24px_80px_rgba(0,0,0,0.55),inset_0_1px_0_rgba(255,255,255,0.06)]"
      transition:scale={{ duration: 120, start: 0.98 }}
    >
      <!-- Header -->
      <div class="shrink-0 border-b border-hairline px-5 py-4">
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <h2 class="text-[15px] font-semibold text-text-primary truncate">
              Review: {item.title}
            </h2>
            {#if reviewStageName}
              <span
                class="mt-1 inline-flex items-center rounded-md border border-border-subtle/70 bg-bg-deep/55 px-1.5 py-0.5 text-[10px] leading-4 text-text-muted"
              >
                {reviewStageName}
              </span>
            {/if}
          </div>
          <button
            type="button"
            class="shrink-0 rounded-lg p-1 text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary"
            onclick={onClose}
            aria-label="Close review"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      <!-- Error -->
      {#if error}
        <div class="shrink-0 border-b border-hairline px-5 py-3">
          <p
            class="rounded-md border border-red/30 bg-red/10 px-3 py-2 text-[12px] text-red"
            role="alert"
          >
            {error}
          </p>
        </div>
      {/if}

      <!-- Details -->
      <div class="flex-1 overflow-y-auto px-5 py-4">
        <div class="grid grid-cols-[5rem_minmax(0,1fr)] gap-x-2 gap-y-1.5 text-[12px] leading-5">
          {#if reviewPackage.agentSummary}
            <span class="text-text-subtle">Summary</span>
            <span class="text-text-primary">{reviewPackage.agentSummary}</span>
          {/if}

          {#if reviewPackage.tests}
            <span class="text-text-subtle">Tests</span>
            <span class="whitespace-pre-line text-text-primary"
              >{reviewPackage.tests}</span
            >
          {/if}

          {#if reviewPackage.changedFiles.length > 0}
            <span class="text-text-subtle">Files</span>
            <span
              class="text-text-primary"
              title={reviewPackage.changedFiles.join("\n")}
            >
              {reviewPackage.changedFiles.join(", ")}
            </span>
          {/if}

          {#if reviewPackage.plan}
            <span class="text-text-subtle">Plan</span>
            <span class="text-text-primary">{reviewPackage.plan.title}</span>
          {/if}

          {#if reviewPackage.feedback}
            <span class="text-text-subtle">Feedback</span>
            <span class="text-text-primary">{reviewPackage.feedback.title}</span>
          {/if}

          {#if reviewPackage.worktreeLabel}
            <span class="text-text-subtle">Worktree</span>
            {#if canOpenReviewWorktree}
              <button
                type="button"
                class="text-left font-mono text-text-secondary transition-colors hover:text-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
                title={reviewPackage.worktreePath ?? undefined}
                aria-label="Open worktree"
                onclick={() =>
                  reviewPackage.worktreePath &&
                  onOpenWorktree?.(reviewPackage.worktreePath)}
              >
                {reviewPackage.worktreeLabel}
              </button>
            {:else}
              <span
                class="font-mono text-text-primary"
                title={reviewPackage.worktreePath ?? undefined}
              >
                {reviewPackage.worktreeLabel}
              </span>
            {/if}
          {/if}

          {#if reviewPackage.branch}
            <span class="text-text-subtle">Branch</span>
            <span class="font-mono text-text-primary">{reviewPackage.branch}</span>
          {/if}

          {#if reviewPackage.prUrl}
            <span class="text-text-subtle">PR</span>
            <a
              href={reviewPackage.prUrl}
              target="_blank"
              rel="noreferrer"
              class="text-green underline-offset-2 hover:underline"
            >
              {reviewPackage.prUrl}
            </a>
          {/if}
        </div>
      </div>

      <!-- Request changes form -->
      {#if requestChangesOpen}
        <div class="shrink-0 border-t border-hairline px-5 py-3">
          <form
            class="flex flex-col gap-2"
            onsubmit={(event) => {
              event.preventDefault();
              void handleSubmitRequestChanges();
            }}
            data-testid="work-item-request-changes-form"
          >
            <textarea
              class="min-h-[4.5rem] w-full resize-y rounded-md border border-border-subtle bg-bg-deep/70 px-2.5 py-2 text-[12px] leading-4 text-text-primary placeholder:text-text-muted/60 focus:border-accent-dim focus:outline-none focus:ring-1 focus:ring-accent-dim/50"
              placeholder="Describe the changes you're requesting..."
              bind:value={requestChangesNote}
              disabled={requestChangesPending}
            ></textarea>
            <div class="flex items-center justify-end gap-2">
              <button
                type="button"
                class="inline-flex h-7 items-center rounded-md px-3 text-[11px] text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
                onclick={() => (requestChangesOpen = false)}
                disabled={requestChangesPending}
              >
                Cancel
              </button>
              <button
                type="submit"
                class="inline-flex h-7 items-center rounded-md border border-amber/30 bg-amber/10 px-3 text-[11px] font-semibold text-amber transition-colors hover:bg-amber/15 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-amber/50 disabled:cursor-wait disabled:opacity-60"
                disabled={requestChangesPending || !requestChangesNote.trim()}
              >
                {requestChangesPending ? "Requesting..." : "Request changes"}
              </button>
            </div>
          </form>
        </div>
      {/if}

      <!-- Actions -->
      <div class="shrink-0 border-t border-hairline px-5 py-4">
        <div class="flex flex-col gap-2">
          <!-- Primary + secondary actions -->
          <div class="flex items-stretch gap-2">
            <button
              type="button"
              class="flex-1 inline-flex h-9 items-center justify-center gap-1.5 rounded-xl border border-green/30 bg-green/10 px-3 text-[13px] font-semibold text-green transition-colors hover:bg-green/15 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-green/50 disabled:cursor-wait disabled:opacity-60"
              onclick={() => onAccept(item.id, item)}
              aria-label="Accept work item review"
              disabled={acceptPending}
            >
              <Check size={14} strokeWidth={2.2} />
              {acceptPending ? "Accepting..." : acceptReviewText}
            </button>
            <button
              type="button"
              class="inline-flex h-9 items-center justify-center gap-1.5 rounded-xl border border-amber/30 bg-amber/10 px-3 text-[13px] font-semibold text-amber transition-colors hover:bg-amber/15 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-amber/50 disabled:cursor-wait disabled:opacity-60"
              onclick={() => (requestChangesOpen = !requestChangesOpen)}
              aria-label="Request changes"
              disabled={requestChangesPending}
            >
              <MessageSquareWarning size={14} strokeWidth={2.2} />
              {requestChangesPending ? "Requesting..." : "Request changes"}
            </button>
          </div>

          <!-- Tertiary actions -->
          {#if canRunWorkflowStage || canFixCi || canOpenReviewAgent || canViewDiff || (openTarget?.sessionId || reviewPackage.sessionId)}
            <div class="flex flex-wrap items-center gap-1.5">
              {#if canRunWorkflowStage}
                <button
                  type="button"
                  class="inline-flex h-7 items-center gap-1.5 rounded-lg border border-border-subtle bg-bg-surface px-2.5 text-[11px] font-medium text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
                  onclick={() => onRunStage?.(item.id, item)}
                  disabled={stagePending}
                >
                  <Play size={11} fill="currentColor" strokeWidth={2.2} />
                  {stagePending ? "Running..." : workflowStageActionText}
                </button>
              {/if}

              {#if canFixCi}
                <button
                  type="button"
                  class="inline-flex h-7 items-center gap-1.5 rounded-lg border border-border-subtle bg-bg-surface px-2.5 text-[11px] font-medium text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
                  onclick={() => onFixCi?.(item.id, item)}
                  disabled={startPending}
                >
                  <Wrench size={11} strokeWidth={2.2} />
                  {startPending ? "Starting..." : "Fix CI"}
                </button>
              {/if}

              {#if canOpenReviewAgent}
                <button
                  type="button"
                  class="inline-flex h-7 items-center gap-1.5 rounded-lg border border-border-subtle bg-bg-surface px-2.5 text-[11px] font-medium text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
                  onclick={() => onOpenAgent?.(item, reviewPackage)}
                  disabled={openAgentPending}
                >
                  <Bot size={11} strokeWidth={2.2} />
                  {openAgentPending ? "Opening..." : "Open agent"}
                </button>
              {/if}

              {#if canViewDiff}
                <button
                  type="button"
                  class="inline-flex h-7 items-center gap-1.5 rounded-lg border border-border-subtle bg-bg-surface px-2.5 text-[11px] font-medium text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
                  onclick={() => onViewDiff?.(item)}
                  disabled={viewDiffPending}
                >
                  <GitCompare size={11} strokeWidth={2.2} />
                  {viewDiffPending ? "Opening..." : "View diff"}
                </button>
              {/if}

              {#if openTarget?.sessionId || reviewPackage.sessionId}
                <button
                  type="button"
                  class="inline-flex h-7 items-center gap-1.5 rounded-lg border border-border-subtle bg-bg-surface px-2.5 text-[11px] font-medium text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
                  onclick={handleOpenTerminal}
                >
                  <Terminal size={11} strokeWidth={2.2} />
                  Open terminal
                </button>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}
