  <script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import { sessionState, setActiveSession, removeSession, renameSession } from "$lib/stores/sessions";
  import { removeSessionPanes } from "$lib/stores/panes";
  import { killSession, removeWorktree, writeToSession } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import { closeAuxiliaryPanes } from "$lib/panes/actions";
  import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
  import { reconnectSession } from "$lib/sessions/reconnect";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
  }

  let { onNewSession, onOpenSettings }: Props = $props();

  async function handleClose(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;

    // Confirm if session is active (thinking/generating)
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

    // Worktree cleanup
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
    // Permission dialog cursor is on "Yes" by default — just press Enter
    await writeToSession(id, "\r");
  }

  async function handleAlways(id: string) {
    // Shift+Tab selects "allow during this session"
    await writeToSession(id, "\x1b[Z");
  }

  async function handleDeny(id: string) {
    // Move down twice to "No", then Enter
    // Down arrow = \x1b[B
    await writeToSession(id, "\x1b[B\x1b[B\r");
  }

  async function handleReconnect(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;
    await reconnectSession(session);
  }
</script>

<div class="h-full flex flex-col bg-bg-base border-r border-border-subtle">
  <div class="px-4 pt-3.5 pb-2.5 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Sessions</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$sessionState.sessions.length}
    </span>
  </div>

  <div class="flex-1 overflow-y-auto px-2 scrollbar-thin">
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

  <div class="p-2 border-t border-border-subtle flex gap-1">
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
