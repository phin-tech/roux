<script lang="ts">
  import { onDestroy } from "svelte";
  import { get } from "svelte/store";
  import SessionCard from "./SessionCard.svelte";
  import ArchivedSessionsList from "./ArchivedSessionsList.svelte";
  import {
    activeSession,
    activeSessionId,
    sessionState,
    sessionList,
    setActiveSession,
    renameSession,
    addSession,
    updateSessionGitStatus,
    setSessionProject,
  } from "$lib/stores/sessions";
  import { initSessionWithProfile } from "$lib/panes/actions";
  import { defaultAgentProfileId } from "$lib/panes/defaultAgent";
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
  import { projects, createProject, removeProject } from "$lib/stores/projects";
  import { setSessionProject as tauriSetSessionProject } from "$lib/tauri";
  import { log, logError } from "$lib/logging";
  import type { GroupBy, Session, SessionBlueprint, Project } from "$lib/types";
  import { getGroupedSessions } from "$lib/sessions/order";
  import { spawnBlueprintForProject } from "$lib/sessions/spawnBlueprint";
  import {
    openNewProjectDialog,
    openEditProjectDialog,
  } from "$lib/stores/newProjectDialog";
  import { openMainView } from "$lib/stores/mainView";

  import Info from "@lucide/svelte/icons/info";
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

  // Sidebar collapse state is persisted so it survives reload. We also track
  // which project ids have been seen at least once: any *newly* created
  // project lands in the sidebar in collapsed form so it doesn't crowd the
  // view. Once the user has manually toggled a project group, their choice
  // wins.
  const COLLAPSED_GROUPS_KEY = "roux.sidebar.collapsedGroups";
  const SEEN_PROJECTS_KEY = "roux.sidebar.seenProjects";

  function loadStringSet(key: string): Set<string> {
    try {
      if (typeof window === "undefined" || !window.localStorage) return new Set();
      const raw = window.localStorage.getItem(key);
      if (!raw) return new Set();
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? new Set(parsed.filter((v) => typeof v === "string")) : new Set();
    } catch {
      return new Set();
    }
  }

  function saveStringSet(key: string, set: Set<string>): void {
    try {
      if (typeof window === "undefined" || !window.localStorage) return;
      window.localStorage.setItem(key, JSON.stringify([...set]));
    } catch {
      // localStorage write failures are non-fatal — collapse state will just
      // not survive the next reload.
    }
  }

  let collapsedGroups = $state(loadStringSet(COLLAPSED_GROUPS_KEY));
  let seenProjects = $state(loadStringSet(SEEN_PROJECTS_KEY));

  let groupByMode = $derived($settings.groupBy ?? "repo");
  let grouped = $derived(
    getGroupedSessions($sessionList, $projects, groupByMode),
  );
  // In "session" mode there is exactly one synthetic "Sessions" group — its
  // header would just take up space, so we hide it. In repo/project modes a
  // header is useful even with one group (it labels which repo/project).
  let showGroupHeaders = $derived(
    grouped.length > 0 && ($settings.groupBy ?? "repo") !== "session",
  );

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
    saveStringSet(COLLAPSED_GROUPS_KEY, next);
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
    for (const s of $sessionList) {
      if (s.blueprintId) set.add(s.blueprintId);
    }
    return set;
  });

  // Prune persisted entries for projects that no longer exist. Without this,
  // `seenProjects` and `collapsedGroups` accumulate stale ids forever as
  // projects are deleted. Only project ids are pruned: repo-path keys (used
  // in `repo` mode) and "__all__" (session mode) are left alone.
  $effect(() => {
    const validIds = new Set($projects.map((p) => p.id));
    const staleSeen = [...seenProjects].filter((id) => !validIds.has(id));
    if (staleSeen.length === 0) return;
    const newSeen = new Set(seenProjects);
    const newCollapsed = new Set(collapsedGroups);
    let changedCollapsed = false;
    for (const id of staleSeen) {
      newSeen.delete(id);
      if (newCollapsed.has(id)) {
        newCollapsed.delete(id);
        changedCollapsed = true;
      }
    }
    seenProjects = newSeen;
    saveStringSet(SEEN_PROJECTS_KEY, newSeen);
    if (changedCollapsed) {
      collapsedGroups = newCollapsed;
      saveStringSet(COLLAPSED_GROUPS_KEY, newCollapsed);
    }
  });

  // First-sight of a project group: if it has no live sessions yet (a
  // template-only / blueprint-only project), collapse it so it doesn't
  // crowd the sidebar. If it already has live sessions, the user just
  // spawned them via NewProjectDialog and should see them — leave the
  // group expanded. Either way, mark the project as seen so we don't
  // override the user's manual toggle on subsequent renders.
  $effect(() => {
    if (($settings.groupBy ?? "repo") !== "project") return;
    const newSeen = new Set(seenProjects);
    const newCollapsed = new Set(collapsedGroups);
    let changedSeen = false;
    let changedCollapsed = false;
    for (const group of grouped) {
      if (!projectsById.has(group.key)) continue; // skip __untagged__
      if (newSeen.has(group.key)) continue;
      newSeen.add(group.key);
      changedSeen = true;
      if (group.sessions.length === 0 && !newCollapsed.has(group.key)) {
        newCollapsed.add(group.key);
        changedCollapsed = true;
      }
    }
    if (changedSeen) {
      seenProjects = newSeen;
      saveStringSet(SEEN_PROJECTS_KEY, newSeen);
    }
    if (changedCollapsed) {
      collapsedGroups = newCollapsed;
      saveStringSet(COLLAPSED_GROUPS_KEY, newCollapsed);
    }
  });

  function projectBlueprintsForGroup(groupKey: string): SessionBlueprint[] {
    if (($settings.groupBy ?? "repo") !== "project") return [];
    const project = projectsById.get(groupKey);
    if (!project) return [];
    return (project.sessionBlueprints ?? []).filter((bp) => !liveBlueprintIds.has(bp.id));
  }

  let spawningAll = $state(new Set<string>());

  // Spawn every blueprint on a project that does not already have a live
  // session attached. We await each spawn sequentially: each one creates a
  // PTY plus runs profile init, and firing all in parallel both starves the
  // backend and makes failures hard to attribute. Sequential keeps ordering
  // deterministic and matches the per-blueprint click flow.
  async function spawnAllBlueprintsForProject(project: Project) {
    if (spawningAll.has(project.id)) return;
    const blueprints = projectBlueprintsForGroup(project.id);
    if (blueprints.length === 0) return;
    const next = new Set(spawningAll);
    next.add(project.id);
    spawningAll = next;
    try {
      for (const bp of blueprints) {
        await spawnBlueprintFromSidebar(project, bp);
      }
    } finally {
      const done = new Set(spawningAll);
      done.delete(project.id);
      spawningAll = done;
    }
  }

  async function spawnBlueprintFromSidebar(project: Project, bp: SessionBlueprint) {
    try {
      await spawnBlueprintForProject(project, bp);
    } catch (e) {
      logError(`spawnBlueprintFromSidebar failed: ${e}`);
    }
  }

  // Cycle order: repo → project → session → repo. Keeps the default (repo)
  // as the starting point and walks forward predictably on each click.
  const GROUP_BY_CYCLE = ["repo", "project", "session"] as const;

  function nextGroupBy(current: GroupBy): GroupBy {
    const i = GROUP_BY_CYCLE.indexOf(current as (typeof GROUP_BY_CYCLE)[number]);
    return GROUP_BY_CYCLE[(i + 1) % GROUP_BY_CYCLE.length];
  }

  function toggleGroupBy() {
    updateSetting("groupBy", nextGroupBy($settings.groupBy ?? "repo"));
  }

  let contextMenu = $state<{ x: number; y: number; session: Session } | null>(null);
  let groupHeaderMenu = $state<{ x: number; y: number; project: Project } | null>(null);
  let groupHeaderConfirmDelete = $state(false);
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

  // Reset every menu/popover state. Callers use this when opening a new
  // menu so the previous one (session vs project header) doesn't linger
  // and overlap. `closeContextMenu` (the global outside-click handler)
  // also delegates here.
  function resetMenus() {
    contextMenu = null;
    groupHeaderMenu = null;
    groupHeaderConfirmDelete = false;
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

  function handleContextMenu(e: MouseEvent, session: Session) {
    resetMenus();
    contextMenu = { x: e.clientX, y: e.clientY, session };
  }

  function handleOpenSessionDetails() {
    if (!contextMenu) return;
    const id = contextMenu.session.id;
    setActiveSession(id);
    openMainView({ kind: "sessionDetail", sessionId: id });
    closeContextMenu();
  }

  function handleGroupHeaderContextMenu(e: MouseEvent, key: string) {
    const p = projectsById.get(key);
    if (!p) return;
    e.preventDefault();
    e.stopPropagation();
    resetMenus();
    groupHeaderMenu = { x: e.clientX, y: e.clientY, project: p };
  }

  async function handleDeleteProject(project: Project) {
    try {
      await removeProject(project.id);
    } catch (e) {
      logError(`removeProject failed: ${e}`);
    } finally {
      closeContextMenu();
    }
  }

  function closeContextMenu() {
    resetMenus();
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

      const profileId = defaultAgentProfileId();
      const profileRef: SpawnProfileRef = { kind: "registered", id: profileId };
      const { resolveProfileRef } = await import("$lib/panes/profiles");
      const { runProfileInPane } = await import("$lib/panes/profileRunner");
      const profile = resolveProfileRef(profileRef);

      const session = await createSessionShell(
        repo, name, null, branch,
        {
          profile: profileId,
          base: worktreeBase,
          fetchFirst: worktreeFetchFirst,
        },
      );
      log(`Worktree session created: ${session.id}`);
      addSession(session);
      const mainPaneId = initSessionWithProfile(session.id, profileRef);
      const { connectPaneTerminal } = await import("$lib/panes/terminals");
      await connectPaneTerminal(mainPaneId);
      if (profile)
        await runProfileInPane(session.id, profile, {
        });
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
    const worktreePath = $activeSession?.worktreePath ?? null;
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

  // Poll non-git sessions to detect when they become git repos (e.g. after
  // `git init`). Read $sessionList inside the timer so we don't keep
  // refreshing sessions that have since been removed (the prior
  // implementation closed over a stale snapshot).
  $effect(() => {
    const interval = setInterval(() => {
      for (const s of $sessionList) {
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
    const session = $sessionList.find((s) => s.id === id);
    if (!session) return;
    await closeSession(session);
  }

  async function handleReconnect(id: string) {
    const session = $sessionList.find((s) => s.id === id);
    if (!session) return;
    await continueSession(session);
  }

  async function handleArchivedRestore(id: string) {
    const session = get(sessionState).sessions.find((s) => s.id === id);
    if (!session) {
      throw new Error(`restored session ${id} was not returned by listSessions`);
    }
    setActiveSession(id);
    try {
      await continueSession(session);
    } catch (e) {
      logError(`Failed to reconnect restored session ${id}`, e);
      throw e;
    }
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
      title="Group by {nextGroupBy($settings.groupBy ?? 'repo')}"
    >
      <span class="text-text-muted/70">Group</span>
      <span class="text-text-primary">{$settings.groupBy ?? "repo"}</span>
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
          oncontextmenu={(e) => handleGroupHeaderContextMenu(e, group.key)}
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
              active={session.id === $activeSessionId}
              groupBy={groupByMode}
              slotNumber={slotById.get(session.id)}
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
            {@const groupProject = projectsById.get(group.key)}
            {#if groupProject && projectBlueprintsForGroup(group.key).length >= 2}
              <button
                class="mt-0.5 flex w-full cursor-pointer items-center gap-1.5 rounded-md border border-dashed border-accent-dim/30 bg-transparent px-2.5 py-1.5 text-left text-[11px] font-medium text-accent transition-colors hover:border-accent-dim/60 hover:bg-accent-dim/10 disabled:cursor-not-allowed disabled:opacity-60"
                disabled={spawningAll.has(groupProject.id)}
                onclick={() => spawnAllBlueprintsForProject(groupProject)}
                title="Spawn all unspawned blueprints in this project"
              >
                <span class="text-[10px] leading-none">»</span>
                <span class="flex-1 truncate">
                  {spawningAll.has(groupProject.id)
                    ? "Spawning…"
                    : `Spawn all (${projectBlueprintsForGroup(group.key).length})`}
                </span>
              </button>
            {/if}
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
      onrestore={handleArchivedRestore}
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
        onclick={handleOpenSessionDetails}
      >
        <Info size={12} class="text-text-secondary" />
        Session Details
      </button>
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

{#if groupHeaderMenu}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="ui-dialog fixed z-50 min-w-52 py-1"
    style="left: {groupHeaderMenu.x}px; top: {groupHeaderMenu.y}px;"
    onclick={(e) => e.stopPropagation()}
  >
    {#if !groupHeaderConfirmDelete}
      <button
        class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-base"
        onclick={() => {
          if (groupHeaderMenu) openEditProjectDialog(groupHeaderMenu.project);
          closeContextMenu();
        }}
      >
        <span class="text-[11px] text-text-secondary">&#9998;</span>
        Edit project…
      </button>
      <button
        class="flex w-full cursor-pointer items-center gap-2 bg-transparent px-3 py-2 text-left text-xs text-red/85 hover:bg-red/10 hover:text-red focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/40 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-base"
        onclick={() => (groupHeaderConfirmDelete = true)}
      >
        <span class="text-[11px]">&times;</span>
        Delete project…
      </button>
    {:else}
      <div class="px-3 py-2">
        <div class="mb-1.5 text-[11px] text-text-muted">
          Delete <span class="font-mono text-text-primary">{groupHeaderMenu.project.name}</span>?
          <br />Sessions stay (just untagged).
        </div>
        <div class="flex gap-1.5">
          <button
            class="flex-1 cursor-pointer border border-red/30 bg-red/15 px-2.5 py-1.5 text-[11px] font-medium text-red hover:bg-red/24"
            onclick={() => groupHeaderMenu && handleDeleteProject(groupHeaderMenu.project)}
          >
            Delete
          </button>
          <button
            class="flex-1 cursor-pointer border border-border-subtle bg-bg-surface px-2.5 py-1.5 text-[11px] text-text-secondary hover:bg-bg-hover"
            onclick={() => (groupHeaderConfirmDelete = false)}
          >
            Cancel
          </button>
        </div>
      </div>
    {/if}
  </div>
{/if}
