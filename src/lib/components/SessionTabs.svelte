<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import {
    sessionState,
    setActiveSession,
    renameSession,
    addSession,
    updateSessionGitStatus,
  } from "$lib/stores/sessions";
  import { initSessionWithProfile } from "$lib/panes/actions";
  import {
    createSessionShell,
    openInEditor,
    refreshSessionGitStatus,
  } from "$lib/tauri";
  import type { SpawnProfileRef } from "$lib/panes/profiles";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { reconnectSession } from "$lib/sessions/reconnect";
  import { closeSession } from "$lib/sessions/close";
  import { refreshTasks, initTaskOverrides } from "$lib/stores/tasks";
  import { projects, createProject } from "$lib/stores/projects";
  import { setSessionProject } from "$lib/stores/sessions";
  import { setSessionProject as tauriSetSessionProject } from "$lib/tauri";
  import { log, logError } from "$lib/logging";
  import { failureCount } from "$lib/stores/watches";
  import { unreadTotal } from "$lib/stores/notifications";
  import type { Session } from "$lib/types";
  import { getGroupedSessions } from "$lib/sessions/order";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
    onToggleWatches: () => void;
    onToggleNotifications: () => void;
  }

  let { onNewSession, onOpenSettings, onToggleWatches, onToggleNotifications }: Props = $props();

  let dragging = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();
  let collapsedGroups = $state(new Set<string>());

  let grouped = $derived(
    getGroupedSessions(
      $sessionState.sessions,
      $projects,
      $settings.groupBy ?? "repo",
    ),
  );
  let showGroupHeaders = $derived(grouped.length > 0);

  // Map of session id -> slot number (1..10) in sidebar visual order.
  // Collapsed groups are intentionally counted so Cmd+N shortcuts do not
  // renumber when groups are collapsed.
  let slotById = $derived.by(() => {
    const map = new Map<string, number>();
    let slot = 1;
    for (const group of grouped) {
      for (const session of group.sessions) {
        if (slot > 10) return map;
        map.set(session.id, slot);
        slot += 1;
      }
    }
    return map;
  });

  function toggleGroup(key: string) {
    const next = new Set(collapsedGroups);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsedGroups = next;
  }

  function toggleGroupBy() {
    updateSetting("groupBy", $settings.groupBy === "project" ? "repo" : "project");
  }

  let contextMenu = $state<{ x: number; y: number; session: Session } | null>(null);
  let worktreeInput = $state(false);
  let branchName = $state("");
  let creatingWorktree = $state(false);
  let worktreeError = $state("");
  let projectMenu = $state(false);
  let newProjectInput = $state(false);
  let newProjectName = $state("");
  let lastTaskWorktreePath = $state<string | null>(null);

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
    projectMenu = false;
    newProjectInput = false;
    newProjectName = "";
  }

  function showWorktreeInput() {
    worktreeInput = true;
  }

  function showProjectMenu() {
    projectMenu = true;
  }

  async function assignProject(projectId: string | null) {
    if (!contextMenu) return;
    setSessionProject(contextMenu.session.id, projectId);
    await tauriSetSessionProject(contextMenu.session.id, projectId);
    closeContextMenu();
  }

  async function handleCreateAndAssignProject() {
    if (!contextMenu || !newProjectName.trim()) return;
    const project = await createProject(newProjectName.trim());
    setSessionProject(contextMenu.session.id, project.id);
    await tauriSetSessionProject(contextMenu.session.id, project.id);
    closeContextMenu();
  }

  async function handleCreateWorktree() {
    if (!contextMenu || !branchName.trim()) return;
    creatingWorktree = true;
    worktreeError = "";
    try {
      const repo = contextMenu.session.repoRoot;
      const branch = branchName.trim();
      const name = repo.split("/").pop() + "-" + branch;
      log(`Creating worktree session: repo=${repo}, branch=${branch}`);

      // Resolve Claude profile's nono config up-front so the primary shell
      // is sandboxed from the start (matches the layout/dialog paths).
      const profileRef: SpawnProfileRef = { kind: "registered", id: "claude" };
      const { resolveProfileRef } = await import("$lib/panes/profiles");
      const { runProfileInPane } = await import("$lib/panes/profileRunner");
      const profile = resolveProfileRef(profileRef);
      const nonoProfile = profile?.nonoProfile ?? undefined;
      const nonoAllowDirs = profile?.nonoAllowDirs ?? undefined;

      const session = await createSessionShell(
        repo, name, null, branch,
        nonoProfile, nonoAllowDirs,
      );
      log(`Worktree session created: ${session.id}`);
      addSession(session);
      const mainPaneId = initSessionWithProfile(session.id, profileRef, {
        nonoProfile,
        nonoAllowDirs,
      });
      const { connectPaneTerminal } = await import("$lib/panes/terminals");
      await connectPaneTerminal(mainPaneId);
      if (profile) await runProfileInPane(session.id, profile);
      closeContextMenu();
    } catch (e) {
      logError("Failed to create worktree session", e);
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
    const worktreePath = session?.worktreePath ?? null;
    if (!worktreePath) {
      lastTaskWorktreePath = null;
      return;
    }
    if (worktreePath !== lastTaskWorktreePath) {
      lastTaskWorktreePath = worktreePath;
      void refreshTasks(worktreePath);
    }
  });

  $effect(() => {
    void initTaskOverrides();
  });

  // Poll non-git sessions to detect when they become git repos (e.g. after `git init`)
  $effect(() => {
    const interval = setInterval(() => {
      for (const s of $sessionState.sessions) {
        if (!s.isGitRepo) {
          refreshSessionGitStatus(s.id).then((isGit) => {
            if (isGit) updateSessionGitStatus(s.id, true);
          });
        }
      }
    }, 5000);
    return () => clearInterval(interval);
  });

  async function handleClose(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;
    await closeSession(session);
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

<div
  class="flex h-full flex-col overflow-hidden bg-bg-base/96 shadow-[0_0_0_1px_rgba(255,255,255,0.03)]"
  bind:this={containerEl}
>
  <div class="flex h-9 shrink-0 items-center justify-between px-3">
    <div class="flex items-center gap-2">
      <button
        class="flex h-5 w-5 items-center justify-center text-text-secondary cursor-pointer transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={() => updateSetting("sidebarCollapsed", true)}
        title="Collapse sidebar"
        aria-label="Collapse sidebar"
      >
        <span class="text-[11px]">{$settings.tabPosition === "right" ? "\u25B6" : "\u25C0"}</span>
      </button>
      <span class="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">Sessions</span>
      <button
        class="relative flex items-center justify-center text-text-secondary cursor-pointer transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={onToggleNotifications}
        title="Toggle notifications (⌘I)"
        aria-label="Toggle notifications"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
          <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
        </svg>
        {#if $unreadTotal > 0}
          <span class="absolute -right-1.5 -top-1 inline-flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-accent px-1 text-[9px] font-bold text-white">
            {$unreadTotal}
          </span>
        {/if}
      </button>
    </div>
    <div class="flex items-center gap-1.5">
      <button
        class="border border-border-subtle bg-bg-surface px-2 py-1 text-[13px] font-medium text-text-secondary cursor-pointer transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={toggleGroupBy}
        title="Group by {$settings.groupBy === 'project' ? 'repo' : 'project'}"
      >
        {$settings.groupBy === "project" ? "project" : "repo"}
      </button>
      <span class="border border-border-subtle bg-bg-surface px-2 py-1 font-mono text-[12px] text-text-secondary">
        {$sessionState.sessions.length}
      </span>
    </div>
  </div>

  <div class="flex shrink-0 gap-1 border-b border-hairline p-2">
    <button
      class="flex flex-1 items-center justify-center gap-1.5 bg-bg-active/50 py-2 text-[13px] font-medium text-text-secondary cursor-pointer transition-all duration-150 hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
      onclick={onNewSession}
    >
      <span class="text-sm">+</span> New
    </button>
    <button
      class="flex items-center justify-center gap-1 bg-bg-active/50 px-3 py-2 text-[13px] font-medium text-text-secondary cursor-pointer transition-all duration-150 hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
      onclick={onToggleWatches}
      title="Toggle watches"
    >
      Watches
      {#if $failureCount > 0}
        <span class="ml-1 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-red px-1 text-[10px] font-bold text-white">
          {$failureCount}
        </span>
      {/if}
    </button>
    <button
      class="flex items-center justify-center bg-bg-active/50 px-3 py-2 text-[13px] font-medium text-text-secondary cursor-pointer transition-all duration-150 hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
      onclick={onOpenSettings}
    >
      &#9881;
    </button>
  </div>

  <div
    class="app-scrollbar overflow-y-auto px-2"
    style="flex: {!$settings.taskPanelCollapsed && $sessionState.activeSessionId ? 1 - $settings.taskPanelSplit : 1};"
  >
    {#each grouped as group (group.key)}
      {#if showGroupHeaders}
        <button
          class="group mt-1 flex w-full cursor-pointer items-center gap-1.5 bg-transparent px-1.5 py-2 text-left first:mt-0 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
          onclick={() => toggleGroup(group.key)}
          title={group.key}
        >
          <span class="text-[10px] text-text-secondary transition-transform duration-150 {collapsedGroups.has(group.key) ? '' : 'rotate-90'}">&#9654;</span>
          <span class="truncate text-[11px] font-semibold text-text-secondary">{group.name}</span>
        </button>
      {/if}
      {#if !collapsedGroups.has(group.key)}
        <div class={showGroupHeaders ? "pl-1" : ""}>
          {#each group.sessions as session (session.id)}
            <SessionCard
              {session}
              active={session.id === $sessionState.activeSessionId}
              slotNumber={slotById.get(session.id)}
              hideProjectTag={($settings.groupBy ?? "repo") === "project"}
              onselect={() => setActiveSession(session.id)}
              onclose={() => handleClose(session.id)}
              onrename={(newName) => renameSession(session.id, newName)}
              onreconnect={() => handleReconnect(session.id)}
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
        class="shrink-0 flex w-full items-center gap-1.5 border-t border-hairline bg-bg-deep/35 px-3 py-2 text-left cursor-pointer hover:bg-bg-active/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
        onclick={() => updateSetting("taskPanelCollapsed", false)}
      >
        <span class="text-[11px] text-text-secondary">&#9654;</span>
        <span class="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">Tasks</span>
      </button>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="group flex h-3 shrink-0 cursor-row-resize items-center px-2" onmousedown={handleDividerDown}>
        <div
          class="h-px w-full transition-all duration-150 {dragging ? 'bg-white/22' : 'bg-white/10 group-hover:bg-white/16'}"
        ></div>
      </div>

      <div class="min-h-0 bg-bg-deep/35" style="flex: {$settings.taskPanelSplit};">
        <TaskPanel onCollapse={() => updateSetting("taskPanelCollapsed", true)} />
      </div>
    {/if}
  {/if}

</div>

{#if contextMenu}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="ui-dialog fixed z-50 min-w-48 py-1"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
    onclick={(e) => e.stopPropagation()}
  >
    {#if projectMenu}
      {#if !newProjectInput}
        <div class="px-1 py-1">
          <div class="px-2 py-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">Set Project</div>
          {#if contextMenu.session.projectId}
            <button
              class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary"
              onclick={() => assignProject(null)}
            >
              <span class="text-[11px] text-text-secondary">&times;</span>
              Remove Project
            </button>
          {/if}
          {#each $projects as project (project.id)}
            <button
              class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs hover:bg-bg-hover
                {contextMenu.session.projectId === project.id ? 'text-accent' : 'text-text-secondary hover:text-text-primary'}"
              onclick={() => assignProject(project.id)}
            >
              {project.name}
            </button>
          {/each}
          <button
            class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs text-accent hover:bg-bg-hover"
            onclick={() => { newProjectInput = true; }}
          >
            <span class="text-[10px]">+</span>
            New Project...
          </button>
        </div>
      {:else}
        <div class="px-3 py-2">
          <div class="mb-1.5 text-[11px] text-text-muted">Project name</div>
          <form
            onsubmit={(e) => { e.preventDefault(); handleCreateAndAssignProject(); }}
            class="flex gap-1.5"
          >
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="min-w-0 flex-1 border border-border-subtle bg-bg-deep px-2 py-1.5 font-mono text-[12px] text-text-primary outline-none focus:border-accent-dim/50"
              bind:value={newProjectName}
              placeholder="my-project"
              autofocus
            />
            <button
              class="cursor-pointer border border-accent-dim/20 bg-accent-dim/15 px-2.5 py-1.5 text-[11px] font-medium text-accent hover:bg-accent-dim/24 disabled:opacity-50"
              type="submit"
              disabled={!newProjectName.trim()}
            >
              Go
            </button>
          </form>
        </div>
      {/if}
    {:else if !worktreeInput}
      <button
        class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-base"
        onclick={showProjectMenu}
      >
        <span class="text-[11px] text-text-secondary">&#9776;</span>
        Set Project
      </button>
      {#if contextMenu.session.isGitRepo}
        <button
          class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-base"
          onclick={showWorktreeInput}
        >
          <span class="text-[11px] text-text-secondary">&#9095;</span>
          New Worktree
        </button>
      {/if}
      <button
        class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-base"
        onclick={handleOpenInCode}
      >
        <span class="text-[11px] text-text-secondary">&#9998;</span>
        Open in Code
      </button>
    {:else}
      <div class="px-3 py-2">
        <div class="mb-1.5 text-[11px] text-text-muted">Branch name</div>
        <form
          onsubmit={(e) => {
            e.preventDefault();
            handleCreateWorktree();
          }}
          class="flex gap-1.5"
        >
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="min-w-0 flex-1 border border-border-subtle bg-bg-deep px-2 py-1.5 font-mono text-[12px] text-text-primary outline-none focus:border-accent-dim/50"
            bind:value={branchName}
            placeholder="feature/my-branch"
            disabled={creatingWorktree}
            autofocus
          />
          <button
            class="cursor-pointer border border-accent-dim/20 bg-accent-dim/15 px-2.5 py-1.5 text-[11px] font-medium text-accent hover:bg-accent-dim/24 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-base"
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
