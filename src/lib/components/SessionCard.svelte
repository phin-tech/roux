<script lang="ts">
  import type { GroupBy, Session } from "$lib/types";
  import { renameSignal, sessionDisplayName } from "$lib/stores/sessions";
  import { projects } from "$lib/stores/projects";
  import { flashingSessions } from "$lib/stores/watches";
  import { unreadBySession } from "$lib/stores/notifications";
  import { showSessionHints } from "$lib/stores/ui";
  import { ptyInventoryBySession } from "$lib/stores/ptyInventory";
  import { experimentValues } from "$lib/experiments";
  import {
    sessionAgentStatus,
    computeEffectiveSessionStatus,
  } from "$lib/panes/agentState";
  import CloseButton from "./CloseButton.svelte";
  import Pencil from "@lucide/svelte/icons/pencil";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import SessionWorktrunkChips from "./SessionWorktrunkChips.svelte";

  interface Props {
    session: Session;
    active: boolean;
    groupBy: GroupBy;
    slotNumber?: number;
    onselect: () => void;
    onclose: () => void;
    onrename: (newName: string) => void;
    onreconnect: () => void;
    oncontextmenu?: (e: MouseEvent) => void;
  }

  let {
    session,
    active,
    groupBy,
    slotNumber,
    onselect,
    onclose,
    onrename,
    onreconnect,
    oncontextmenu,
  }: Props = $props();

  let simplified = $derived($experimentValues.simplifiedSessionTabs);
  // Split on both separators so Windows paths (e.g. C:\src\repo\.worktrees\foo)
  // resolve to the trailing component, not the entire absolute path.
  let worktreeName = $derived(pathBasename(session.worktreePath));
  let repoName = $derived(pathBasename(session.repoRoot));
  let contextualSecondary = $derived(
    groupBy === "project" ? repoName : worktreeName,
  );

  function pathBasename(path: string): string {
    return path.split(/[\\/]+/).filter(Boolean).pop() ?? "";
  }

  let slotLabel = $derived(
    slotNumber == null ? null : slotNumber === 10 ? "0" : String(slotNumber),
  );

  // Shared PTY inventory lets the sidebar show pane counts without one poller
  // per rendered session row.
  let ptyInventory = $derived($ptyInventoryBySession.get(session.id));
  let attachedCount = $derived(ptyInventory?.attachedCount ?? 0);
  let detachedCount = $derived(ptyInventory?.detachedCount ?? 0);
  let detachedHasUnread = $derived(ptyInventory?.detachedHasUnread ?? false);
  let showPaneInventory = $derived(attachedCount > 1 || detachedCount > 0);
  let activePaneTitle = $derived(
    `${attachedCount} active pane${attachedCount === 1 ? "" : "s"}`
  );
  let detachedPaneTitle = $derived(
    `${detachedCount} detached terminal${detachedCount === 1 ? "" : "s"}${detachedHasUnread ? " (unread output)" : ""}`
  );

  let displayName = $derived(sessionDisplayName(session));
  let hasCustomName = $derived(Boolean(session.nameOverride?.trim()));
  let primaryLabel = $derived(
    session.isWorktree && session.branch && !hasCustomName
      ? session.branch
      : displayName,
  );
  let secondaryBranch = $derived(
    session.isWorktree && session.branch && hasCustomName && session.branch !== displayName
      ? session.branch
      : null,
  );
  let branchParts = $derived.by(() => {
    const shouldSplitBranchPrimary =
      session.isWorktree && !hasCustomName && primaryLabel.includes("/");
    if (!shouldSplitBranchPrimary) {
      return { prefix: "", tail: primaryLabel };
    }
    const slash = primaryLabel.lastIndexOf("/");
    return {
      prefix: primaryLabel.slice(0, slash + 1),
      tail: primaryLabel.slice(slash + 1),
    };
  });

  let editing = $state(false);
  let editName = $state("");

  $effect(() => {
    if (!editing) editName = displayName;
  });

  let lastSignal = $state($renameSignal);
  $effect(() => {
    if ($renameSignal !== lastSignal) {
      lastSignal = $renameSignal;
      if (active) {
        editName = displayName;
        editing = true;
      }
    }
  });

  function startEditing(e: MouseEvent) {
    e.stopPropagation();
    editName = displayName;
    editing = true;
  }

  function commitRename() {
    editing = false;
    const trimmed = editName.trim();
    if (trimmed && trimmed !== displayName) onrename(trimmed);
  }

  // Only "attention" animates. All other statuses use a solid dot.
  const statusDotClasses: Record<Session["status"], string> = {
    idle: "bg-green",
    thinking: "bg-amber",
    generating: "bg-blue",
    error: "bg-red",
    disconnected: "bg-gray",
    attention: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)]",
  };

  let projectName = $derived(
    session.projectId ? $projects.find((p) => p.id === session.projectId)?.name ?? null : null
  );
  let showProjectTag = $derived(groupBy !== "project" && projectName != null);

  let isFlashing = $derived($flashingSessions.has(session.id));
  let unreadCount = $derived($unreadBySession.get(session.id) ?? 0);

  let agentAggregate = $derived($sessionAgentStatus.get(session.id) ?? null);
  let effectiveStatus = $derived(
    computeEffectiveSessionStatus(session.status, agentAggregate),
  );

  let showRow2 = $derived(
    simplified
      ? Boolean(secondaryBranch || contextualSecondary)
      : session.isWorktree || showProjectTag || session.cost != null,
  );

  let detailLabel = $derived(
    session.isGitRepo && session.branch
      ? `${session.branch} · ${session.worktreePath}`
      : session.worktreePath,
  );
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative mb-1 flex w-full cursor-pointer overflow-hidden border-l-2 text-left transition-colors duration-150
    {active
      ? 'border-accent bg-bg-active shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]'
      : 'border-transparent bg-transparent hover:bg-bg-active/30'}
    {isFlashing ? 'watch-flash' : ''}"
  onclick={onselect}
  oncontextmenu={(e) => {
    if (oncontextmenu) {
      e.preventDefault();
      oncontextmenu(e);
    }
  }}
