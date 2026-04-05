<script lang="ts">
  import {
    taskGroups,
    taskRuns,
    setKeepOpenOverride,
    getEffectiveKeepOpen,
  } from "$lib/stores/tasks";
  import { sessionState } from "$lib/stores/sessions";
  import { runTask, expandTask } from "$lib/tasks/runner";
  import type { TaskDefinition } from "$lib/types/tasks";

  interface Props {
    onCollapse?: () => void;
  }

  let { onCollapse }: Props = $props();

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

  function handleExpand(e: MouseEvent, ptyId: string) {
    e.stopPropagation();
    e.preventDefault();
    if (!$sessionState.activeSessionId) return;
    expandTask($sessionState.activeSessionId, ptyId);
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

  const activeRuns = $derived.by(() => {
    const sessionId = $sessionState.activeSessionId;
    if (!sessionId) return new Map();
    const runs = $taskRuns.get(sessionId) ?? [];
    const map = new Map();
    for (const run of runs) {
      map.set(run.taskId, run);
    }
    return map;
  });

  function elapsed(startedAt: number): string {
    const s = Math.floor((Date.now() - startedAt) / 1000);
    if (s < 60) return `${s}s`;
    return `${Math.floor(s / 60)}m${s % 60}s`;
  }

  function taskSubtitle(task: TaskDefinition): string {
    return task.description || task.command;
  }
</script>

<svelte:window onclick={handleClickOutside} />

<div class="flex h-full flex-col bg-transparent">
  <div class="flex items-start justify-between px-4 pt-3 pb-2">
    <div class="space-y-0.5">
      <span class="block text-xs font-semibold uppercase tracking-[0.22em] text-zinc-500">Tasks</span>
      <span class="block text-[11px] text-zinc-600">Runnable workspace actions</span>
    </div>
    <div class="flex items-center gap-1.5">
      <span class="rounded-md bg-zinc-900 px-1.5 py-0.5 font-mono text-[10px] text-zinc-500">
        {$taskGroups.reduce((n, g) => n + g.tasks.length, 0)}
      </span>
      {#if onCollapse}
        <button
          class="cursor-pointer bg-transparent p-0.5 text-[10px] text-zinc-600 hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
          onclick={onCollapse}
          title="Collapse tasks panel"
        >&#9660;</button>
      {/if}
    </div>
  </div>

  <div class="app-scrollbar flex-1 overflow-y-auto px-2">
    {#if $taskGroups.length === 0}
      <p class="py-4 text-center text-xs text-zinc-600">No tasks found</p>
    {:else}
      {#each $taskGroups as group (group.runner)}
        <button
          class="flex w-full items-center gap-1.5 bg-transparent px-2 py-2 text-[10px] font-semibold uppercase tracking-[0.2em] text-zinc-500 cursor-pointer hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
          onclick={() => toggleGroup(group.runner)}
        >
          <span class="text-[10px] transition-transform {collapsedGroups.has(group.runner) ? '' : 'rotate-90'}">&#9654;</span>
          {group.runner}
          <span class="ml-auto font-mono text-[10px] font-normal normal-case tracking-normal text-zinc-600">{group.tasks.length}</span>
        </button>

        {#if !collapsedGroups.has(group.runner)}
          {#each group.tasks as task (task.id)}
            {@const run = activeRuns.get(task.id) ?? null}
            <div>
              <button
                class="group flex w-full items-start gap-3 rounded-xl bg-transparent px-3 py-2 text-left cursor-pointer transition-colors hover:bg-white/[0.05] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
                onclick={() => handleRun(task)}
                oncontextmenu={(e) => handleContextMenu(e, task)}
                title={task.description || task.command}
              >
                <div class="min-w-0 flex-1">
                  <div class="truncate text-[12px] font-medium text-zinc-100">{task.name}</div>
                  <div class="mt-0.5 truncate font-mono text-[10px] text-zinc-600">{taskSubtitle(task)}</div>
                </div>
                {#if run?.status === "running"}
                  <div class="flex shrink-0 items-center gap-2 pt-0.5">
                    <span class="font-mono text-[10px] text-zinc-500">{elapsed(run.startedAt)}</span>
                    <span class="inline-flex items-center gap-1 rounded-full bg-sky-500/10 px-2 py-0.5 text-[9px] font-medium uppercase tracking-[0.18em] text-sky-200">
                      <span class="h-1.5 w-1.5 rounded-full bg-sky-400 animate-pulse"></span>
                      live
                    </span>
                  </div>
                {:else if run?.status === "succeeded"}
                  <span class="mt-0.5 inline-flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/10 px-2 py-0.5 text-[9px] font-medium uppercase tracking-[0.18em] text-emerald-200">
                    <span class="h-1.5 w-1.5 rounded-full bg-emerald-400"></span>
                    done
                  </span>
                {:else if run?.status === "failed"}
                  <span class="mt-0.5 inline-flex shrink-0 items-center gap-1 rounded-full bg-rose-500/10 px-2 py-0.5 text-[9px] font-medium uppercase tracking-[0.18em] text-rose-200">
                    <span class="h-1.5 w-1.5 rounded-full bg-rose-400"></span>
                    error
                  </span>
                {:else}
                  <span class="shrink-0 pt-1 text-[10px] text-zinc-600 opacity-0 transition-opacity group-hover:opacity-100">&#9654;</span>
                {/if}
              </button>

              {#if run && run.outputLines.length > 0}
                <div class="mx-3 mb-2 overflow-hidden rounded-xl border border-zinc-800/50 bg-zinc-950/90">
                  <div class="flex items-center justify-between border-b border-zinc-800/50 px-2.5 py-1.5">
                    <span class="font-mono text-[10px] text-zinc-500">
                      {run.status === "running"
                        ? "running..."
                        : run.status === "succeeded"
                          ? "exit 0"
                          : `exit ${run.exitCode ?? "?"}`}
                    </span>
                    {#if !run.paneId}
                      <button
                        class="cursor-pointer bg-transparent px-1 text-[10px] text-zinc-600 hover:text-sky-200 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
                        onclick={(e) => handleExpand(e, run.ptyId)}
                        title="Expand to terminal pane"
                      >&#8599;</button>
                    {/if}
                  </div>
                  <pre class="m-0 max-h-24 overflow-y-auto whitespace-pre-wrap break-all px-2.5 py-2 text-[11px] leading-tight text-zinc-300">{run.outputLines.slice(-15).join("\n")}</pre>
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      {/each}
    {/if}
  </div>
</div>

{#if contextMenu}
  <div
    class="fixed z-50 min-w-40 rounded-md border border-zinc-800/70 bg-zinc-900 py-1 shadow-lg"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
  >
    <div class="px-3 py-1.5 text-[11px] uppercase tracking-wide text-zinc-500">Keep open</div>
    {#each [["always", "Always"], ["on-error", "On Error"], ["never", "Never"]] as [value, label]}
      {@const current = getEffectiveKeepOpen(contextMenu.repoRoot, contextMenu.task.id, contextMenu.task.keepOpen)}
      <button
        class="flex w-full items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs text-zinc-300 cursor-pointer hover:bg-white/[0.05] hover:text-zinc-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
        onclick={() => setKeepOpen(value as "always" | "on-error" | "never")}
      >
        <span class="w-3 text-[10px] text-sky-300">{current === value ? "✓" : ""}</span>
        {label}
      </button>
    {/each}
  </div>
{/if}
