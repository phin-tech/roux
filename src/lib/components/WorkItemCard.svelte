<script lang="ts">
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Bot from "@lucide/svelte/icons/bot";
  import Check from "@lucide/svelte/icons/check";
  import ClipboardList from "@lucide/svelte/icons/clipboard-list";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import MoreVertical from "@lucide/svelte/icons/more-vertical";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Terminal from "@lucide/svelte/icons/terminal";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { WorkItem, WorkItemStatus } from "$lib/bindings";
  import type { WorkItemDecision } from "$lib/types/workItems";
  import type { SessionStatus } from "$lib/types";
  import { profileList } from "$lib/panes/profiles";
  import { clearDraggedWorkItem, writeWorkItemDragData } from "$lib/board/drag";
  import { unreadBySession } from "$lib/stores/notifications";
  import { projects } from "$lib/stores/projects";

  interface Props {
    item: WorkItem;
    sessionStatus?: SessionStatus | null;
    onMove?: (id: string, status: WorkItemStatus) => void;
    onStart?: (id: string, item: WorkItem) => void;
    /** Start or open a planning run for this work item. */
    onPlan?: (id: string, item: WorkItem, replaceActive?: boolean) => void;
    /** Open the card's bound session (by session id). */
    onOpen?: (sessionId: string) => void;
    /** Open the card editor (by work item id). */
    onEdit?: (id: string) => void;
    /** Delete the card (by work item id). */
    onDelete?: (id: string, item: WorkItem) => void;
    /** Accept a review-requested implementation run. */
    onAcceptReview?: (id: string, item: WorkItem) => void;
    startPending?: boolean;
    planPending?: boolean;
    acceptPending?: boolean;
    startError?: string | null;
    pendingDecision?: WorkItemDecision | null;
    planningSessionId?: string | null;
    attachedSessionIds?: string[];
    /** Opt-in card dragging. The full-screen board enables it; the sidebar leaves it off. */
    draggable?: boolean;
  }

  const {
    item,
    sessionStatus = null,
    onMove,
    onStart,
    onPlan,
    onOpen,
    onEdit,
    onDelete,
    onAcceptReview,
    startPending = false,
    planPending = false,
    acceptPending = false,
    startError = null,
    pendingDecision = null,
    planningSessionId = null,
    attachedSessionIds = [],
    draggable = false,
  }: Props = $props();

  const COLUMN_OPTIONS: WorkItemStatus[] = [
    "todo",
    "ready",
    "doing",
    "review",
    "done",
  ];
  const COLUMN_LABELS: Record<WorkItemStatus, string> = {
    todo: "To Do",
    ready: "Ready",
    doing: "In Progress",
    review: "Review",
    done: "Done",
  };

  const statusDotClasses: Partial<Record<SessionStatus, string>> = {
    idle: "bg-green",
    generating: "bg-blue",
    thinking: "bg-blue",
    attention: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)]",
    error: "bg-red",
    disconnected: "bg-muted",
  };

  const hasSession = $derived(!!item.sessionId);
  const hasPlanningSession = $derived(!!planningSessionId);
  const isStartable = $derived(
    !!item.agentProfile && (!!item.repoPath || !!item.projectId),
  );
  const hasMenuActions = $derived(
    !!onEdit || !!onPlan || !!onDelete || !!onAcceptReview,
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
  const attentionSessionId = $derived(
    pendingDecision ? (planningSessionId ?? item.sessionId ?? null) : null,
  );
  const allAttachedSessionIds = $derived.by(() => {
    const ids = new Set<string>();
    if (item.sessionId) ids.add(item.sessionId);
    if (planningSessionId) ids.add(planningSessionId);
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
      ? pendingDecision
        ? "3.5rem"
        : "2rem"
      : pendingDecision
        ? "2rem"
        : "0.5rem",
  );
  const chipClass =
    "inline-flex min-w-0 max-w-full items-center gap-1 rounded-md border border-border-subtle/70 bg-bg-deep/55 px-1.5 py-0.5 text-[10px] leading-4 text-text-muted";

  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);

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

  function handleDelete(): void {
    menuOpen = false;
    onDelete?.(item.id, item);
  }

  function handleAcceptReview(): void {
    menuOpen = false;
    onAcceptReview?.(item.id, item);
  }

  function handleAttentionOpen(event: MouseEvent): void {
    event.stopPropagation();
    if (!attentionSessionId) return;
    onOpen?.(attentionSessionId);
  }

  function handleUnreadActivityOpen(event: MouseEvent): void {
    event.stopPropagation();
    if (!unreadActivity.targetSessionId) return;
    onOpen?.(unreadActivity.targetSessionId);
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") menuOpen = false;
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
  data-blocked={!!pendingDecision}
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

  {#if pendingDecision}
    <button
      type="button"
      class="absolute top-1.5 z-10 flex h-6 w-6 items-center justify-center rounded-md border border-accent-dim/35 bg-accent-dim/15 text-accent shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors hover:bg-accent-dim/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/60 disabled:cursor-default disabled:opacity-60"
      style={`right: ${attentionButtonRight};`}
      aria-label="Open session with pending question"
      title={pendingDecision.question}
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

  {#if projectLabel || profileLabel || targetLabel || branchLabel}
    <div class="flex flex-wrap gap-1.5">
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
        <span
          class={chipClass}
          title={item.worktreePath ?? item.repoPath ?? undefined}
        >
          <span class="truncate font-mono">{targetLabel}</span>
        </span>
      {/if}
      {#if branchLabel}
        <span class={chipClass}>
          <GitBranch size={10} strokeWidth={2.2} class="shrink-0" />
          <span class="truncate font-mono">{branchLabel}</span>
        </span>
      {/if}
    </div>
  {/if}

  {#if startError}
    <p class="text-[11px] leading-snug text-red" role="alert">{startError}</p>
  {/if}

  <div class="flex items-center gap-1.5 pt-0.5">
    <!-- Column quick-move buttons -->
    {#each COLUMN_OPTIONS.filter((c) => c !== item.status && !(item.status === "review" && c === "done" && onAcceptReview)) as col (col)}
      <button
        class="inline-flex h-5 items-center gap-1 rounded px-1.5 text-[10px] text-text-muted/80 transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={() => onMove?.(item.id, col)}
        aria-label="Move to {COLUMN_LABELS[col]}"
      >
        <ArrowRight size={10} strokeWidth={2.25} />
        <span>{COLUMN_LABELS[col]}</span>
      </button>
    {/each}

    {#if hasSession && onOpen}
      <button
        class="ml-auto inline-flex h-6 items-center gap-1.5 rounded-md border border-accent-dim/30 bg-accent-dim/15 px-2 text-[10px] font-semibold text-accent shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors hover:bg-accent-dim/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/60"
        onclick={() => item.sessionId && onOpen?.(item.sessionId)}
        aria-label="Open terminal"
      >
        <Terminal size={11} strokeWidth={2.2} />
        <span>Open terminal</span>
      </button>
    {:else if hasPlanningSession && onOpen}
      <button
        class="ml-auto inline-flex h-6 items-center gap-1.5 rounded-md border border-amber/30 bg-amber/10 px-2 text-[10px] font-semibold text-amber shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors hover:bg-amber/15 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-amber/50"
        onclick={() => planningSessionId && onOpen?.(planningSessionId)}
        aria-label="Open planning terminal"
      >
        <Terminal size={11} strokeWidth={2.2} />
        <span>Open planning terminal</span>
      </button>
    {:else if !hasSession && onStart}
      <button
        class="ml-auto inline-flex h-6 items-center gap-1.5 rounded-md border border-accent-dim/30 bg-accent-dim/15 px-2 text-[10px] font-semibold text-accent shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors hover:bg-accent-dim/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/60 disabled:cursor-wait disabled:opacity-60"
        onclick={() => onStart?.(item.id, item)}
        aria-label="Start work item"
        aria-busy={startPending}
        disabled={startPending}
      >
        <Play size={10} fill="currentColor" strokeWidth={2.2} />
        <span
          >{startPending
            ? "Starting..."
            : isStartable
              ? "Start"
              : "Configure"}</span
        >
      </button>
    {/if}
  </div>
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
    {#if onPlan && !hasSession && !hasPlanningSession}
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
    {#if onAcceptReview && item.status === "review"}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
        onclick={handleAcceptReview}
        disabled={acceptPending}
      >
        <Check size={13} strokeWidth={2.1} />
        <span>{acceptPending ? "Accepting..." : "Accept done"}</span>
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
