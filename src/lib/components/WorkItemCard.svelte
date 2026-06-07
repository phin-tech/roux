<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import Bot from "@lucide/svelte/icons/bot";
  import Check from "@lucide/svelte/icons/check";
  import ClipboardList from "@lucide/svelte/icons/clipboard-list";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import MessageSquareWarning from "@lucide/svelte/icons/message-square-warning";
  import MoreVertical from "@lucide/svelte/icons/more-vertical";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Terminal from "@lucide/svelte/icons/terminal";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Wrench from "@lucide/svelte/icons/wrench";
  import type { WorkItem } from "$lib/bindings";
  import type { WorkItemPhase } from "$lib/workItems/phase";
  import type { WorkItemReviewPackage } from "$lib/workItems/reviewPackage";
  import type { SessionStatus } from "$lib/types";
  import { profileList } from "$lib/panes/profiles";
  import { clearDraggedWorkItem, writeWorkItemDragData } from "$lib/board/drag";
  import { unreadBySession } from "$lib/stores/notifications";
  import { projects } from "$lib/stores/projects";
  import { settings } from "$lib/stores/settings";
  import { reviewStageLabel } from "$lib/workItems/reviewStages";
  import {
    workflowStage,
    workflowStageActionLabel,
    workflowStageLabel,
  } from "$lib/workItems/workflow";
  import { worktreeMetadataFor } from "$lib/stores/worktreeMetadata";
  import { ciChipFor } from "$lib/ciIcon";
  import type { WorkItemStartActionOptions } from "$lib/stores/workItems";

  interface Props {
    item: WorkItem;
    sessionStatus?: SessionStatus | null;
    onStart?: (
      id: string,
      item: WorkItem,
      options?: WorkItemStartActionOptions,
    ) => void;
    /** Start or open a planning run for this work item. */
    onPlan?: (id: string, item: WorkItem, replaceActive?: boolean) => void;
    /** Run the card's current workflow stage. */
    onRunStage?: (id: string, item: WorkItem) => void | Promise<void>;
    /** Open the card's bound session (by session id). */
    onOpen?: (sessionId: string) => void;
    /** Open the card editor (by work item id). */
    onEdit?: (id: string) => void;
    /** Delete the card (by work item id). */
    onDelete?: (id: string, item: WorkItem) => void;
    /** Archive the card (by work item id). */
    onArchive?: (id: string, item: WorkItem) => void;
    /** Accept a review-requested implementation run. */
    onAcceptReview?: (id: string, item: WorkItem) => void;
    /** Attach review feedback and move the card back to active work. */
    onRequestChanges?: (
      id: string,
      item: WorkItem,
      note: string,
    ) => void | Promise<void>;
    /** Reveal the card's worktree path in the OS file manager. */
    onOpenWorktree?: (path: string) => void | Promise<void>;
    /** Open a new agent session in the review worktree. */
    onOpenAgent?: (
      item: WorkItem,
      reviewPackage: WorkItemReviewPackage,
    ) => void | Promise<void>;
    startPending?: boolean;
    planPending?: boolean;
    stagePending?: boolean;
    acceptPending?: boolean;
    requestChangesPending?: boolean;
    openAgentPending?: boolean;
    archivePending?: boolean;
    startError?: string | null;
    /** Derived run/column phase: drives the action affordance + blocked state. */
    phase: WorkItemPhase;
    reviewPackage?: WorkItemReviewPackage | null;
    attachedSessionIds?: string[];
    attentionSessionId?: string | null;
    /** Opt-in card dragging. The full-screen board enables it; the sidebar leaves it off. */
    draggable?: boolean;
  }

  const {
    item,
    sessionStatus = null,
    onStart,
    onPlan,
    onRunStage,
    onOpen,
    onEdit,
    onDelete,
    onArchive,
    onAcceptReview,
    onRequestChanges,
    onOpenWorktree,
    onOpenAgent,
    startPending = false,
    planPending = false,
    stagePending = false,
    acceptPending = false,
    requestChangesPending = false,
    openAgentPending = false,
    archivePending = false,
    startError = null,
    phase,
    reviewPackage = null,
    attachedSessionIds = [],
    attentionSessionId: attachedAttentionSessionId = null,
    draggable = false,
  }: Props = $props();

  const statusDotClasses: Partial<Record<SessionStatus, string>> = {
    idle: "bg-green",
    generating: "bg-blue",
    thinking: "bg-blue",
    attention: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)]",
    error: "bg-red",
    disconnected: "bg-muted",
  };

  const hasSession = $derived(phase.hasSession);
  const hasPlanningSession = $derived(phase.hasPlanningSession);
  const isPlanning = $derived(phase.isPlanning);
  const hasAttachedPlan = $derived(phase.hasAttachedPlan);
  const pendingDecision = $derived(phase.pendingDecision);
  const attentionSessionId = $derived(
    phase.attentionSessionId ?? attachedAttentionSessionId,
  );
  const hasPendingQuestion = $derived(!!pendingDecision || !!attentionSessionId);
  const primaryOpenSessionId = $derived(
    phase.action.kind === "open-session" ? phase.action.sessionId : null,
  );
  const planningOpenSessionId = $derived(
    phase.action.kind === "open-planning" ? phase.action.sessionId : null,
  );
  const startActionAriaLabel = $derived(
    phase.action.kind === "configure"
      ? "Configure work item"
      : phase.action.kind === "approve-start"
        ? "Approve and start work item"
        : "Start work item",
  );
  const startActionText = $derived(
    phase.action.kind === "configure"
      ? "Configure"
      : phase.action.kind === "approve-start"
        ? "Approve & start"
        : "Start",
  );
  const dotClass = $derived(
    sessionStatus ? (statusDotClasses[sessionStatus] ?? "bg-muted") : null,
  );
  const projectLabel = $derived(
    item.projectId
      ? ($projects.find((p) => p.id === item.projectId)?.name ?? "Project")
      : null,
  );
  const profileLabel = $derived(
    item.agentProfile
      ? ($profileList.find((profile) => profile.id === item.agentProfile)
          ?.name ?? item.agentProfile)
      : null,
  );
  const targetLabel = $derived.by(() => {
    const path = item.worktreePath ?? item.repoPath;
    if (!path) return null;
    const segments = path.replaceAll("\\", "/").split("/").filter(Boolean);
    return segments.slice(-2).join("/") || path;
  });
  const branchLabel = $derived(item.branch ?? null);
  const canForceStartPlanning = $derived(phase.canForceStart && !!onStart);
  const canFixCi = $derived(
    item.status === "review" &&
      item.reviewStageId === "pr_review" &&
      !!item.pinnedPrUrl &&
      !!onStart,
  );
  const hasMenuActions = $derived(
    !!onEdit || !!onPlan || !!onDelete || !!onArchive || canForceStartPlanning,
  );
  const workflowStageName = $derived(
    item.workflowStageLabel ??
      workflowStageLabel(item.workflowStageId, $settings.kanban),
  );
  const workflowStageActionText = $derived(
    workflowStageActionLabel(item.workflowStageId, $settings.kanban) ??
      workflowStageName ??
      "Run",
  );
  const activeWorkflowStage = $derived(
    workflowStage($settings.kanban, item.workflowStageId),
  );
  const workflowStageIsAgentBacked = $derived(
    activeWorkflowStage?.runner?.type === "agent",
  );
  const canRunWorkflowStage = $derived(
    !!onRunStage &&
      !!item.workflowStageId &&
      item.status !== "done" &&
      (!workflowStageIsAgentBacked ||
        phase.action.kind === "plan" ||
        phase.action.kind === "start" ||
        phase.action.kind === "accept-review"),
  );
  const reviewSessionId = $derived(reviewPackage?.sessionId ?? null);
  const reviewStageName = $derived(
    reviewStageLabel(item.reviewStageId, $settings.kanban),
  );
  const acceptReviewText = $derived(
    reviewStageName ? `Accept ${reviewStageName}` : "Accept done",
  );
  const canOpenReviewWorktree = $derived(
    !!reviewPackage?.worktreePath && !!onOpenWorktree,
  );
  const canOpenReviewAgent = $derived(
    !!reviewPackage?.worktreePath && !!onOpenAgent,
  );
  const reviewWorktreeMetadata = $derived(
    reviewPackage?.worktreePath
      ? worktreeMetadataFor(reviewPackage.worktreePath)
      : null,
  );
  const reviewWorktreeMeta = $derived(
    reviewWorktreeMetadata ? $reviewWorktreeMetadata : null,
  );
  const reviewCiChip = $derived(
    ciChipFor(reviewWorktreeMeta?.ciStatus ?? null),
  );
  const allAttachedSessionIds = $derived.by(() => {
    const ids = new Set<string>();
    if (item.sessionId) ids.add(item.sessionId);
    if (attentionSessionId) ids.add(attentionSessionId);
    for (const sessionId of attachedSessionIds) {
      if (sessionId) ids.add(sessionId);
    }
    return [...ids];
  });
  const unreadActivity = $derived.by(() => {
    let count = 0;
    let sessionCount = 0;
    let targetSessionId: string | null = null;
    for (const sessionId of allAttachedSessionIds) {
      const sessionUnread = $unreadBySession.get(sessionId) ?? 0;
      if (sessionUnread <= 0) continue;
      count += sessionUnread;
      sessionCount += 1;
      targetSessionId ??= sessionId;
    }
    return { count, sessionCount, targetSessionId };
  });
  const unreadActivityTitle = $derived(
    unreadActivity.sessionCount > 1
      ? `${unreadActivity.count} unread notification${unreadActivity.count === 1 ? "" : "s"} across ${unreadActivity.sessionCount} attached sessions`
      : `${unreadActivity.count} unread notification${unreadActivity.count === 1 ? "" : "s"}`,
  );
  const attentionButtonRight = $derived(hasMenuActions ? "2rem" : "0.375rem");
  const liveStatusRight = $derived(
    hasMenuActions
      ? hasPendingQuestion
        ? "3.5rem"
        : "2rem"
      : hasPendingQuestion
        ? "2rem"
        : "0.5rem",
  );
  const chipClass =
    "inline-flex min-w-0 max-w-full items-center gap-1 rounded-md border border-border-subtle/70 bg-bg-deep/55 px-1.5 py-0.5 text-[10px] leading-4 text-text-muted";
  const primaryActionClass =
    "ml-auto inline-flex h-6 items-center gap-1.5 rounded-md border px-2 text-[10px] font-semibold shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors focus-visible:outline-none disabled:cursor-wait disabled:opacity-60";
  const accentActionClass =
    primaryActionClass +
    " border-accent-dim/30 bg-accent-dim/15 text-accent hover:bg-accent-dim/25 focus-visible:ring-1 focus-visible:ring-accent-dim/60";
  const amberActionClass =
    primaryActionClass +
    " border-amber/30 bg-amber/10 text-amber hover:bg-amber/15 focus-visible:ring-1 focus-visible:ring-amber/50";
  const doneActionClass =
    primaryActionClass +
    " border-green/30 bg-green/10 text-green hover:bg-green/15 focus-visible:ring-1 focus-visible:ring-green/50";
  const reviewActionClass =
    "inline-flex h-7 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border px-2 text-[11px] font-semibold shadow-[inset_0_1px_0_rgba(255,255,255,0.05)] transition-colors focus-visible:outline-none disabled:cursor-wait disabled:opacity-60";
  const reviewAgentActionClass =
    reviewActionClass +
    " border-accent-dim/35 bg-accent-dim/15 text-accent hover:bg-accent-dim/25 focus-visible:ring-1 focus-visible:ring-accent-dim/60";
  const reviewChangesActionClass =
    reviewActionClass +
    " border-amber/30 bg-amber/10 text-amber hover:bg-amber/15 focus-visible:ring-1 focus-visible:ring-amber/50";
  const reviewDoneActionClass =
    reviewActionClass +
    " border-green/30 bg-green/10 text-green hover:bg-green/15 focus-visible:ring-1 focus-visible:ring-green/50";

  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);
  let requestChangesOpen = $state(false);
  let requestChangesNote = $state("");

  function openMenuAt(clientX: number, clientY: number): void {
    if (!hasMenuActions) return;
    menuX = Math.max(8, clientX);
    menuY = Math.max(8, clientY);
    menuOpen = true;
  }

  function handleMenuButtonClick(event: MouseEvent): void {
    event.stopPropagation();
    const target = event.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    openMenuAt(rect.right - 160, rect.bottom + 4);
  }

  function handleContextMenu(event: MouseEvent): void {
    if (!hasMenuActions) return;
    event.preventDefault();
    openMenuAt(event.clientX, event.clientY);
  }

  function handleEdit(): void {
    menuOpen = false;
    onEdit?.(item.id);
  }

  function handlePlan(): void {
    menuOpen = false;
    onPlan?.(item.id, item);
  }

  function handleReplan(): void {
    menuOpen = false;
    onPlan?.(item.id, item, true);
  }

  function handleRunStage(): void {
    menuOpen = false;
    void onRunStage?.(item.id, item);
  }

  function handleForceStart(): void {
    menuOpen = false;
    onStart?.(item.id, item, { forceStart: true });
  }

  function handleFixCi(): void {
    menuOpen = false;
    onStart?.(item.id, item, { fixCi: true });
  }

  function handleDelete(): void {
    menuOpen = false;
    onDelete?.(item.id, item);
  }

  function handleArchive(): void {
    menuOpen = false;
    onArchive?.(item.id, item);
  }

  function handleAcceptReview(): void {
    menuOpen = false;
    onAcceptReview?.(item.id, item);
  }

  function handleOpenWorktree(): void {
    menuOpen = false;
    const path = reviewPackage?.worktreePath;
    if (!path) return;
    void onOpenWorktree?.(path);
  }

  function handleOpenItemWorktree(): void {
    const path = item.worktreePath;
    if (!path) return;
    void onOpenWorktree?.(path);
  }

  function handleOpenReviewTerminal(): void {
    menuOpen = false;
    if (!reviewSessionId) return;
    onOpen?.(reviewSessionId);
  }

  function handleOpenReviewAgent(): void {
    menuOpen = false;
    if (!reviewPackage) return;
    void onOpenAgent?.(item, reviewPackage);
  }

  function handleRequestChangesOpen(): void {
    menuOpen = false;
    requestChangesOpen = true;
  }

  async function handleSubmitRequestChanges(): Promise<void> {
    const note = requestChangesNote.trim();
    if (!note || requestChangesPending) return;
    try {
      await onRequestChanges?.(item.id, item, note);
      requestChangesNote = "";
      requestChangesOpen = false;
    } catch {
      // Parent surfaces the error on the card; keep the note editable.
    }
  }

  function handleAttentionOpen(event: MouseEvent): void {
    event.stopPropagation();
    if (!attentionSessionId) return;
    onOpen?.(attentionSessionId);
  }

  function handleQuestionChipClick(event: MouseEvent): void {
    event.stopPropagation();
    if (pendingDecision && onEdit) {
      onEdit(item.id);
      return;
    }
    if (attentionSessionId && onOpen) {
      onOpen(attentionSessionId);
    }
  }

  function handleUnreadActivityOpen(event: MouseEvent): void {
    event.stopPropagation();
    if (!unreadActivity.targetSessionId) return;
    onOpen?.(unreadActivity.targetSessionId);
  }

  function handlePlanChipClick(): void {
    if (!onEdit) return;
    onEdit(item.id);
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      menuOpen = false;
      requestChangesOpen = false;
    }
  }
