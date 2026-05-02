<script lang="ts">
  import {
    taskGroups,
    taskRuns,
    setKeepOpenOverride,
    getEffectiveKeepOpen,
  } from "$lib/stores/tasks";
  import { activeSession, activeSessionId } from "$lib/stores/sessions";
  import { runTask, expandTask } from "$lib/tasks/runner";
  import type { TaskDefinition } from "$lib/types";
  import { createWatch } from "$lib/tauri";
  import type { CreateWatchConfig } from "$lib/types";

  import PinButton from "./PinButton.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";

  interface Props {
    visible?: boolean;
    onCollapse?: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible = true, onCollapse, pinned = false, onTogglePin }: Props = $props();

  let collapsedGroups = $state(new Set<string>());
  let contextMenu = $state<{ x: number; y: number; task: TaskDefinition; repoRoot: string } | null>(null);
  let filter = $state("");

  const filteredGroups = $derived.by(() => {
    if (!visible) return [];
    const q = filter.trim().toLowerCase();
    if (!q) return $taskGroups;
    return $taskGroups
      .map((g) => ({
        ...g,
        tasks: g.tasks.filter(
          (t) =>
            t.name.toLowerCase().includes(q) ||
            t.command.toLowerCase().includes(q) ||
            (t.description ?? "").toLowerCase().includes(q)
        ),
      }))
      .filter((g) => g.tasks.length > 0);
  });

  const currentSession = $derived(visible ? $activeSession : null);
  const currentSessionId = $derived(visible ? $activeSessionId : null);

  function toggleGroup(runner: string) {
    collapsedGroups = new Set(collapsedGroups);
    if (collapsedGroups.has(runner)) {
      collapsedGroups.delete(runner);
    } else {
      collapsedGroups.add(runner);
    }
  }

  function handleRun(task: TaskDefinition) {
    if (!currentSession || !currentSessionId) return;
    void runTask(currentSessionId, currentSession.worktreePath, task);
  }

  function handleExpand(e: MouseEvent, ptyId: string) {
    e.stopPropagation();
    e.preventDefault();
    if (!currentSessionId) return;
    expandTask(currentSessionId, ptyId);
  }

  function handleContextMenu(e: MouseEvent, task: TaskDefinition) {
    e.preventDefault();
    if (!currentSession) return;
    contextMenu = { x: e.clientX, y: e.clientY, task, repoRoot: currentSession.repoRoot };
  }

  async function handleWatchTask(task: TaskDefinition, repoRoot: string, sessionId: string) {
    const config: CreateWatchConfig = {
      name: `Task: ${task.name}`,
      kind: {
        type: "task",
        taskId: task.id,
        command: task.command,
        workingDir: repoRoot,
      },
      mode: { type: "recurring", intervalSecs: 30 },
      scope: { type: "session", sessionId },
      notify: null,
    };
    await createWatch(config);
    contextMenu = null;
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
    const sessionId = currentSessionId;
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
  <SidebarPanelHeader title="Tasks">
    {#snippet actions()}
      <span class="border border-border-subtle bg-bg-surface px-2 py-1 text-[12px] text-text-secondary">
        {filteredGroups.reduce((n, g) => n + g.tasks.length, 0)}
      </span>
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      {#if onCollapse}
        <CollapseSidebarButton
          onclick={onCollapse}
          label="Collapse tasks sidebar"
          title="Collapse tasks sidebar"
        />
      {/if}
    {/snippet}
  </SidebarPanelHeader>

  {#if $taskGroups.length > 0}
    <div class="px-3 pb-2">
      <input
        class="w-full border border-border-subtle bg-bg-surface/80 px-3 py-2 text-[13px] text-text-primary placeholder:text-text-muted outline-none transition-colors focus:border-border focus:bg-bg-surface"
        placeholder="Filter tasks..."
        bind:value={filter}
      />
    </div>
  {/if}

  <div class="app-scrollbar flex-1 overflow-y-auto px-2">
    {#if $taskGroups.length === 0}
      <p class="py-4 text-center text-xs text-text-muted">No tasks found</p>
    {:else if filteredGroups.length === 0}
      <p class="py-4 text-center text-xs text-text-muted">No matching tasks</p>
    {:else}
      {#each filteredGroups as group (group.runner)}
        <button
          class="flex w-full items-center gap-1.5 bg-transparent px-2 py-1.5 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary cursor-pointer hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
          onclick={() => toggleGroup(group.runner)}
        >
          <span class="text-[11px] text-text-secondary transition-transform {collapsedGroups.has(group.runner) ? '' : 'rotate-90'}">&#9654;</span>
          {group.runner}
          <span class="ml-auto border border-border-subtle bg-bg-surface px-2 py-1 text-[11px] font-medium normal-case tracking-normal text-text-secondary">{group.tasks.length}</span>
        </button>

        {#if !collapsedGroups.has(group.runner)}
          {#each group.tasks as task (task.id)}
            {@const run = activeRuns.get(task.id) ?? null}
            <div class="mb-1">
              <button
                class="group relative flex w-full items-start gap-3 px-3 py-2 text-left cursor-pointer transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep
                  {run?.status === 'running'
                    ? 'bg-accent-dim/8 hover:bg-accent-dim/12'
                    : run?.status === 'failed'
                      ? 'bg-red/6 hover:bg-red/10'
                      : run?.status === 'succeeded'
                        ? 'bg-green/6 hover:bg-green/10'
                        : 'bg-transparent hover:bg-bg-active/40'}"
                onclick={() => handleRun(task)}
                oncontextmenu={(e) => handleContextMenu(e, task)}
                title={task.description || task.command}
              >
                {#if run?.status === "running"}
                  <div class="absolute left-0 top-2 bottom-2 w-[2px] rounded-full bg-accent shadow-[0_0_10px_var(--color-blue-dim)]"></div>
                {/if}
                <div class="min-w-0 flex-1">
                  <div class="truncate text-[13px] font-semibold text-text-secondary group-hover:text-text-primary transition-colors">{task.name}</div>
                  <div class="mt-0.5 truncate font-mono text-[11px] text-text-muted">{taskSubtitle(task)}</div>
                </div>
                {#if run?.status === "running"}
                  <div class="flex shrink-0 items-center gap-2 pt-0.5">
                    <span class="text-[11px] text-text-secondary">{elapsed(run.startedAt)}</span>
                    <span class="inline-flex items-center gap-1 border border-accent-dim/20 bg-accent-dim/15 px-2 py-0.5 text-[9px] font-medium uppercase tracking-[0.18em] text-accent">
                      <span class="h-1.5 w-1.5 rounded-full bg-accent animate-pulse"></span>
                      live
                    </span>
                  </div>
                {:else if run?.status === "succeeded"}
                  <span class="mt-0.5 inline-flex shrink-0 items-center gap-1 border border-green/20 bg-green/12 px-2 py-0.5 text-[9px] font-medium uppercase tracking-[0.18em] text-green">
                    <span class="h-1.5 w-1.5 rounded-full bg-green"></span>
                    done
                  </span>
                {:else if run?.status === "failed"}
                  <span class="mt-0.5 inline-flex shrink-0 items-center gap-1 border border-red/20 bg-red/12 px-2 py-0.5 text-[9px] font-medium uppercase tracking-[0.18em] text-red">
                    <span class="h-1.5 w-1.5 rounded-full bg-red"></span>
                    error
                  </span>
                {:else}
                  <span class="shrink-0 pt-1 text-[11px] text-text-secondary opacity-80 transition-opacity group-hover:opacity-100">&#9654;</span>
                {/if}
              </button>

              {#if run && run.outputLines.length > 0}
                <div class="ui-panel mx-3 mb-2 overflow-hidden">
                  <div class="flex items-center justify-between border-b border-border-subtle px-2.5 py-1.5">
                    <span class="text-[10px] text-text-secondary">
                      {run.status === "running"
                        ? "running..."
                        : run.status === "succeeded"
                          ? "exit 0"
                          : `exit ${run.exitCode ?? "?"}`}
                    </span>
                    {#if !run.paneId}
                      <button
                        class="cursor-pointer bg-transparent px-1 text-[11px] font-medium text-text-secondary hover:text-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
                        onclick={(e) => handleExpand(e, run.ptyId)}
                        title="Expand to terminal pane"
                      >&#8599;</button>
                    {/if}
                  </div>
                  <pre class="m-0 max-h-24 overflow-y-auto whitespace-pre-wrap break-all px-2.5 py-2 text-[11px] leading-tight text-text-primary">{run.outputLines.slice(-15).join("\n")}</pre>
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
    class="ui-dialog fixed z-50 min-w-40 py-1"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
  >
    <button
      class="flex w-full items-center gap-2 bg-transparent px-3 py-1.5 text-left text-sm text-text-primary hover:bg-bg-hover"
      onclick={() => {
        if (!contextMenu || !currentSession) return;
        handleWatchTask(contextMenu.task, contextMenu.repoRoot, currentSession.id);
      }}
    >
      Watch
    </button>
    <div class="mx-2 border-t border-hairline"></div>
    <div class="px-3 py-1.5 text-[11px] uppercase tracking-wide text-text-muted">Keep open</div>
    {#each [["always", "Always"], ["on-error", "On Error"], ["never", "Never"]] as [value, label]}
      {@const current = getEffectiveKeepOpen(contextMenu.repoRoot, contextMenu.task.id, contextMenu.task.keepOpen)}
      <button
        class="flex w-full items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs text-text-secondary cursor-pointer hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
        onclick={() => setKeepOpen(value as "always" | "on-error" | "never")}
      >
        <span class="w-3 text-[10px] text-accent">{current === value ? "✓" : ""}</span>
        {label}
      </button>
    {/each}
  </div>
{/if}
