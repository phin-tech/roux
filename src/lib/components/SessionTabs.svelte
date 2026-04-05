<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import { sessionState, setActiveSession, removeSession, renameSession, addSession } from "$lib/stores/sessions";
  import { removeSessionPanes, initSessionPanes } from "$lib/stores/panes";
  import { killSession, removeWorktree, writeToSession, createSession, openInEditor } from "$lib/tauri";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { closeAuxiliaryPanes } from "$lib/panes/actions";
  import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
  import { reconnectSession } from "$lib/sessions/reconnect";
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

  // Group sessions by repoRoot, sorted by most recent createdAt in each group
  let grouped = $derived.by(() => {
    const map = new Map<string, { name: string; repoRoot: string; sessions: Session[]; latest: number }>();
    for (const s of $sessionState.sessions) {
      let group = map.get(s.repoRoot);
      if (!group) {
        group = { name: s.repoRoot.split("/").pop() || s.repoRoot, repoRoot: s.repoRoot, sessions: [], latest: 0 };
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

  // Context menu state
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

  // Refresh tasks when active session changes
  $effect(() => {
    const session = $sessionState.sessions.find((s) => s.id === $sessionState.activeSessionId);
    if (session) {
      void refreshTasks(session.worktreePath);
    }
  });

  // Load overrides on mount
  $effect(() => {
    void initTaskOverrides();
  });

  async function handleClose(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;

    if (
      $settings.confirmOnClose &&
      (session.status === "thinking" || session.status === "generating")
    ) {
      const confirmed = window.confirm(
        `"${session.name}" is currently ${session.status}. Close it?`
      );
      if (!confirmed) return;
    }

    await closeAuxiliaryPanes(id);
    await disposeClaudeTerminal(id);
    await killSession(id);

    if (session.isWorktree) {
      if ($settings.cleanupWorktreesOnClose) {
        await removeWorktree(session.worktreePath).catch(() => {});
      } else {
        const remove = window.confirm(
          `Also remove the worktree at ${session.worktreePath}?`
        );
        if (remove) {
          await removeWorktree(session.worktreePath).catch(() => {});
        }
      }
    }

    removeSessionPanes(id);
    removeSession(id);
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

<div class="h-full flex flex-col bg-bg-base border-r border-border-subtle" bind:this={containerEl}>
  <div class="px-4 pt-3.5 pb-2.5 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Sessions</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$sessionState.sessions.length}
    </span>
  </div>

  <div class="overflow-y-auto px-2 scrollbar-thin" style="flex: {!$settings.taskPanelCollapsed && $sessionState.activeSessionId ? 1 - $settings.taskPanelSplit : 1};">
    {#each grouped as group (group.repoRoot)}
      {#if showGroupHeaders}
        <button
          class="w-full flex items-center gap-1.5 px-1.5 py-1.5 bg-transparent border-none cursor-pointer text-left group mt-1 first:mt-0"
          onclick={() => toggleGroup(group.repoRoot)}
          title={group.repoRoot}
        >
          <span class="text-[9px] text-text-muted transition-transform duration-150 {collapsedGroups.has(group.repoRoot) ? '' : 'rotate-90'}">&#9654;</span>
          <span class="text-[11px] font-medium text-text-secondary truncate">{group.name}</span>
        </button>
      {/if}
      {#if !collapsedGroups.has(group.repoRoot)}
        <div class={showGroupHeaders ? 'pl-1' : ''}>
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
        class="shrink-0 px-4 py-1.5 border-t border-border-subtle flex items-center gap-1.5 bg-transparent border-x-0 border-b-0 cursor-pointer hover:bg-bg-hover w-full text-left"
        onclick={() => updateSetting("taskPanelCollapsed", false)}
      >
        <span class="text-[10px] text-text-muted">&#9654;</span>
        <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Tasks</span>
      </button>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="h-px bg-border-subtle cursor-row-resize hover:bg-accent-dim transition-colors shrink-0 {dragging ? 'bg-accent' : ''}"
        onmousedown={handleDividerDown}
      ></div>

      <div style="flex: {$settings.taskPanelSplit}; min-height: 0;">
        <TaskPanel onCollapse={() => updateSetting("taskPanelCollapsed", true)} />
      </div>
    {/if}
  {/if}

  <div class="p-2 border-t border-border-subtle flex gap-1 shrink-0">
    <button
      class="flex-1 py-2 bg-accent-dim border-none rounded-md text-accent text-xs font-sans cursor-pointer flex items-center justify-center gap-1.5 transition-all duration-150 hover:bg-accent hover:text-bg-deep"
      onclick={onNewSession}
    >
      <span class="text-sm">+</span> New
    </button>
    <button
      class="py-2 px-3 bg-bg-elevated border border-border-subtle rounded-md text-text-secondary text-xs cursor-pointer flex items-center justify-center transition-all duration-150 hover:bg-bg-hover hover:text-text-primary"
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
    class="fixed z-50 bg-bg-elevated border border-border rounded-lg shadow-[0_12px_32px_rgba(0,0,0,0.5)] py-1 min-w-48"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
    onclick={(e) => e.stopPropagation()}
  >
    {#if !worktreeInput}
      <button
        class="w-full text-left px-3 py-2 text-xs bg-transparent border-none cursor-pointer hover:bg-bg-hover text-text-secondary hover:text-text-primary flex items-center gap-2"
        onclick={showWorktreeInput}
      >
        <span class="text-[10px] opacity-70">&#9095;</span>
        New Worktree
      </button>
      <button
        class="w-full text-left px-3 py-2 text-xs bg-transparent border-none cursor-pointer hover:bg-bg-hover text-text-secondary hover:text-text-primary flex items-center gap-2"
        onclick={handleOpenInCode}
      >
        <span class="text-[10px] opacity-70">&#9998;</span>
        Open in Code
      </button>
    {:else}
      <div class="px-3 py-2">
        <div class="text-[11px] text-text-muted mb-1.5">Branch name</div>
        <form
          onsubmit={(e) => { e.preventDefault(); handleCreateWorktree(); }}
          class="flex gap-1.5"
        >
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="flex-1 bg-bg-deep border border-border rounded-md px-2 py-1.5 font-mono text-[12px] text-text-primary outline-none focus:border-accent-dim min-w-0"
            bind:value={branchName}
            placeholder="feature/my-branch"
            disabled={creatingWorktree}
            autofocus
          />
          <button
            class="px-2.5 py-1.5 bg-accent-dim border-none rounded-md text-accent text-[11px] font-medium cursor-pointer hover:bg-accent hover:text-bg-deep disabled:opacity-50"
            type="submit"
            disabled={creatingWorktree || !branchName.trim()}
          >
            {creatingWorktree ? "..." : "Go"}
          </button>
        </form>
        {#if worktreeError}
          <div class="text-[10px] text-red mt-1 truncate" title={worktreeError}>{worktreeError}</div>
        {/if}
      </div>
    {/if}
  </div>
{/if}
