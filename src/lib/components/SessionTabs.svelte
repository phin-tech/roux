<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import { sessionState, setActiveSession, removeSession, renameSession } from "$lib/stores/sessions";
  import { removeSessionPanes } from "$lib/stores/panes";
  import { killSession, removeWorktree, writeToSession } from "$lib/tauri";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { closeAuxiliaryPanes } from "$lib/panes/actions";
  import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
  import { reconnectSession } from "$lib/sessions/reconnect";
  import { refreshTasks, initTaskOverrides } from "$lib/stores/tasks";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
  }

  let { onNewSession, onOpenSettings }: Props = $props();

  let dragging = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();

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

<div class="h-full flex flex-col bg-bg-base border-r border-border-subtle" bind:this={containerEl}>
  <div class="px-4 pt-3.5 pb-2.5 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Sessions</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$sessionState.sessions.length}
    </span>
  </div>

  <div class="overflow-y-auto px-2 scrollbar-thin" style="flex: {1 - $settings.taskPanelSplit};">
    {#each $sessionState.sessions as session (session.id)}
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
      />
    {/each}
  </div>

  {#if !$settings.taskPanelCollapsed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="h-px bg-border-subtle cursor-row-resize hover:bg-accent-dim transition-colors shrink-0 {dragging ? 'bg-accent' : ''}"
      onmousedown={handleDividerDown}
    ></div>

    <div style="flex: {$settings.taskPanelSplit}; min-height: 0;">
      <TaskPanel />
    </div>
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