>
  <!-- Left gutter: persistent status dot -->
  <div class="flex h-10 w-5 shrink-0 items-start justify-center pt-[13px]">
    <span class="relative inline-flex h-2 w-2 items-center justify-center">
      {#if effectiveStatus === "attention"}
        <span class="absolute inline-flex h-2 w-2 rounded-full {statusDotClasses[effectiveStatus]} animate-ping opacity-60"></span>
      {/if}
      <span class="relative inline-flex h-2 w-2 rounded-full {statusDotClasses[effectiveStatus]}"></span>
    </span>
  </div>

  <!-- Body -->
  <div class="min-w-0 flex-1 py-1.5 pr-2">
    <div class="flex min-h-5 items-center gap-1.5">
      <div class="min-w-0 flex-1">
        {#if editing}
          <input
            class="h-5 w-full min-w-0 border border-accent-dim/30 bg-bg-deep px-1.5 py-0 text-[13px] font-semibold leading-none text-text-primary outline-none"
            bind:value={editName}
            onblur={commitRename}
            onkeydown={(e) => {
              if (e.key === "Enter") { e.stopPropagation(); commitRename(); }
              if (e.key === "Escape") { e.stopPropagation(); editing = false; }
            }}
          />
        {:else}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            data-testid="session-primary-label"
            class="flex min-w-0 items-baseline text-[13px] font-semibold leading-5 {active ? 'text-text-primary' : 'text-text-secondary'}"
            title={detailLabel}
            ondblclick={startEditing}
          >
            {#if session.isWorktree && branchParts.prefix}
              <span data-testid="session-primary-prefix" class="min-w-0 truncate text-text-muted">{branchParts.prefix}</span>
              <span data-testid="session-primary-tail" class="min-w-0 truncate">{branchParts.tail}</span>
            {:else}
              <span class="min-w-0 truncate">{primaryLabel}</span>
            {/if}
          </span>
        {/if}
      </div>

      <div class="flex shrink-0 items-center gap-1">
        {#if !simplified && showPaneInventory}
          <span
            class="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded bg-bg-surface px-1 text-[9px] font-semibold tabular-nums text-text-muted"
            title={activePaneTitle}
          >{attachedCount}</span>
        {/if}
        {#if !simplified && detachedCount > 0}
          <span
            class="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded px-1 text-[9px] font-semibold tabular-nums
              {detachedHasUnread
                ? 'bg-accent text-white'
                : 'bg-bg-surface text-text-muted'}"
            title={detachedPaneTitle}
          >{detachedCount}</span>
        {/if}
        {#if unreadCount > 0}
          <span
            class="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded bg-accent-dim/30 px-1 text-[9px] font-semibold tabular-nums text-accent"
            title="{unreadCount} unread notification{unreadCount === 1 ? '' : 's'}"
          >{unreadCount > 99 ? "99+" : unreadCount}</span>
        {/if}
        {#if session.status === "disconnected"}
          <button
            class="h-5 cursor-pointer border border-accent-dim/20 bg-accent-dim/15 px-2 py-0 text-[10px] font-semibold text-accent hover:bg-accent-dim/24"
            onclick={(e) => { e.stopPropagation(); onreconnect(); }}
          >
            continue
          </button>
        {/if}
        {#if !editing}
          <button
            type="button"
            class="pointer-events-none flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center border border-transparent bg-transparent p-0 text-text-muted opacity-0 transition-colors duration-150 hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:pointer-events-auto focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 group-hover:pointer-events-auto group-hover:opacity-100"
            onclick={startEditing}
            aria-label="Rename session"
            title="Rename session"
          >
            <Pencil size={12} />
          </button>
        {/if}
        <CloseButton
          class="pointer-events-none flex h-5 w-5 items-center justify-center p-0 opacity-0 duration-150 hover:border-transparent hover:text-red focus-visible:pointer-events-auto focus-visible:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100"
          onclick={(e) => { e.stopPropagation(); onclose(); }}
          label="Close session"
          title="Close session"
          size={13}
        />
      </div>
    </div>

    {#if showRow2}
      {#if simplified}
        <div
          data-testid="session-secondary"
          class="mt-0.5 flex min-h-4 min-w-0 items-center gap-1.5 overflow-hidden text-[10px] text-text-muted"
        >
          {#if secondaryBranch}
            <span
              data-testid="session-secondary-branch"
              class="min-w-0 truncate font-mono text-text-muted"
              title={secondaryBranch}
            >{secondaryBranch}</span>
            {#if contextualSecondary}
              <span class="shrink-0 text-text-muted">·</span>
            {/if}
          {/if}
          {#if contextualSecondary}
            <span
              data-testid="session-secondary-context"
              class="min-w-0 truncate"
              title={contextualSecondary}
            >{contextualSecondary}</span>
          {/if}
        </div>
      {:else}
        <div class="mt-0.5 flex min-h-4 items-center gap-1.5 overflow-hidden text-[10px] text-text-muted">
          {#if session.isWorktree}
            <span
              class="inline-flex h-4 shrink-0 items-center gap-1 rounded bg-bg-surface/70 px-1.5 font-medium text-text-secondary"
              title={detailLabel}
            >
              <GitBranch size={10} />
              <span>worktree</span>
            </span>
          {/if}
          {#if secondaryBranch}
            <span class="min-w-0 truncate font-mono text-[10px] text-text-muted" title={secondaryBranch}>
              {secondaryBranch}
            </span>
          {/if}
          {#if session.isWorktree}
            <span class="inline-flex min-w-0 items-center gap-1">
              <SessionWorktrunkChips worktreePath={session.worktreePath} />
            </span>
          {/if}
          {#if showProjectTag}
            <span class="inline-flex h-4 shrink-0 items-center rounded bg-accent-dim/15 px-1.5 font-semibold text-accent">{projectName}</span>
          {/if}
          {#if session.cost != null}
            <span class="ml-auto shrink-0 font-semibold tabular-nums">${session.cost.toFixed(2)}</span>
          {/if}
        </div>
      {/if}
    {/if}
  </div>

  {#if slotLabel}
    <div
      class="slot-hint-overlay pointer-events-none absolute inset-0 flex items-center justify-center bg-bg-deep/75 backdrop-blur-[1px] transition-opacity duration-[120ms]"
      class:slot-hint-visible={$showSessionHints}
      aria-hidden="true"
      style:--flash-color="transparent"
    >
      <span class="text-[28px] font-bold leading-none text-text-primary drop-shadow-[0_1px_4px_rgba(0,0,0,0.6)]">
        &#8984;{slotLabel}
      </span>
    </div>
  {/if}
</div>

<style>
  .watch-flash {
    animation: watch-flash-anim 1.5s ease-out;
  }

  @keyframes watch-flash-anim {
    0% { background-color: var(--color-amber-dim, rgba(245,158,11,0.15)); }
    100% { background-color: transparent; }
  }

  .slot-hint-overlay {
    opacity: 0;
  }

  .slot-hint-overlay.slot-hint-visible {
    opacity: 1;
  }
</style>