</script>

<svelte:window
  onclick={() => (menuOpen = false)}
  onkeydown={handleWindowKeydown}
/>

<div
  role="presentation"
  class="work-card group relative flex flex-col gap-1.5 px-3 py-2.5 pl-3.5"
  class:cursor-grab={draggable}
  class:active:cursor-grabbing={draggable}
  {draggable}
  data-session-bound={hasSession}
  data-error={!!startError}
  data-blocked={hasPendingQuestion}
  ondragstart={draggable
    ? (e) => writeWorkItemDragData(e.dataTransfer, item)
    : undefined}
  ondragend={draggable ? clearDraggedWorkItem : undefined}
  oncontextmenu={handleContextMenu}
  data-testid="work-item-card"
  data-item-id={item.id}
>
  <!-- Live status dot -->
  {#if dotClass}
    <span
      class="absolute top-3 flex h-2 w-2"
      style={`right: ${liveStatusRight};`}
      role="img"
      aria-label="live status"
    >
      <span
        class="absolute inline-flex h-2 w-2 animate-ping rounded-full opacity-60 {dotClass}"
      ></span>
      <span class="relative inline-flex h-2 w-2 rounded-full {dotClass}"></span>
    </span>
  {/if}

  {#if hasPendingQuestion}
    <button
      type="button"
      class="absolute top-1.5 z-10 flex h-6 w-6 items-center justify-center rounded-md border border-accent-dim/35 bg-accent-dim/15 text-accent shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors hover:bg-accent-dim/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/60 disabled:cursor-default disabled:opacity-60"
      style={`right: ${attentionButtonRight};`}
      aria-label="Open session with pending question"
      title={pendingDecision?.question ?? "Session needs attention"}
      onclick={handleAttentionOpen}
      disabled={!attentionSessionId || !onOpen}
    >
      <ClipboardList size={13} strokeWidth={2.2} />
    </button>
  {/if}

  {#if hasMenuActions}
    <button
      type="button"
      class="absolute right-1.5 top-1.5 z-10 flex h-6 w-6 items-center justify-center rounded text-text-muted/70 opacity-0 transition-colors hover:bg-bg-hover hover:text-text-primary focus:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 group-hover:opacity-100"
      aria-label="Card actions"
      aria-haspopup="menu"
      aria-expanded={menuOpen}
      onclick={handleMenuButtonClick}
    >
      <MoreVertical size={14} strokeWidth={2.1} />
    </button>
  {/if}

  <div class="flex min-w-0 items-start gap-2 pr-16">
    {#if onEdit}
      <button
        type="button"
        class="min-w-0 flex-1 text-left text-[13px] font-semibold leading-snug text-text-primary transition-colors hover:text-accent focus-visible:outline-none"
        onclick={() => onEdit?.(item.id)}
        aria-label="Edit card"
      >
        {item.title}
      </button>
    {:else}
      <p
        class="min-w-0 flex-1 text-[13px] font-semibold leading-snug text-text-primary"
      >
        {item.title}
      </p>
    {/if}
    {#if unreadActivity.count > 0}
      <button
        type="button"
        class="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded bg-accent-dim/30 px-1 text-[9px] font-semibold tabular-nums text-accent transition-colors hover:bg-accent-dim/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-default disabled:opacity-70"
        title={unreadActivityTitle}
        aria-label="Open session with unread activity"
        onclick={handleUnreadActivityOpen}
        disabled={!unreadActivity.targetSessionId || !onOpen}
        >{unreadActivity.count > 99 ? "99+" : unreadActivity.count}</button
      >
    {/if}
  </div>

  {#if item.body}
    <p class="line-clamp-2 text-[11px] leading-4 text-text-muted">
      {item.body}
    </p>
  {/if}

  {#if hasPendingQuestion || workflowStageName || hasAttachedPlan || projectLabel || profileLabel || targetLabel || branchLabel}
    <div class="flex flex-wrap gap-1.5">
      {#if hasPendingQuestion}
        {#if onEdit || (attentionSessionId && onOpen)}
          <button
            type="button"
            class="inline-flex min-w-0 max-w-full items-center gap-1 rounded-md border border-amber/35 bg-amber/12 px-1.5 py-0.5 text-[10px] font-semibold leading-4 text-amber transition-colors hover:bg-amber/18 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-amber/50"
            title={pendingDecision?.question ?? "Session needs attention"}
            aria-label="Open pending question"
            onclick={handleQuestionChipClick}
          >
            <MessageSquareWarning
              size={10}
              strokeWidth={2.2}
              class="shrink-0"
            />
            <span class="truncate">Question</span>
          </button>
        {:else}
          <span
            class="inline-flex min-w-0 max-w-full items-center gap-1 rounded-md border border-amber/35 bg-amber/12 px-1.5 py-0.5 text-[10px] font-semibold leading-4 text-amber"
            title={pendingDecision?.question ?? "Session needs attention"}
          >
            <MessageSquareWarning
              size={10}
              strokeWidth={2.2}
              class="shrink-0"
            />
            <span class="truncate">Question</span>
          </span>
        {/if}
      {/if}
      {#if workflowStageName}
        <span class={chipClass}>
          <span class="truncate">{workflowStageName}</span>
        </span>
      {/if}
      {#if hasAttachedPlan}
        {#if onEdit}
          <button
            type="button"
            class="inline-flex min-w-0 max-w-full items-center gap-1 rounded-md border border-green/30 bg-green/10 px-1.5 py-0.5 text-[10px] leading-4 text-green transition-colors hover:bg-green/15 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-green/50"
            title="Open plan attachments"
            aria-label="Open plan attachments"
            onclick={handlePlanChipClick}
          >
            <ClipboardList size={10} strokeWidth={2.2} class="shrink-0" />
            <span class="truncate">Plan</span>
          </button>
        {:else}
          <span
            class="inline-flex min-w-0 max-w-full items-center gap-1 rounded-md border border-green/30 bg-green/10 px-1.5 py-0.5 text-[10px] leading-4 text-green"
            title="Plan attached"
          >
            <ClipboardList size={10} strokeWidth={2.2} class="shrink-0" />
            <span class="truncate">Plan</span>
          </span>
        {/if}
      {/if}
      {#if projectLabel}
        <span class={chipClass}>
          <span class="truncate">{projectLabel}</span>
        </span>
      {/if}
      {#if profileLabel}
        <span class={chipClass}>
          <Bot size={10} strokeWidth={2.2} class="shrink-0" />
          <span class="truncate">{profileLabel}</span>
        </span>
      {/if}
      {#if targetLabel}
        {#if item.worktreePath && onOpenWorktree}
          <button
            type="button"
            class={chipClass +
              " transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"}
            title={item.worktreePath}
            aria-label="Open worktree"
            onclick={handleOpenItemWorktree}
          >
            <span class="truncate font-mono">{targetLabel}</span>
          </button>
        {:else}
          <span
            class={chipClass}
            title={item.worktreePath ?? item.repoPath ?? undefined}
          >
            <span class="truncate font-mono">{targetLabel}</span>
          </span>
        {/if}
      {/if}
      {#if branchLabel}
        <span class={chipClass}>
          <GitBranch size={10} strokeWidth={2.2} class="shrink-0" />
          <span class="truncate font-mono">{branchLabel}</span>
        </span>
      {/if}
    </div>
  {/if}

  {#if item.status === "review" && reviewPackage}
    <div
      class="mt-0.5 flex flex-col gap-1.5 border-t border-border-subtle/55 pt-1.5 text-[10px] leading-4 text-text-muted"
      data-testid="work-item-review-package"
    >
      <div
        class="grid min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] gap-x-1 gap-y-0.5"
      >
        {#if reviewStageName}
          <span class="text-text-subtle">Stage</span>
          <span data-testid="work-item-review-stage">{reviewStageName}</span>
        {/if}
        {#if reviewPackage.agentSummary}
          <span class="text-text-subtle">Summary</span>
          <span class="line-clamp-2">{reviewPackage.agentSummary}</span>
        {/if}
        {#if reviewPackage.tests}
          <span class="text-text-subtle">Tests</span>
          <span class="line-clamp-2 whitespace-pre-line"
            >{reviewPackage.tests}</span
          >
        {/if}
        {#if reviewPackage.changedFiles.length > 0}
          <span class="text-text-subtle">Files</span>
          <span class="truncate" title={reviewPackage.changedFiles.join("\n")}
            >{reviewPackage.changedFiles.join(", ")}</span
          >
        {/if}
        {#if reviewPackage.worktreeLabel}
          <span class="text-text-subtle">Worktree</span>
          {#if canOpenReviewWorktree}
            <button
              type="button"
              class="min-w-0 truncate text-left font-mono text-text-secondary transition-colors hover:text-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              title={reviewPackage.worktreePath ?? undefined}
              aria-label="Open worktree"
              onclick={handleOpenWorktree}
            >
              {reviewPackage.worktreeLabel}
            </button>
          {:else}
            <span
              class="truncate font-mono"
              title={reviewPackage.worktreePath ?? undefined}
              >{reviewPackage.worktreeLabel}</span
            >
          {/if}
        {/if}
        {#if reviewPackage.branch}
          <span class="text-text-subtle">Branch</span>
          <span class="truncate font-mono">{reviewPackage.branch}</span>
        {/if}
        {#if reviewCiChip}
          {@const Icon = reviewCiChip.icon}
          {@const running = reviewWorktreeMeta?.ciStatus === "running"}
          <span class="text-text-subtle">CI</span>
          <span
            class={`inline-flex min-w-0 items-center gap-1 ${reviewCiChip.color} ${reviewWorktreeMeta?.ciStale ? "opacity-60" : ""}`}
            title={`CI: ${reviewCiChip.label}${reviewWorktreeMeta?.ciStale ? " (stale)" : ""}`}
            aria-label={`CI ${reviewCiChip.label}`}
          >
            <Icon size={11} class={running ? "animate-spin" : ""} />
            <span class="truncate">{reviewCiChip.label}</span>
          </span>
        {/if}
        {#if reviewSessionId && onOpen}
          <span class="text-text-subtle">Session</span>
          <button
            type="button"
            class="min-w-0 truncate text-left font-mono text-text-secondary transition-colors hover:text-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
            aria-label="Open terminal"
            onclick={handleOpenReviewTerminal}
          >
            {reviewSessionId}
          </button>
        {/if}
        {#if reviewPackage.prUrl}
          <span class="text-text-subtle">PR</span>
          <a
            href={reviewPackage.prUrl}
            target="_blank"
            rel="noreferrer"
            class="truncate text-green underline-offset-2 hover:underline"
            >{reviewPackage.prUrl}</a
          >
        {/if}
      </div>
    </div>
  {/if}

  {#if startError}
    <p class="text-[11px] leading-snug text-red" role="alert">{startError}</p>
  {/if}

  {#if requestChangesOpen && onRequestChanges && item.status === "review"}
    <form
      class="flex flex-col gap-1.5 border-t border-border-subtle/55 pt-1.5"
      onsubmit={(event) => {
        event.preventDefault();
        void handleSubmitRequestChanges();
      }}
      data-testid="work-item-request-changes-form"
    >
      <textarea
        class="min-h-16 w-full resize-y rounded-md border border-border-subtle bg-bg-deep/70 px-2 py-1.5 text-[11px] leading-4 text-text-primary placeholder:text-text-muted/60 focus:border-accent-dim focus:outline-none focus:ring-1 focus:ring-accent-dim/50"
        placeholder="Requested changes"
        bind:value={requestChangesNote}
        disabled={requestChangesPending}
      ></textarea>
      <div class="flex items-center justify-end gap-1">
        <button
          type="button"
          class="inline-flex h-6 items-center rounded-md px-2 text-[10px] text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
          onclick={() => (requestChangesOpen = false)}
          disabled={requestChangesPending}
        >
          Cancel
        </button>
        <button
          type="submit"
          class="inline-flex h-6 items-center rounded-md border border-amber/30 bg-amber/10 px-2 text-[10px] font-semibold text-amber transition-colors hover:bg-amber/15 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-amber/50 disabled:cursor-wait disabled:opacity-60"
          disabled={requestChangesPending || !requestChangesNote.trim()}
        >
          {requestChangesPending ? "Requesting..." : "Request changes"}
        </button>
      </div>
    </form>
  {/if}

  {#if item.status === "review"}
    <div class="flex items-center gap-1.5 pt-0.5">
      {#if canRunWorkflowStage}
        <button
          type="button"
          class={accentActionClass}
          onclick={handleRunStage}
          aria-label="Run workflow stage"
          aria-busy={stagePending}
          disabled={stagePending}
        >
          <Play size={10} fill="currentColor" strokeWidth={2.2} />
          <span class="truncate"
            >{stagePending ? "Running..." : workflowStageActionText}</span
          >
        </button>
      {/if}
      {#if canFixCi}
        <button
          type="button"
          class={reviewChangesActionClass}
          onclick={handleFixCi}
          aria-label="Fix CI"
          aria-busy={startPending}
          disabled={startPending}
        >
          <Wrench size={12} strokeWidth={2.2} />
          <span class="truncate">{startPending ? "Starting..." : "Fix CI"}</span
          >
        </button>
      {/if}
      {#if canOpenReviewAgent}
        <button
          type="button"
          class={reviewAgentActionClass}
          onclick={handleOpenReviewAgent}
          aria-label="Open agent"
          aria-busy={openAgentPending}
          disabled={openAgentPending}
        >
          <Bot size={12} strokeWidth={2.2} />
          <span class="truncate"
            >{openAgentPending ? "Opening..." : "Open agent"}</span
          >
        </button>
      {/if}
      {#if onRequestChanges}
        <button
          type="button"
          class={reviewChangesActionClass}
          onclick={handleRequestChangesOpen}
          aria-label="Request changes"
          aria-busy={requestChangesPending}
          disabled={requestChangesPending}
        >
          <MessageSquareWarning size={12} strokeWidth={2.2} />
          <span class="truncate"
            >{requestChangesPending ? "Requesting..." : "Request changes"}</span
          >
        </button>
      {/if}
      {#if onAcceptReview}
        <button
          type="button"
          class={reviewDoneActionClass}
          onclick={handleAcceptReview}
          aria-label="Accept work item review"
          aria-busy={acceptPending}
          disabled={acceptPending}
        >
          <Check size={12} strokeWidth={2.2} />
          <span class="truncate"
            >{acceptPending ? "Accepting..." : acceptReviewText}</span
          >
        </button>
      {/if}
    </div>
  {:else}
    <div class="flex items-center gap-1.5 pt-0.5">
      {#if canRunWorkflowStage}
        <button
          class={accentActionClass}
          onclick={handleRunStage}
          aria-label="Run workflow stage"
          aria-busy={stagePending}
          disabled={stagePending}
        >
          <Play size={10} fill="currentColor" strokeWidth={2.2} />
          <span class="truncate"
            >{stagePending ? "Running..." : workflowStageActionText}</span
          >
        </button>
      {:else if phase.action.kind === "plan" && onPlan}
        <button
          class={amberActionClass}
          onclick={() => onPlan?.(item.id, item)}
          aria-label="Plan work item"
          aria-busy={planPending}
          disabled={planPending}
        >
          <ClipboardList size={11} strokeWidth={2.2} />
          <span>{planPending ? "Planning..." : "Plan"}</span>
        </button>
      {:else if phase.action.kind === "accept-review" && onAcceptReview}
        <button
          class={doneActionClass}
          onclick={() => onAcceptReview?.(item.id, item)}
          aria-label="Accept work item review"
          aria-busy={acceptPending}
          disabled={acceptPending}
        >
          <Check size={11} strokeWidth={2.2} />
          <span class="truncate"
            >{acceptPending ? "Accepting..." : acceptReviewText}</span
          >
        </button>
      {:else if phase.action.kind === "open-session" && onOpen}
        <button
          class={accentActionClass}
          onclick={() => primaryOpenSessionId && onOpen?.(primaryOpenSessionId)}
          aria-label="Open terminal"
        >
          <Terminal size={11} strokeWidth={2.2} />
          <span>Open terminal</span>
        </button>
      {:else if phase.action.kind === "open-planning" && onOpen}
        <button
          class={amberActionClass}
          onclick={() =>
            planningOpenSessionId && onOpen?.(planningOpenSessionId)}
          aria-label="Open planning terminal"
        >
          <Terminal size={11} strokeWidth={2.2} />
          <span>Open planning terminal</span>
        </button>
      {:else if (phase.action.kind === "approve-start" || phase.action.kind === "configure" || phase.action.kind === "start") && onStart}
        <button
          class={accentActionClass}
          onclick={() => onStart?.(item.id, item)}
          aria-label={startActionAriaLabel}
          aria-busy={startPending}
          disabled={startPending}
        >
          <Play size={10} fill="currentColor" strokeWidth={2.2} />
          <span>{startPending ? "Starting..." : startActionText}</span>
        </button>
      {/if}
    </div>
  {/if}
</div>

{#if menuOpen}
  <div
    class="fixed z-[80] min-w-40 rounded-lg border border-border-subtle bg-bg-surface/95 p-1 shadow-[0_18px_48px_rgba(0,0,0,0.45),inset_0_1px_0_rgba(255,255,255,0.06)] backdrop-blur"
    style={`left: ${menuX}px; top: ${menuY}px;`}
    role="menu"
    aria-label="Card actions"
    tabindex="-1"
  >
    {#if onEdit}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={handleEdit}
      >
        <Pencil size={13} strokeWidth={2.1} />
        <span>Edit card</span>
      </button>
    {/if}
    {#if onPlan && !hasSession && !hasPlanningSession && (!isPlanning || !hasAttachedPlan)}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
        onclick={handlePlan}
        disabled={planPending}
      >
        <ClipboardList size={13} strokeWidth={2.1} />
        <span>{planPending ? "Planning..." : "Plan"}</span>
      </button>
    {/if}
    {#if onPlan && hasPlanningSession && !hasSession}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
        onclick={handleReplan}
        disabled={planPending}
      >
        <ClipboardList size={13} strokeWidth={2.1} />
        <span>{planPending ? "Replanning..." : "Retry planning"}</span>
      </button>
    {/if}
    {#if canForceStartPlanning}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
        onclick={handleForceStart}
        disabled={startPending}
      >
        <Play size={13} fill="currentColor" strokeWidth={2.1} />
        <span>{startPending ? "Starting..." : "Approve & start anyway"}</span>
      </button>
    {/if}
    {#if onArchive}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
        onclick={handleArchive}
        disabled={archivePending}
      >
        <Archive size={13} strokeWidth={2.1} />
        <span>{archivePending ? "Archiving..." : "Archive card"}</span>
      </button>
    {/if}
    {#if onDelete}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-red transition-colors hover:bg-red/12 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/50"
        onclick={handleDelete}
      >
        <Trash2 size={13} strokeWidth={2.1} />
        <span>Delete card</span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .work-card {
    overflow: hidden;
    border: 1px solid
      color-mix(in srgb, var(--color-border-subtle) 82%, transparent);
    border-radius: 8px;
    background: linear-gradient(
      180deg,
      color-mix(
          in srgb,
          var(--color-bg-surface) 88%,
          var(--color-bg-elevated) 12%
        )
        0%,
      color-mix(in srgb, var(--color-bg-surface) 74%, var(--color-bg-deep) 26%)
        100%
    );
    box-shadow:
      0 10px 24px rgba(0, 0, 0, 0.22),
      inset 0 1px 0 rgba(255, 255, 255, 0.045);
    transition:
      transform 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease,
      background 140ms ease;
  }

  .work-card::before {
    position: absolute;
    inset: 0 auto 0 0;
    width: 2px;
    content: "";
    background: color-mix(in srgb, var(--color-border) 72%, transparent);
  }

  .work-card::after {
    position: absolute;
    inset: 0;
    pointer-events: none;
    content: "";
    background: linear-gradient(
      120deg,
      rgba(255, 255, 255, 0.055),
      transparent 34%
    );
    opacity: 0.58;
  }

  .work-card:hover {
    transform: translateY(-1px);
    border-color: color-mix(
      in srgb,
      var(--color-border) 82%,
      var(--color-accent) 18%
    );
    box-shadow:
      0 16px 34px rgba(0, 0, 0, 0.3),
      0 0 0 1px color-mix(in srgb, var(--color-accent-dim) 16%, transparent),
      inset 0 1px 0 rgba(255, 255, 255, 0.06);
  }

  .work-card[data-session-bound="true"]::before {
    background: var(--color-accent);
    box-shadow: 0 0 18px
      color-mix(in srgb, var(--color-accent) 44%, transparent);
  }

  .work-card[data-error="true"] {
    border-color: color-mix(in srgb, var(--color-red) 42%, var(--color-border));
  }

  .work-card[data-error="true"]::before {
    background: var(--color-red);
    box-shadow: 0 0 18px color-mix(in srgb, var(--color-red) 38%, transparent);
  }

  .work-card[data-blocked="true"] {
    border-color: color-mix(
      in srgb,
      var(--color-accent) 34%,
      var(--color-border)
    );
  }

  .work-card[data-blocked="true"]::before {
    background: var(--color-accent);
    box-shadow: 0 0 18px
      color-mix(in srgb, var(--color-accent) 34%, transparent);
  }

  @media (prefers-reduced-motion: reduce) {
    .work-card {
      transition:
        border-color 140ms ease,
        box-shadow 140ms ease;
    }

    .work-card:hover {
      transform: none;
    }
  }
</style>
