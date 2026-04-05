<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import {
    sessionState,
    setActiveSession,
    renameSession,
    addSession,
  } from "$lib/stores/sessions";
  import { initSessionPanes } from "$lib/stores/panes";
  import {
    writeToSession,
    createSession,
    openInEditor,
  } from "$lib/tauri";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { reconnectSession } from "$lib/sessions/reconnect";
  import { closeSession } from "$lib/sessions/close";
  import { refreshTasks, initTaskOverrides } from "$lib/stores/tasks";
  import type { Session } from "$lib/types";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
  }

  let { onNewSession, onOpenSettings }: Props = $props();

  let dragging = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();
  let collapsedGroups = $state(new Set<string>());

  let grouped = $derived.by(() => {
    const map = new Map<string, { name: string; repoRoot: string; sessions: Session[]; latest: number }>();
    for (const s of $sessionState.sessions) {
      let group = map.get(s.repoRoot);
      if (!group) {
        group = {
          name: s.repoRoot.split("/").pop() || s.repoRoot,
          repoRoot: s.repoRoot,
          sessions: [],
          latest: 0,
        };
        map.set(s.repoRoot, group);
      }
      group.sessions.push(s);
      if (s.createdAt > group.latest) group.latest = s.createdAt;
    }
    return [...map.values()].sort((a, b) => b.latest - a.latest);
  });

  let showGroupHeaders = $derived(grouped.length > 0);

  function toggleGroup(repoRoot: string) {
    const next = new Set(collapsedGroups);
    if (next.has(repoRoot)) next.delete(repoRoot);
    else next.add(repoRoot);
    collapsedGroups = next;
  }

  let contextMenu = $state<{ x: number; y: number; session: Session } | null>(null);
  let worktreeInput = $state(false);
  let branchName = $state("");
  let creatingWorktree = $state(false);
  let worktreeError = $state("");

  function handleContextMenu(e: MouseEvent, session: Session) {
    contextMenu = { x: e.clientX, y: e.clientY, session };
    worktreeInput = false;
    branchName = "";
    worktreeError = "";
  }

  function closeContextMenu() {
    contextMenu = null;
    worktreeInput = false;
    branchName = "";
    worktreeError = "";
  }

  function showWorktreeInput() {
    worktreeInput = true;
  }

  async function handleCreateWorktree() {
    if (!contextMenu || !branchName.trim()) return;
    creatingWorktree = true;
    worktreeError = "";
    try {
      const repo = contextMenu.session.repoRoot;
      const branch = branchName.trim();
      const name = repo.split("/").pop() + "-" + branch;
      const session = await createSession(repo, name, null, branch);
      addSession(session);
      initSessionPanes(session.id);
      closeContextMenu();
    } catch (e) {
      worktreeError = String(e);
    } finally {
      creatingWorktree = false;
    }
  }

  async function handleOpenInCode() {
    if (!contextMenu) return;
    await openInEditor(contextMenu.session.worktreePath).catch(() => {});
    closeContextMenu();
  }

  $effect(() => {
    const session = $sessionState.sessions.find((s) => s.id === $sessionState.activeSessionId);
    if (session) {
      void refreshTasks(session.worktreePath);
    }
  });

  $effect(() => {
    void initTaskOverrides();
  });

  async function handleClose(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;
    await closeSession(session);
  }

  async function handleApprove(id: string) {
    await writeToSession(id, "\r");
  }

  async function handleAlways(id: string) {
    await writeToSession(id, "\x1b[Z");
  }

  async function handleDeny(id: string) {
    await writeToSession(id, "\x1b[B\x1b[B\r");
  }

  async function handleReconnect(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;
    await reconnectSession(session);
  }

  function handleDividerDown(e: MouseEvent) {
    e.preventDefault();
    dragging = true;

    function onMove(ev: MouseEvent) {
      if (!containerEl) return;
      const rect = containerEl.getBoundingClientRect();
      const ratio = (ev.clientY - rect.top) / rect.height;
      const clamped = Math.max(0.15, Math.min(0.85, ratio));
      updateSetting("taskPanelSplit", 1 - clamped);
    }

    function onUp() {
      dragging = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }
</script>

<svelte:window onclick={closeContextMenu} />

<div class="flex h-full flex-col border-r border-zinc-800/50 bg-zinc-950/95" bind:this={containerEl}>
  <div class="flex items-center justify-between px-4 pt-4 pb-2.5">
    <div class="space-y-0.5">
      <span class="block text-xs font-semibold uppercase tracking-[0.22em] text-zinc-500">Sessions</span>
      <span class="block text-[11px] text-zinc-600">Command center activity</span>
    </div>
    <span class="rounded-md bg-zinc-900 px-1.5 py-0.5 font-mono text-[10px] text-zinc-500">
      {$sessionState.sessions.length}
    </span>
  </div>

  <div
    class="app-scrollbar overflow-y-auto px-2"
    style="flex: {!$settings.taskPanelCollapsed && $sessionState.activeSessionId ? 1 - $settings.taskPanelSplit : 1};"
  >
    {#each grouped as group (group.repoRoot)}
      {#if showGroupHeaders}
        <button
          class="group mt-1 flex w-full cursor-pointer items-center gap-1.5 bg-transparent px-1.5 py-2 text-left first:mt-0 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
          onclick={() => toggleGroup(group.repoRoot)}
          title={group.repoRoot}
        >
          <span class="text-[9px] text-zinc-600 transition-transform duration-150 {collapsedGroups.has(group.repoRoot) ? '' : 'rotate-90'}">&#9654;</span>
          <span class="truncate text-[11px] font-medium text-zinc-400">{group.name}</span>
        </button>
      {/if}
      {#if !collapsedGroups.has(group.repoRoot)}
        <div class={showGroupHeaders ? "pl-1" : ""}>
          {#each group.sessions as session (session.id)}
            <SessionCard
              {session}
              active={session.id === $sessionState.activeSessionId}
              onselect={() => setActiveSession(session.id)}
              onclose={() => handleClose(session.id)}
              onrename={(newName) => renameSession(session.id, newName)}
              onreconnect={() => handleReconnect(session.id)}
              onapprove={() => handleApprove(session.id)}
              onalways={() => handleAlways(session.id)}
              ondeny={() => handleDeny(session.id)}
              oncontextmenu={(e) => handleContextMenu(e, session)}
            />
          {/each}
        </div>
      {/if}
    {/each}
  </div>

  {#if $sessionState.activeSessionId}
    {#if $settings.taskPanelCollapsed}
      <button
        class="shrink-0 flex w-full items-center gap-1.5 border-t border-zinc-800/50 bg-transparent px-4 py-2 text-left cursor-pointer hover:bg-white/[0.03] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
        onclick={() => updateSetting("taskPanelCollapsed", false)}
      >
        <span class="text-[10px] text-zinc-600">&#9654;</span>
        <span class="text-[11px] font-semibold uppercase tracking-[0.18em] text-zinc-500">Tasks</span>
      </button>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="group flex h-3 shrink-0 cursor-row-resize items-center px-2" onmousedown={handleDividerDown}>
        <div
          class="h-px w-full rounded-full transition-all duration-150 {dragging ? 'bg-zinc-700/70 opacity-100' : 'bg-zinc-800/20 opacity-0 group-hover:opacity-100'}"
        ></div>
      </div>

      <div style="flex: {$settings.taskPanelSplit}; min-height: 0;">
        <TaskPanel onCollapse={() => updateSetting("taskPanelCollapsed", true)} />
      </div>
    {/if}
  {/if}

  <div class="flex shrink-0 gap-1 border-t border-zinc-800/50 p-2">
    <button
      class="flex flex-1 items-center justify-center gap-1.5 rounded-md bg-sky-500/12 py-2 text-xs text-sky-200 cursor-pointer transition-all duration-150 hover:bg-sky-500/22 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
      onclick={onNewSession}
    >
      <span class="text-sm">+</span> New
    </button>
    <button
      class="flex items-center justify-center rounded-md border border-zinc-800/70 bg-zinc-900 px-3 py-2 text-xs text-zinc-400 cursor-pointer transition-all duration-150 hover:bg-zinc-800 hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
      onclick={onOpenSettings}
    >
      &#9881;
    </button>
  </div>
</div>

{#if contextMenu}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed z-50 min-w-48 rounded-lg border border-zinc-800/70 bg-zinc-900 py-1 shadow-[0_12px_32px_rgba(0,0,0,0.5)]"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
    onclick={(e) => e.stopPropagation()}
  >
    {#if !worktreeInput}
      <button
        class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-zinc-300 hover:bg-white/[0.05] hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-900"
        onclick={showWorktreeInput}
      >
        <span class="text-[10px] opacity-70">&#9095;</span>
        New Worktree
      </button>
      <button
        class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-zinc-300 hover:bg-white/[0.05] hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-900"
        onclick={handleOpenInCode}
      >
        <span class="text-[10px] opacity-70">&#9998;</span>
        Open in Code
      </button>
    {:else}
      <div class="px-3 py-2">
        <div class="mb-1.5 text-[11px] text-zinc-500">Branch name</div>
        <form
          onsubmit={(e) => {
            e.preventDefault();
            handleCreateWorktree();
          }}
          class="flex gap-1.5"
        >
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="min-w-0 flex-1 rounded-md border border-zinc-800 bg-zinc-950 px-2 py-1.5 font-mono text-[12px] text-zinc-100 outline-none focus:border-sky-500/50"
            bind:value={branchName}
            placeholder="feature/my-branch"
            disabled={creatingWorktree}
            autofocus
          />
          <button
            class="cursor-pointer rounded-md bg-sky-500/12 px-2.5 py-1.5 text-[11px] font-medium text-sky-200 hover:bg-sky-500/22 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-900"
            type="submit"
            disabled={creatingWorktree || !branchName.trim()}
          >
            {creatingWorktree ? "..." : "Go"}
          </button>
        </form>
        {#if worktreeError}
          <div class="mt-1 truncate text-[10px] text-red" title={worktreeError}>{worktreeError}</div>
        {/if}
      </div>
    {/if}
  </div>
{/if}
