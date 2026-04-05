<script lang="ts">
  import { taskGroups, getTaskRun, setKeepOpenOverride, getEffectiveKeepOpen } from "$lib/stores/tasks";
  import { sessionState } from "$lib/stores/sessions";
  import { runTask } from "$lib/tasks/runner";
  import type { TaskDefinition } from "$lib/types/tasks";

  let collapsedGroups = $state(new Set<string>());
  let contextMenu = $state<{ x: number; y: number; task: TaskDefinition; repoRoot: string } | null>(null);

  const activeSession = $derived(
    $sessionState.sessions.find((s) => s.id === $sessionState.activeSessionId)
  );

  function toggleGroup(runner: string) {
    collapsedGroups = new Set(collapsedGroups);
    if (collapsedGroups.has(runner)) {
      collapsedGroups.delete(runner);
    } else {
      collapsedGroups.add(runner);
    }
  }

  function handleRun(task: TaskDefinition) {
    if (!activeSession || !$sessionState.activeSessionId) return;
    void runTask($sessionState.activeSessionId, activeSession.worktreePath, task);
  }

  function handleContextMenu(e: MouseEvent, task: TaskDefinition) {
    e.preventDefault();
    if (!activeSession) return;
    contextMenu = { x: e.clientX, y: e.clientY, task, repoRoot: activeSession.repoRoot };
  }

  function setKeepOpen(value: "always" | "on-error" | "never") {
    if (!contextMenu) return;
    setKeepOpenOverride(contextMenu.repoRoot, contextMenu.task.id, value);
    contextMenu = null;
  }

  function handleClickOutside() {
    contextMenu = null;
  }

  function getRunStatus(taskId: string): "running" | "succeeded" | "failed" | null {
    if (!$sessionState.activeSessionId) return null;
    const run = getTaskRun($sessionState.activeSessionId, taskId);
    return run?.status ?? null;
  }
</script>

<svelte:window onclick={handleClickOutside} />

<div class="flex flex-col h-full">
  <div class="px-4 pt-2.5 pb-2 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Tasks</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$taskGroups.reduce((n, g) => n + g.tasks.length, 0)}
    </span>
  </div>

  <div class="flex-1 overflow-y-auto px-2 scrollbar-thin">
    {#if $taskGroups.length === 0}
      <p class="text-xs text-text-muted text-center py-4">No tasks found</p>
    {:else}
      {#each $taskGroups as group (group.runner)}
        <button
          class="w-full flex items-center gap-1.5 px-2 py-1.5 text-[11px] font-semibold text-text-secondary uppercase tracking-wide cursor-pointer bg-transparent border-none hover:text-text-primary"
          onclick={() => toggleGroup(group.runner)}
        >
          <span class="text-[10px] transition-transform {collapsedGroups.has(group.runner) ? '' : 'rotate-90'}">&#9654;</span>
          {group.runner}
          <span class="font-mono text-[10px] text-text-muted font-normal normal-case tracking-normal ml-auto">{group.tasks.length}</span>
        </button>

        {#if !collapsedGroups.has(group.runner)}
          {#each group.tasks as task (task.id)}
            {@const status = getRunStatus(task.id)}
            <button
              class="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-text-secondary bg-transparent border-none cursor-pointer rounded hover:bg-bg-hover hover:text-text-primary group"
              onclick={() => handleRun(task)}
              oncontextmenu={(e) => handleContextMenu(e, task)}
              title={task.description || task.command}
            >
              <span class="flex-1 text-left truncate font-mono text-[12px]">{task.name}</span>
              {#if status === "running"}
                <span class="w-2 h-2 rounded-full bg-blue-400 animate-pulse shrink-0"></span>
              {:else if status === "succeeded"}
                <span class="w-2 h-2 rounded-full bg-green-400 shrink-0"></span>
              {:else if status === "failed"}
                <span class="w-2 h-2 rounded-full bg-red-400 shrink-0"></span>
              {:else}
                <span class="text-text-muted opacity-0 group-hover:opacity-100 text-[10px] shrink-0">&#9654;</span>
              {/if}
            </button>
          {/each}
        {/if}
      {/each}
    {/if}
  </div>
</div>

{#if contextMenu}
  <div
    class="fixed z-50 bg-bg-elevated border border-border rounded-md shadow-lg py-1 min-w-40"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
  >
    <div class="px-3 py-1.5 text-[11px] text-text-muted uppercase tracking-wide">Keep open</div>
    {#each [["always", "Always"], ["on-error", "On Error"], ["never", "Never"]] as [value, label]}
      {@const current = getEffectiveKeepOpen(contextMenu.repoRoot, contextMenu.task.id, contextMenu.task.keepOpen)}
      <button
        class="w-full text-left px-3 py-1.5 text-xs bg-transparent border-none cursor-pointer hover:bg-bg-hover text-text-secondary hover:text-text-primary flex items-center gap-2"
        onclick={() => setKeepOpen(value as "always" | "on-error" | "never")}
      >
        <span class="w-3 text-accent text-[10px]">{current === value ? "✓" : ""}</span>
        {label}
      </button>
    {/each}
  </div>
{/if}
