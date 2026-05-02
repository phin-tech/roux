<script lang="ts">
  import { onDestroy } from "svelte";
  import SessionCard from "./SessionCard.svelte";
  import ArchivedSessionsList from "./ArchivedSessionsList.svelte";
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
  import { continueSession } from "$lib/sessions/reconnect";
  import { closeSession } from "$lib/sessions/close";
  import { refreshTasks, initTaskOverrides } from "$lib/stores/tasks";
  import { projects, createProject } from "$lib/stores/projects";
  import { setSessionProject } from "$lib/stores/sessions";
  import { setSessionProject as tauriSetSessionProject } from "$lib/tauri";
  import { log, logError } from "$lib/logging";
  import type { Session, SessionBlueprint, Project } from "$lib/types";
  import { getGroupedSessions } from "$lib/sessions/order";
  import {
    openNewProjectDialog,
    openEditProjectDialog,
  } from "$lib/stores/newProjectDialog";

  import PinButton from "./PinButton.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";

  interface Props {
    onclose?: () => void;
    onNewSession: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let {
    onclose,
    onNewSession,
    pinned = false,
    onTogglePin,
  }: Props = $props();

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

  // Project lookup keyed by id, used to resolve a sidebar group header to
  // its full project record when the user clicks a blueprint row or the
  // group's edit affordance.
  let projectsById = $derived.by(() => {
    const m = new Map<string, Project>();
    for (const p of $projects) m.set(p.id, p);
    return m;
  });

  // Set of blueprint ids that already have a live session attached (the
  // session was spawned from this blueprint). Used to suppress dimmed
  // blueprint rows whose live counterpart is already in the sidebar.
  let liveBlueprintIds = $derived.by(() => {
    const set = new Set<string>();
    for (const s of $sessionState.sessions) {
      if (s.blueprintId) set.add(s.blueprintId);
    }
    return set;
  });

  function projectBlueprintsForGroup(groupKey: string): SessionBlueprint[] {
    if (($settings.groupBy ?? "repo") !== "project") return [];
    const project = projectsById.get(groupKey);
    if (!project) return [];
    return (project.sessionBlueprints ?? []).filter((bp) => !liveBlueprintIds.has(bp.id));
  }

  async function spawnBlueprintFromSidebar(project: Project, bp: SessionBlueprint) {
    try {
      const { resolveProfileRef } = await import("$lib/panes/profiles");
      const { runProfileInPane } = await import("$lib/panes/profileRunner");
      const profileRef: SpawnProfileRef = { kind: "registered", id: bp.spawnProfile };
      const profile = resolveProfileRef(profileRef);
      const nonoProfile = bp.nonoProfile ?? profile?.nonoProfile ?? undefined;
      const nonoAllowDirs =
        bp.nonoAllowDirs && bp.nonoAllowDirs.length > 0
          ? bp.nonoAllowDirs
          : profile?.nonoAllowDirs ?? undefined;
      const newSession = await createSessionShell(
        bp.repoRoot,
        bp.name,
        bp.worktreePath ?? null,
        bp.branch ?? null,
        {
          nonoProfile,
          nonoAllowDirs,
          profile: bp.spawnProfile,
          base: bp.base ?? null,
          fetchFirst: bp.fetchFirst ?? false,
          projectId: project.id,
          blueprintId: bp.id,
        },
      );
      addSession(newSession);
      // Defensive: backend already stamped project_id; keep frontend mirror in sync.
      await tauriSetSessionProject(newSession.id, project.id);
      const mainPaneId = initSessionWithProfile(newSession.id, profileRef, {
        nonoProfile,
        nonoAllowDirs,
      });
      const { connectPaneTerminal } = await import("$lib/panes/terminals");
      await connectPaneTerminal(mainPaneId);
      if (profile) {
        await runProfileInPane(newSession.id, profile, {
          appendSystemPrompt: project.projectPrompt ?? "",
        });
      }
    } catch (e) {
      logError(`spawnBlueprintFromSidebar failed: ${e}`);
    }
  }

  function toggleGroupBy() {
    updateSetting("groupBy", $settings.groupBy === "project" ? "repo" : "project");
  }

  let contextMenu = $state<{ x: number; y: number; session: Session } | null>(null);
  let worktreeInput = $state(false);
  let worktreeBase = $state<string | null>(null);
  let worktreeBaseLabel = $state("");
  let worktreeFetchFirst = $state(false);
  let branchName = $state("");
  let creatingWorktree = $state(false);
  let worktreeError = $state("");
  let projectMenu = $state(false);
  let newProjectInput = $state(false);
  let newProjectName = $state("");
  let lastTaskWorktreePath = $state<string | null>(null);
  let rootEl = $state<HTMLDivElement | null>(null);
  let archivedCollapsed = $state(true);
  let archivedHeight = $state(180);
  let archivedDragging = $state(false);
  let archivedDragTeardown: (() => void) | null = null;

  function handleContextMenu(e: MouseEvent, session: Session) {
    contextMenu = { x: e.clientX, y: e.clientY, session };
    worktreeInput = false;
    worktreeBase = null;
    worktreeBaseLabel = "";
    worktreeFetchFirst = false;
    branchName = "";
    worktreeError = "";
  }

  function closeContextMenu() {
    contextMenu = null;
    worktreeInput = false;
    worktreeBase = null;
    worktreeBaseLabel = "";
    worktreeFetchFirst = false;
    branchName = "";
    worktreeError = "";
    projectMenu = false;
    newProjectInput = false;
    newProjectName = "";
  }

  function pickWorktreeBase(base: string | null, label: string, fetchFirst: boolean) {
    worktreeBase = base;
    worktreeBaseLabel = label;
    worktreeFetchFirst = fetchFirst;
    worktreeInput = true;
  }

  // Detached-HEAD sessions report `branch` as "" — treat that as "branch
  // from HEAD" (null) rather than passing an empty start point through to
  // backend `rev-parse --verify`.
  function currentBranchBase(session: Session): string | null {
    const trimmed = session.branch.trim();
    return trimmed ? trimmed : null;
  }

  // Keyboard handler for the "New Worktree" trigger button: ArrowRight /
  // ArrowDown opens the hover flyout by focusing its first menuitem.
  // Enter/Space keep their native button behavior (activate onclick =
  // pickDefaultWorktreeBase), so keyboard users get the same click-default
  // semantics as mouse users.
  function handleWorktreeTriggerKeydown(e: KeyboardEvent) {
    if (e.key !== "ArrowRight" && e.key !== "ArrowDown") return;
    e.preventDefault();
    const trigger = e.currentTarget as HTMLElement;
    const submenu = trigger.nextElementSibling;
    const first = submenu?.querySelector<HTMLButtonElement>('button[role="menuitem"]');
    first?.focus();
  }

  // Keyboard handler for items inside the "Branch from" flyout:
  // Arrow up/down cycle through items, Home/End jump to endpoints, and
  // Escape returns focus to the trigger (which collapses the flyout via
  // `group-focus-within`).
  function handleWorktreeMenuItemKeydown(e: KeyboardEvent) {
    const current = e.currentTarget as HTMLButtonElement;
    const parent = current.parentElement;
    if (!parent) return;
    const items = Array.from(
      parent.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]'),
    );
    const i = items.indexOf(current);
    if (i < 0) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        items[(i + 1) % items.length]?.focus();
        break;
      case "ArrowUp":
        e.preventDefault();
        items[(i - 1 + items.length) % items.length]?.focus();
        break;
      case "Home":
        e.preventDefault();
        items[0]?.focus();
        break;
      case "End":
        e.preventDefault();
        items[items.length - 1]?.focus();
        break;
      case "Escape": {
        e.preventDefault();
        // Escape returns focus to the trigger, which collapses the flyout
        // (the outer .group loses :focus-within). A second Escape would
        // then need to be handled by the context-menu level — currently
        // the context menu closes on outside click, not Escape.
        const trigger = parent.parentElement?.querySelector<HTMLButtonElement>(
          ":scope > button",
        );
        trigger?.focus();
        break;
      }
    }
  }

  function pickDefaultWorktreeBase() {
    const session = contextMenu?.session;
    if (!session) return;
    switch ($settings.worktreeDefaultBase ?? "currentBranch") {
      case "main":
        pickWorktreeBase("main", "main", false);
        break;
      case "originMain":
        pickWorktreeBase("origin/main", "origin/main", true);
        break;
      case "currentBranch":
      default:
        pickWorktreeBase(currentBranchBase(session), "current branch", false);
        break;
    }
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
        {
          nonoProfile,
          nonoAllowDirs,
          profile: "claude",
          base: worktreeBase,
          fetchFirst: worktreeFetchFirst,
        },
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
    try {
      await openInEditor(contextMenu.session.worktreePath);
    } catch (e) {
      logError("Failed to open in editor", e);
    } finally {
      closeContextMenu();
    }
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
    await continueSession(session);
  }

  function archivedMaxHeight(): number {
    if (!rootEl) return 420;
    const rect = rootEl.getBoundingClientRect();
    return Math.max(140, Math.min(520, rect.height * 0.65));
  }

  function clampArchivedHeight(height: number): number {
    return Math.max(96, Math.min(archivedMaxHeight(), height));
  }

  function archivedSectionStyle(): string {
    return archivedCollapsed ? "" : `height: ${archivedHeight}px;`;
  }

  function endArchivedDrag(): void {
    archivedDragTeardown?.();
    archivedDragTeardown = null;
    archivedDragging = false;
  }

  function onArchivedResizeStart(e: MouseEvent): void {
    if (archivedCollapsed) return;
    e.preventDefault();
    endArchivedDrag();
    archivedDragging = true;
    const startY = e.clientY;
    const startHeight = archivedHeight;
    const onMove = (ev: MouseEvent) => {
      archivedHeight = clampArchivedHeight(startHeight - (ev.clientY - startY));
    };
    const onUp = () => endArchivedDrag();
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") endArchivedDrag();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("blur", endArchivedDrag);
    window.addEventListener("keydown", onKey);
    archivedDragTeardown = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("blur", endArchivedDrag);
      window.removeEventListener("keydown", onKey);
    };
  }

  onDestroy(() => endArchivedDrag());

