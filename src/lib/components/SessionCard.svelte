<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { Session } from "$lib/types";
  import { renameSignal, sessionDisplayName } from "$lib/stores/sessions";
  import { projects } from "$lib/stores/projects";
  import { flashingSessions } from "$lib/stores/watches";
  import { unreadBySession } from "$lib/stores/notifications";
  import { showSessionHints } from "$lib/stores/ui";
  import {
    sessionAgentStatus,
    computeEffectiveSessionStatus,
  } from "$lib/panes/agentState";
  import { listSessionPtys } from "$lib/tauri";
  import CloseButton from "./CloseButton.svelte";
  import Pencil from "@lucide/svelte/icons/pencil";
  import SessionWorktrunkChips from "./SessionWorktrunkChips.svelte";

  interface Props {
    session: Session;
    active: boolean;
    slotNumber?: number;
    hideProjectTag?: boolean;
    onselect: () => void;
    onclose: () => void;
    onrename: (newName: string) => void;
    onreconnect: () => void;
    oncontextmenu?: (e: MouseEvent) => void;
  }

  let {
    session,
    active,
    slotNumber,
    hideProjectTag = false,
    onselect,
    onclose,
    onrename,
    onreconnect,
    oncontextmenu,
  }: Props = $props();

  let slotLabel = $derived(
    slotNumber == null ? null : slotNumber === 10 ? "0" : String(slotNumber),
  );

  // Poll PTY inventory so the sidebar can show how many panes are active and
  // whether a session is carrying detached terminals in the background.
  let attachedCount = $state(0);
  let detachedCount = $state(0);
  let detachedHasUnread = $state(false);
  let showPaneInventory = $derived(attachedCount > 1 || detachedCount > 0);
  let activePaneTitle = $derived(
    `${attachedCount} active pane${attachedCount === 1 ? "" : "s"}`
  );
  let detachedPaneTitle = $derived(
    `${detachedCount} detached terminal${detachedCount === 1 ? "" : "s"}${detachedHasUnread ? " (unread output)" : ""}`
  );

  async function refreshDetachedState() {
    try {
      const ptys = await listSessionPtys(session.id);
      const attached = ptys.filter((p) => p.status.type === "RunningAttached");
      const detached = ptys.filter((p) => p.status.type === "RunningDetached");
      attachedCount = attached.length;
      detachedCount = detached.length;
      detachedHasUnread = detached.some((p) => p.unread_output);
    } catch {
      // Non-fatal; badge stays at last known value
    }
  }

  let pollTimer: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    void refreshDetachedState();
    pollTimer = setInterval(() => void refreshDetachedState(), 5000);
  });

  onDestroy(() => {
    if (pollTimer !== null) clearInterval(pollTimer);
  });

  let displayName = $derived(sessionDisplayName(session));

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
  let showProjectTag = $derived(!hideProjectTag && projectName != null);

  let isFlashing = $derived($flashingSessions.has(session.id));
  let unreadCount = $derived($unreadBySession.get(session.id) ?? 0);

  let agentAggregate = $derived($sessionAgentStatus.get(session.id) ?? null);
  let effectiveStatus = $derived(
    computeEffectiveSessionStatus(session.status, agentAggregate),
  );

  let showRow2 = $derived(
    session.isWorktree || showProjectTag || session.cost != null
  );

  let tooltip = $derived(
    session.isGitRepo && session.branch
      ? `${session.branch} · ${session.worktreePath}`
      : session.worktreePath,
  );
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative mb-1 flex w-full cursor-pointer overflow-hidden text-left transition-colors duration-150
    {active
      ? 'bg-bg-active shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]'
      : 'bg-transparent hover:bg-bg-active/40'}
    {isFlashing ? 'watch-flash' : ''}"
  onclick={onselect}
  oncontextmenu={(e) => {
    if (oncontextmenu) {
      e.preventDefault();
      oncontextmenu(e);
    }
  }}
  title={tooltip}