</script>

<svelte:window onclick={closeContextMenu} />

<div
  bind:this={rootEl}
  class="flex h-full flex-col overflow-hidden bg-bg-base/96 shadow-[0_0_0_1px_rgba(255,255,255,0.03)]"
>
  <SidebarPanelHeader title="Sessions">
    {#snippet actions()}
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      {#if onclose}
        <CollapseSidebarButton
          onclick={onclose}
          label="Collapse sessions sidebar"
          title="Collapse sessions sidebar"
        />
      {/if}
    {/snippet}
  </SidebarPanelHeader>

  <div class="flex shrink-0 items-center gap-1.5 border-b border-hairline p-2">
    <button
      class="flex h-8 min-w-0 flex-1 cursor-pointer items-center justify-center gap-1.5 border border-accent-dim/20 bg-accent-dim/15 px-3 text-[13px] font-semibold text-accent transition-all duration-150 hover:bg-accent-dim/24 hover:text-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
      onclick={onNewSession}
    >
      <span class="relative -top-px text-sm leading-none">+</span>
      <span>New</span>
    </button>
    <button
      class="flex h-8 shrink-0 cursor-pointer items-center gap-1.5 border border-border-subtle bg-bg-surface px-3 text-[11px] font-medium text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent-dim/50"
      onclick={toggleGroupBy}
      title="Group by {$settings.groupBy === 'project' ? 'repo' : 'project'}"
    >
      <span class="text-text-muted/70">Group</span>
      <span class="text-text-primary">{$settings.groupBy === "project" ? "project" : "repo"}</span>
    </button>
    <button
      class="flex h-8 shrink-0 cursor-pointer items-center gap-1 border border-border-subtle bg-bg-surface px-2.5 text-[11px] font-medium text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent-dim/50"
      onclick={openNewProjectDialog}
      title="New project"
    >
      <span class="text-sm leading-none">+</span>
      <span>Project</span>
    </button>
  </div>

  <div class="app-scrollbar min-h-0 flex-1 overflow-y-auto px-2">
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
          {#each projectBlueprintsForGroup(group.key) as bp (bp.id)}
            {@const project = projectsById.get(group.key)}
            {#if project}
              <button
                class="group/bp mt-0.5 flex w-full cursor-pointer items-center gap-1.5 rounded-md border border-dashed border-border-subtle/60 bg-transparent px-2.5 py-1.5 text-left text-[11px] text-text-muted transition-colors hover:border-accent-dim/40 hover:bg-bg-hover hover:text-text-primary"
                onclick={() => spawnBlueprintFromSidebar(project, bp)}
                title="Spawn blueprint: {bp.name}"
              >
                <span class="text-[10px] leading-none">+</span>
                <span class="flex-1 truncate font-mono">{bp.name}</span>
                {#if bp.branch}
                  <span class="shrink-0 text-[10px] opacity-70">{bp.branch}</span>
                {/if}
              </button>
            {/if}
          {/each}
          {#if ($settings.groupBy ?? "repo") === "project" && projectsById.has(group.key)}
            <button
              class="mt-0.5 flex w-full cursor-pointer items-center gap-1.5 rounded-md border border-transparent bg-transparent px-2.5 py-1 text-left text-[10px] text-text-muted/70 hover:bg-bg-hover hover:text-text-primary"
              onclick={() => {
                const p = projectsById.get(group.key);
                if (p) openEditProjectDialog(p);
              }}
            >
              <span>edit project…</span>
            </button>
          {/if}
        </div>
      {/if}
    {/each}
  </div>

  <div class="min-h-0 shrink-0 px-2 pb-2" style={archivedSectionStyle()}>
    <ArchivedSessionsList
      collapsed={archivedCollapsed}
      oncollapsedchange={(next) => (archivedCollapsed = next)}
      onresizestart={onArchivedResizeStart}
      resizing={archivedDragging}
    />
  </div>

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
        <div class="group relative">
          <button
            class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary group-hover:bg-bg-hover group-hover:text-text-primary group-focus-within:bg-bg-hover group-focus-within:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-base"
            onclick={pickDefaultWorktreeBase}
            onkeydown={handleWorktreeTriggerKeydown}
            aria-haspopup="menu"
          >
            <span class="text-[11px] text-text-secondary">&#9095;</span>
            New Worktree
            <span class="ml-auto text-[10px] text-text-muted">&#9654;</span>
          </button>
          <!--
            Flyout submenu: appears to the right, aligned to the button's top.
            Visible on hover (mouse) OR on focus-within (keyboard tabbing into
            any menuitem) — so keyboard-only users can reach the three base
            options without the pointer.
          -->
          <div
            class="ui-dialog invisible absolute left-full top-0 z-50 ml-0.5 min-w-48 py-1 opacity-0 transition-opacity duration-75 group-hover:visible group-hover:opacity-100 group-focus-within:visible group-focus-within:opacity-100"
            role="menu"
            aria-label="Branch from"
          >
            <div class="px-2 py-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">Branch from</div>
            <button
              class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              role="menuitem"
              onclick={() => contextMenu && pickWorktreeBase(currentBranchBase(contextMenu.session), "current branch", false)}
              onkeydown={handleWorktreeMenuItemKeydown}
            >
              Current branch
            </button>
            <button
              class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              role="menuitem"
              onclick={() => pickWorktreeBase("main", "main", false)}
              onkeydown={handleWorktreeMenuItemKeydown}
            >
              main
            </button>
            <button
              class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-1.5 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              role="menuitem"
              onclick={() => pickWorktreeBase("origin/main", "origin/main", true)}
              onkeydown={handleWorktreeMenuItemKeydown}
            >
              origin/main
            </button>
          </div>
        </div>
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
        <div class="mb-1.5 text-[11px] text-text-muted">
          New branch from {worktreeBaseLabel}
        </div>
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