>
  <!-- Left gutter: persistent status dot -->
  <div class="flex h-9 w-5 shrink-0 items-center justify-center self-start">
    <span class="relative inline-flex h-2 w-2 items-center justify-center">
      {#if effectiveStatus === "attention"}
        <span class="absolute inline-flex h-2 w-2 rounded-full {statusDotClasses[effectiveStatus]} animate-ping opacity-60"></span>
      {/if}
      <span class="relative inline-flex h-2 w-2 rounded-full {statusDotClasses[effectiveStatus]}"></span>
    </span>
  </div>

  <!-- Body -->
  <div class="min-w-0 flex-1 py-2 pr-2">
    <div class="flex items-center gap-2">
      {#if editing}
        <input
          class="h-5 min-w-0 flex-1 border border-accent-dim/30 bg-bg-deep px-1.5 py-0 text-[13px] font-semibold leading-none tracking-tight text-text-primary outline-none"
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
          class="flex-1 truncate text-[13px] font-semibold tracking-tight {active ? 'text-text-primary' : 'text-text-secondary'}"
          ondblclick={startEditing}
        >
          {displayName}
        </span>
        <button
          type="button"
          class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center border border-transparent bg-transparent p-0 text-text-muted opacity-0 transition-colors duration-150 hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 group-hover:opacity-100"
          onclick={startEditing}
          aria-label="Rename session"
          title="Rename session"
        >
          <Pencil size={12} />
        </button>
      {/if}

      {#if showPaneInventory}
        <span
          class="inline-flex h-4 min-w-[16px] shrink-0 items-center justify-center rounded px-1 text-[9px] font-semibold tabular-nums bg-bg-surface text-text-muted"
          title={activePaneTitle}
        >{attachedCount}</span>
      {/if}
      {#if detachedCount > 0}
        <span
          class="inline-flex h-4 min-w-[16px] shrink-0 items-center justify-center rounded px-1 text-[9px] font-semibold tabular-nums
            {detachedHasUnread
              ? 'bg-accent text-white'
              : 'bg-bg-surface text-text-muted'}"
          title={detachedPaneTitle}
        >{detachedCount}</span>
      {/if}
      {#if unreadCount > 0}
        <span
          class="inline-flex h-4 min-w-[16px] shrink-0 items-center justify-center rounded-full bg-accent-dim/30 px-1 text-[9px] font-semibold text-accent"
          title="{unreadCount} unread notification{unreadCount === 1 ? '' : 's'}"
        >{unreadCount > 99 ? "99+" : unreadCount}</span>
      {/if}
      {#if session.status === "disconnected"}
        <button
          class="cursor-pointer border border-accent-dim/20 bg-accent-dim/15 px-2 py-0.5 text-[11px] font-semibold text-accent hover:bg-accent-dim/24"
          onclick={(e) => { e.stopPropagation(); onreconnect(); }}
        >
          reconnect
        </button>
      {/if}
      <CloseButton
        class="flex h-5 w-5 items-center justify-center p-0 opacity-70 duration-150 group-hover:opacity-100 hover:border-transparent hover:text-red"
        onclick={(e) => { e.stopPropagation(); onclose(); }}
        label="Close session"
        title="Close session"
        size={13}
      />
    </div>

    {#if showRow2}
      <div class="mt-1 flex items-center gap-2 text-[10px] text-text-muted">
        {#if session.isWorktree}
          <span class="flex items-center gap-1 font-mono text-text-secondary">
            <span class="opacity-70">&#9095;</span>
            <span>worktree</span>
          </span>
        {/if}
        {#if session.isWorktree}
          <SessionWorktrunkChips worktreePath={session.worktreePath} />
        {/if}
        {#if showProjectTag}
          <span class="bg-accent-dim/15 px-1.5 py-0.5 font-semibold text-accent">{projectName}</span>
        {/if}
        {#if session.cost != null}
          <span class="ml-auto font-semibold">${session.cost.toFixed(2)}</span>
        {/if}
      </div>
    {/if}
  </div>

  {#if slotLabel}
    <div
      class="slot-hint-overlay pointer-events-none absolute inset-0 flex items-center justify-center bg-bg-deep/75 backdrop-blur-[1px] transition-opacity duration-[120ms]"
      class:slot-hint-visible={$showSessionHints}
      aria-hidden="true"
      style:--flash-color="transparent"
    >
      <span class="font-mono text-[28px] font-bold leading-none text-text-primary drop-shadow-[0_1px_4px_rgba(0,0,0,0.6)]">
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
