<script lang="ts">
  import PinButton from "./PinButton.svelte";
  import { commands } from "$lib/bindings";
  import type {
    WorktrunkDiagnostics,
    WorktrunkLogEntry,
    WorktrunkHookOutputEntry,
  } from "$lib/bindings";
  import {
    activeSession,
    addSession,
    sessionState,
    setActiveSession,
  } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import { worktrunkDetection } from "$lib/stores/worktrunkDetection";
  import {
    createSessionShell,
    createWorktree,
    listWorktrees,
    removeWorktree,
  } from "$lib/tauri";
  import type { Session, Worktree, WorktreeDefaultBase } from "$lib/types";
  import { upsertWorktreeMetadata } from "$lib/stores/worktreeMetadata";
  import WorktreeRowContent from "./WorktreeRowContent.svelte";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();

  type Tab = "worktrees" | "hooks" | "commandLog" | "hookOutput" | "diagnostic";
  let activeTab = $state<Tab>("worktrees");

  let diagnostics = $state<WorktrunkDiagnostics | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  // Worktree-list state, scoped to the active session's repo.
  let worktrees = $state<Worktree[]>([]);
  let worktreesError = $state<string | null>(null);
  let worktreesLoading = $state(false);
  let removing = $state<string | null>(null); // path currently being removed
  let menuOpenFor = $state<string | null>(null); // kebab menu target

  // "New worktree" inline form state.
  let newFormOpen = $state(false);
  let newBranch = $state("");
  let newBase = $state<WorktreeDefaultBase>("currentBranch");
  let creatingNew = $state(false);

  // Sync the base selector to the user's default whenever the form opens.
  $effect(() => {
    if (newFormOpen) {
      newBase = ($settings.worktreeDefaultBase ?? "currentBranch");
    }
  });

  function resolveNewWorktreeBase(choice: WorktreeDefaultBase): {
    startPoint: string | null;
    fetchFirst: boolean;
  } {
    switch (choice) {
      case "main":
        return { startPoint: "main", fetchFirst: false };
      case "originMain":
        return { startPoint: "origin/main", fetchFirst: true };
      case "currentBranch":
      default:
        return { startPoint: null, fetchFirst: false };
    }
  }

  async function handleCreateNew() {
    if (!currentRepo) return;
    const branch = newBranch.trim();
    if (!branch) {
      worktreesError = "Branch name is required";
      return;
    }
    creatingNew = true;
    worktreesError = null;
    try {
      const { startPoint, fetchFirst } = resolveNewWorktreeBase(newBase);
      await createWorktree(currentRepo, branch, { startPoint, fetchFirst });
      newBranch = "";
      newFormOpen = false;
      await loadWorktrees(currentRepo);
    } catch (err) {
      const msg = typeof err === "string" ? err : String(err);
      worktreesError = `Failed to create worktree: ${msg}`;
    } finally {
      creatingNew = false;
    }
  }

  // Log-content reader state. When a user clicks a log entry row we load
  // its body into this side-pane so they can inspect failures without
  // leaving the panel.
  let readerPath = $state<string | null>(null);
  let readerContent = $state<string | null>(null);
  let readerError = $state<string | null>(null);
  let readerLoading = $state(false);

  let currentRepo = $derived($activeSession?.repoRoot ?? null);

  /**
   * Shorten a repo path for header display. Looks for a known forge
   * host segment (`github.com` / `gitlab.com` / `bitbucket.org`) and
   * returns the next two segments as `owner/repo`. Falls back to the
   * last two path segments. The full absolute path stays in the
   * tooltip so users can still eyeball where on disk they are.
   */
  function shortRepo(path: string | null): string {
    if (!path) return "";
    const segments = path.split("/").filter(Boolean);
    const FORGE_HOSTS = new Set([
      "github.com",
      "gitlab.com",
      "bitbucket.org",
      "codeberg.org",
    ]);
    for (let i = 0; i < segments.length - 2; i++) {
      if (FORGE_HOSTS.has(segments[i])) {
        return `${segments[i + 1]}/${segments[i + 2]}`;
      }
    }
    if (segments.length >= 2) {
      return `${segments[segments.length - 2]}/${segments[segments.length - 1]}`;
    }
    return segments[segments.length - 1] ?? path;
  }

  let currentRepoLabel = $derived(shortRepo(currentRepo));

  // Map of worktree path → first active (non-archived) session that
  // owns it. Used both to disable remove on rows with a running session
  // AND to pick between Focus / New-session buttons.
  let sessionByWorktreePath = $derived.by(() => {
    const map = new Map<string, Session>();
    for (const s of $sessionState.sessions) {
      if (s.archived) continue;
      if (!map.has(s.worktreePath)) map.set(s.worktreePath, s);
    }
    return map;
  });
  let worktreePathsWithSession = $derived(
    new Set(sessionByWorktreePath.keys()),
  );

  // Right-click context menu state. `contextMenuFor` is the worktree
  // whose menu is open; `contextMenuPos` is the on-screen origin.
  let contextMenuFor = $state<Worktree | null>(null);
  let contextMenuPos = $state<{ x: number; y: number }>({ x: 0, y: 0 });
  let contextBusy = $state(false);
  let contextError = $state<string | null>(null);

  function openContextMenu(e: MouseEvent, wt: Worktree) {
    e.preventDefault();
    menuOpenFor = null;
    contextError = null;
    contextMenuFor = wt;
    contextMenuPos = { x: e.clientX, y: e.clientY };
  }

  function closeContextMenu() {
    contextMenuFor = null;
    contextBusy = false;
  }

  async function handleCopyPath(wt: Worktree) {
    try {
      await navigator.clipboard.writeText(wt.path);
      closeContextMenu();
    } catch (err) {
      contextError = `Copy failed: ${err}`;
    }
  }

  async function handleRevealInFinder(wt: Worktree) {
    contextBusy = true;
    try {
      await revealItemInDir(wt.path);
      closeContextMenu();
    } catch (err) {
      contextError = `Reveal failed: ${err}`;
    } finally {
      contextBusy = false;
    }
  }

  async function handleOpenTerminal(wt: Worktree) {
    contextBusy = true;
    try {
      const res = await commands.cmdOpenTerminalAt(wt.path);
      if (res.status === "error") {
        contextError = res.error;
      } else {
        closeContextMenu();
      }
    } catch (err) {
      contextError = typeof err === "string" ? err : String(err);
    } finally {
      contextBusy = false;
    }
  }

  // Session-spawning state per worktree path.
  let spawning = $state<string | null>(null);

  async function handleFocus(session: Session) {
    setActiveSession(session.id);
  }

  async function handleNewSessionHere(wt: Worktree) {
    if (!currentRepo) return;
    spawning = wt.path;
    worktreesError = null;
    try {
      const { resolveProfileRef } = await import("$lib/panes/profiles");
      const { runProfileInPane } = await import("$lib/panes/profileRunner");
      const { initSessionWithProfile } = await import("$lib/panes/actions");
      const { connectPaneTerminal } = await import("$lib/panes/terminals");
      const profileRef = { kind: "registered" as const, id: "claude" };
      const profile = resolveProfileRef(profileRef);
      const nonoProfile = profile?.nonoProfile ?? undefined;
      const nonoAllowDirs = profile?.nonoAllowDirs ?? undefined;

      const name = wt.branch || currentRepo.split("/").pop() || "shell";
      const newSession = await createSessionShell(
        currentRepo,
        name,
        wt.path,
        wt.branch,
        { nonoProfile, nonoAllowDirs, profile: "claude" },
      );
      addSession(newSession);
      const mainPaneId = initSessionWithProfile(newSession.id, profileRef, {
        nonoProfile,
        nonoAllowDirs,
      });
      await connectPaneTerminal(mainPaneId);
      if (profile) await runProfileInPane(newSession.id, profile);
      setActiveSession(newSession.id);
    } catch (err) {
      worktreesError = `Failed to start session: ${err}`;
    } finally {
      spawning = null;
    }
  }

  $effect(() => {
    void visible;
    void currentRepo;
    if (!visible) return;
    if (!currentRepo) {
      diagnostics = null;
      worktrees = [];
      error = null;
      worktreesError = null;
      return;
    }
    void loadDiagnostics(currentRepo);
    void loadWorktrees(currentRepo);
  });

  async function loadDiagnostics(repoPath: string) {
    loading = true;
    error = null;
    try {
      const result = await commands.cmdWorktrunkDiagnostics(repoPath);
      if (result.status === "ok") {
        diagnostics = result.data;
      } else {
        diagnostics = null;
        error = result.error;
      }
    } catch (err) {
      diagnostics = null;
      error = typeof err === "string" ? err : String(err);
    } finally {
      loading = false;
    }
  }

  async function loadWorktrees(repoPath: string) {
    worktreesLoading = true;
    worktreesError = null;
    try {
      const entries = await listWorktrees(repoPath);
      worktrees = entries;
      // Feed the shared store so chips on session cards pick up any
      // freshly-listed metadata too.
      upsertWorktreeMetadata(entries);
    } catch (err) {
      worktrees = [];
      worktreesError = typeof err === "string" ? err : String(err);
    } finally {
      worktreesLoading = false;
    }
  }

  async function handleRemove(wt: Worktree, alsoBranch: boolean) {
    if (!currentRepo) return;
    // Belt-and-suspenders: the Remove buttons set `disabled` when these
    // conditions hold, but refuse at the handler layer too so a future
    // refactor that misses the disabled attr can't delete the main
    // clone or yank the rug from under a running session.
    if (wt.isMain) {
      worktreesError = `Refusing to remove the main worktree (${wt.branch}).`;
      return;
    }
    if (worktreePathsWithSession.has(wt.path)) {
      worktreesError = `Refusing to remove ${wt.branch} — a Roux session is active in it. Close the session first.`;
      return;
    }
    menuOpenFor = null;
    const label = alsoBranch
      ? `Remove worktree AND delete branch "${wt.branch}"?`
      : `Remove worktree "${wt.branch}"?`;
    const confirmed = window.confirm(
      `${label}\n\n` +
        `Path: ${wt.path}\n\n` +
        (alsoBranch
          ? "Both the on-disk worktree and the local branch will be deleted."
          : "The worktree is removed from disk; the branch stays in the repo."),
    );
    if (!confirmed) return;
    removing = wt.path;
    try {
      await removeWorktree(currentRepo, wt.path, alsoBranch);
      await loadWorktrees(currentRepo);
    } catch (err) {
      const msg = typeof err === "string" ? err : String(err);
      worktreesError = `Failed to remove ${wt.branch}: ${msg}`;
    } finally {
      removing = null;
    }
  }

  async function openLogEntry(path: string) {
    readerPath = path;
    readerContent = null;
    readerError = null;
    readerLoading = true;
    try {
      const result = await commands.cmdWorktrunkReadLog(path);
      if (result.status === "ok") {
        readerContent = result.data ?? "";
      } else {
        readerError = result.error;
      }
    } catch (err) {
      readerError = typeof err === "string" ? err : String(err);
    } finally {
      readerLoading = false;
    }
  }

  function closeReader() {
    readerPath = null;
    readerContent = null;
    readerError = null;
  }

  function formatRelativeTime(unixSecs: number | null): string {
    if (unixSecs == null) return "—";
    const diff = Date.now() / 1000 - unixSecs;
    if (diff < 60) return "just now";
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
</script>

<div
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <div
    class="flex h-9 shrink-0 items-center gap-2 border-b border-hairline bg-bg-surface/30 px-3"
  >
    <span class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
      Worktrunk
    </span>
    {#if $worktrunkDetection.version}
      <span
        class="rounded bg-green/10 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-green"
      >
        {$worktrunkDetection.version}
      </span>
    {/if}
    <div class="ml-auto flex items-center gap-1">
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <button
        class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-base text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
        onclick={onclose}
        aria-label="Close worktrunk panel">&times;</button
      >
    </div>
  </div>
  {#if currentRepo}
    <div
      data-testid="worktrunk-repo-strip"
      class="flex h-6 shrink-0 items-center gap-2 border-b border-hairline bg-bg-surface/20 px-3"
      title={currentRepo}
    >
      <span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted"
        >Repo</span
      >
      <span
        data-testid="worktrunk-repo-label"
        class="truncate font-mono text-[10px] text-text-secondary"
        >{currentRepoLabel}</span
      >
    </div>
  {/if}

  {#if !currentRepo}
    <div
      class="flex flex-1 items-center justify-center px-6 text-center text-sm text-text-secondary"
    >
      Open a session to view its worktrunk state.
    </div>
  {:else}
    <div class="flex shrink-0 border-b border-hairline bg-bg-surface/20 text-[11px]">
      {#each [
        { id: "worktrees", label: "Worktrees", count: worktrees.length },
        { id: "hooks", label: "Hooks", count: diagnostics?.hooks.length ?? 0 },
        {
          id: "commandLog",
          label: "Command log",
          count: diagnostics?.logs.commandLog.length ?? 0,
        },
        {
          id: "hookOutput",
          label: "Hook output",
          count: diagnostics?.logs.hookOutput.length ?? 0,
        },
        {
          id: "diagnostic",
          label: "Diagnostic",
          count: diagnostics?.logs.diagnostic.length ?? 0,
        },
      ] as const as tab}
        <button
          data-testid={`worktrunk-tab-${tab.id}`}
          class="cursor-pointer px-3 py-2 transition-colors
            {activeTab === tab.id
            ? 'border-b-2 border-accent text-text-primary'
            : 'text-text-secondary hover:bg-bg-hover'}"
          onclick={() => (activeTab = tab.id)}
        >
          {tab.label}
          {#if tab.count > 0}
            <span
              class="ml-1 rounded bg-bg-active px-1 text-[9px] font-semibold text-text-muted"
              >{tab.count}</span
            >
          {/if}
        </button>
      {/each}
    </div>

    {#if activeTab === "worktrees"}
      <div class="flex flex-1 min-h-0 flex-col overflow-auto p-3">
        <div class="mb-2 flex items-center justify-between">
          <span class="text-[10px] uppercase tracking-wider text-text-muted"
            >{worktrees.length}
            {worktrees.length === 1 ? "worktree" : "worktrees"}</span
          >
          <button
            data-testid="worktrunk-new-worktree-open"
            class="cursor-pointer rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-primary hover:border-accent hover:text-accent"
            onclick={() => (newFormOpen = !newFormOpen)}
          >{newFormOpen ? "Cancel" : "+ New worktree"}</button>
        </div>

        {#if newFormOpen}
          <div
            data-testid="worktrunk-new-worktree-form"
            class="mb-3 rounded border border-accent-dim/40 bg-bg-surface/40 p-2"
          >
            <label
              class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-muted"
              for="wt-new-branch">Branch name</label
            >
            <input
              id="wt-new-branch"
              data-testid="worktrunk-new-worktree-branch"
              bind:value={newBranch}
              placeholder="feat/my-feature"
              class="mb-2 w-full rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
              onkeydown={(e) => {
                if (e.key === "Enter" && !creatingNew) handleCreateNew();
                if (e.key === "Escape") newFormOpen = false;
              }}
            />
            <label
              class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-muted"
              for="wt-new-base">Branch from</label
            >
            <div class="mb-2 flex overflow-hidden rounded border border-border bg-bg-deep text-[11px]">
              {#each [
                { id: "currentBranch", label: "Current" },
                { id: "main", label: "main" },
                { id: "originMain", label: "origin/main" },
              ] as const as opt}
                <button
                  data-testid={`worktrunk-new-worktree-base-${opt.id}`}
                  class="cursor-pointer px-2.5 py-1 transition-colors
                    {newBase === opt.id
                    ? 'bg-accent-dim text-text-primary'
                    : 'text-text-secondary hover:bg-bg-hover'}"
                  onclick={() => (newBase = opt.id)}>{opt.label}</button>
              {/each}
            </div>
            <div class="flex justify-end gap-2">
              <button
                class="cursor-pointer rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:bg-bg-hover"
                onclick={() => (newFormOpen = false)}
                disabled={creatingNew}>Cancel</button>
              <button
                data-testid="worktrunk-new-worktree-submit"
                class="cursor-pointer rounded border border-accent bg-accent-dim/20 px-2 py-0.5 text-[10px] text-accent hover:bg-accent-dim/40 disabled:opacity-40"
                onclick={handleCreateNew}
                disabled={creatingNew || !newBranch.trim()}
              >{creatingNew ? "Creating…" : "Create"}</button>
            </div>
          </div>
        {/if}

        {#if worktreesLoading && worktrees.length === 0}
          <div class="text-sm text-text-muted">Loading…</div>
        {:else if worktreesError}
          <div
            data-testid="worktrunk-worktrees-error"
            class="mb-2 rounded border border-red/30 bg-red/10 p-2 text-xs text-red"
          >
            {worktreesError}
          </div>
        {/if}
        {#if worktrees.length === 0 && !worktreesLoading}
          <div
            class="rounded border border-border-subtle bg-bg-surface/30 p-3 text-sm text-text-muted"
          >
            No worktrees.
          </div>
        {:else}
          <ul class="flex flex-col gap-2">
            {#each worktrees as wt (wt.path)}
              {@const session = sessionByWorktreePath.get(wt.path)}
              {@const hasSession = session !== undefined}
              {@const cannotRemove = wt.isMain || hasSession}
              {@const isRemoving = removing === wt.path}
              {@const isSpawning = spawning === wt.path}
              <li
                data-testid="worktrunk-worktree-row"
                class="relative rounded border border-border-subtle bg-bg-surface/30 p-2"
                oncontextmenu={(e) => openContextMenu(e, wt)}
              >
                <div class="flex items-center gap-2">
                  <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                    <WorktreeRowContent {wt} />
                  </div>
                  <div class="flex shrink-0 items-center gap-1">
                    {#if hasSession}
                      <button
                        data-testid="worktrunk-worktree-focus"
                        class="cursor-pointer rounded border border-accent-dim/40 bg-accent-dim/20 px-2 py-0.5 text-[10px] text-accent hover:bg-accent-dim/40"
                        onclick={() => handleFocus(session!)}
                        title={`Focus the "${session!.name}" session`}
                      >Focus</button>
                    {:else}
                      <button
                        data-testid="worktrunk-worktree-new-session"
                        class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary
                          enabled:cursor-pointer enabled:hover:border-accent enabled:hover:text-accent
                          disabled:opacity-40"
                        disabled={isSpawning}
                        onclick={() => handleNewSessionHere(wt)}
                        title="Start a new Claude session in this worktree"
                      >{isSpawning ? "Starting…" : "New session"}</button>
                    {/if}
                    <button
                      data-testid="worktrunk-worktree-menu"
                      class="rounded border border-border-subtle bg-bg-elevated px-1.5 py-0.5 text-[10px] text-text-secondary
                        enabled:cursor-pointer enabled:hover:bg-bg-hover
                        disabled:opacity-40"
                      disabled={isRemoving || isSpawning}
                      onclick={() =>
                        (menuOpenFor = menuOpenFor === wt.path ? null : wt.path)}
                      aria-label="More actions"
                    >⋮</button>
                  </div>
                </div>
                {#if menuOpenFor === wt.path}
                  <div
                    data-testid="worktrunk-worktree-menu-content"
                    class="absolute right-2 top-9 z-10 flex flex-col rounded border border-border bg-bg-elevated p-1 shadow-lg"
                  >
                    <button
                      data-testid="worktrunk-worktree-remove"
                      class="rounded px-2 py-1 text-left text-[11px] text-text-primary
                        enabled:cursor-pointer enabled:hover:bg-red/20
                        disabled:opacity-40"
                      disabled={cannotRemove}
                      onclick={() => handleRemove(wt, false)}
                      title={wt.isMain
                        ? "Cannot remove the main worktree"
                        : hasSession
                          ? "A Roux session is active — close it first"
                          : "Remove the worktree on disk (keep the branch)"}
                    >Remove worktree</button>
                    <button
                      data-testid="worktrunk-worktree-remove-and-branch"
                      class="rounded px-2 py-1 text-left text-[11px] text-text-primary
                        enabled:cursor-pointer enabled:hover:bg-red/20
                        disabled:opacity-40"
                      disabled={cannotRemove}
                      onclick={() => handleRemove(wt, true)}
                    >Remove worktree + branch</button>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {:else if loading && !diagnostics}
      <div class="flex flex-1 items-center justify-center text-sm text-text-muted">
        Loading…
      </div>
    {:else if error}
      <div class="p-4 text-sm text-red">Failed to load diagnostics: {error}</div>
    {:else if diagnostics}
      <div class="flex flex-1 min-h-0 overflow-hidden">
        <div class="flex flex-1 flex-col overflow-auto">
          {#if activeTab === "hooks"}
            <div class="p-3">
              <div class="mb-2 text-[11px] text-text-muted">
                From
                <a
                  href={`file://${diagnostics.config.userPath}`}
                  target="_blank"
                  rel="noreferrer"
                  class="font-mono text-blue underline"
                  class:opacity-50={!diagnostics.config.userExists}
                  title={diagnostics.config.userExists
                    ? "Open user config"
                    : "User config does not exist"}
                >
                  user config
                </a>
                and
                <a
                  href={`file://${diagnostics.config.projectPath}`}
                  target="_blank"
                  rel="noreferrer"
                  class="font-mono text-blue underline"
                  class:opacity-50={!diagnostics.config.projectExists}
                  title={diagnostics.config.projectExists
                    ? "Open project config"
                    : "Project config does not exist"}
                >
                  project config
                </a>.
              </div>
              {#if diagnostics.hooks.length === 0}
                <div
                  data-testid="worktrunk-hooks-empty"
                  class="rounded border border-border-subtle bg-bg-surface/30 p-3 text-sm text-text-muted"
                >
                  No hooks defined.
                  <a
                    data-testid="worktrunk-hooks-docs-link"
                    href="https://worktrunk.dev/hook/"
                    target="_blank"
                    rel="noreferrer"
                    class="text-blue underline"
                  >Learn about worktrunk hooks</a>.
                </div>
              {:else}
                <ul class="flex flex-col gap-2">
                  {#each diagnostics.hooks as hook (hook.source + hook.name)}
                    <li
                      data-testid="worktrunk-hook-row"
                      class="rounded border border-border-subtle bg-bg-surface/30 p-2"
                    >
                      <div class="flex items-center gap-2">
                        <span
                          class="rounded bg-accent-dim/20 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-accent"
                          >{hook.source}</span
                        >
                        <span class="font-mono text-xs text-text-primary"
                          >{hook.name}</span
                        >
                      </div>
                      <pre
                        class="mt-1 overflow-x-auto whitespace-pre-wrap break-words font-mono text-[11px] text-text-secondary">{hook.command}</pre>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {:else if activeTab === "commandLog"}
            {@render logList(diagnostics.logs.commandLog)}
          {:else if activeTab === "hookOutput"}
            {@render hookOutputList(diagnostics.logs.hookOutput)}
          {:else if activeTab === "diagnostic"}
            {@render logList(diagnostics.logs.diagnostic)}
          {/if}
        </div>

        {#if readerPath}
          <div class="flex w-1/2 min-h-0 flex-col border-l border-hairline">
            <div
              class="flex h-8 shrink-0 items-center gap-2 border-b border-hairline bg-bg-surface/30 px-3"
            >
              <span class="flex-1 truncate font-mono text-[10px] text-text-muted"
                >{readerPath}</span
              >
              <button
                data-testid="worktrunk-reader-close"
                class="cursor-pointer rounded border border-transparent bg-transparent px-1 text-xs text-text-muted hover:border-border-subtle hover:text-text-primary"
                onclick={closeReader}
                aria-label="Close log reader">&times;</button
              >
            </div>
            <div class="flex-1 overflow-auto p-2">
              {#if readerLoading}
                <div class="text-xs text-text-muted">Loading…</div>
              {:else if readerError}
                <div class="text-xs text-red">Failed: {readerError}</div>
              {:else if readerContent === ""}
                <div class="text-xs text-text-muted">(empty)</div>
              {:else if readerContent}
                <pre
                  data-testid="worktrunk-reader-content"
                  class="whitespace-pre-wrap break-words font-mono text-[11px] text-text-secondary">{readerContent}</pre>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</div>

{#if contextMenuFor}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-40"
    onclick={closeContextMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeContextMenu();
    }}
  ></div>
  <div
    data-testid="worktrunk-context-menu"
    class="fixed z-50 flex min-w-40 flex-col rounded border border-border bg-bg-elevated p-1 shadow-xl"
    style:left={`${contextMenuPos.x}px`}
    style:top={`${contextMenuPos.y}px`}
  >
    <div class="border-b border-border-subtle/40 px-2 py-1 font-mono text-[10px] text-text-muted">
      {contextMenuFor.branch}
    </div>
    <button
      data-testid="worktrunk-context-copy"
      class="cursor-pointer rounded px-2 py-1 text-left text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-40"
      disabled={contextBusy}
      onclick={() => handleCopyPath(contextMenuFor!)}
    >Copy path</button>
    <button
      data-testid="worktrunk-context-reveal"
      class="cursor-pointer rounded px-2 py-1 text-left text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-40"
      disabled={contextBusy}
      onclick={() => handleRevealInFinder(contextMenuFor!)}
    >Reveal in Finder</button>
    <button
      data-testid="worktrunk-context-terminal"
      class="cursor-pointer rounded px-2 py-1 text-left text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-40"
      disabled={contextBusy}
      onclick={() => handleOpenTerminal(contextMenuFor!)}
    >Open in terminal</button>
    {#if contextError}
      <div
        data-testid="worktrunk-context-error"
        class="mt-1 border-t border-border-subtle/40 px-2 py-1 text-[10px] text-red"
      >
        {contextError}
      </div>
    {/if}
  </div>
{/if}

{#snippet logList(entries: WorktrunkLogEntry[])}
  <div class="p-3">
    {#if entries.length === 0}
      <div
        class="rounded border border-border-subtle bg-bg-surface/30 p-3 text-sm text-text-muted"
      >
        (none)
      </div>
    {:else}
      <ul class="flex flex-col">
        {#each entries as entry (entry.path)}
          <button
            data-testid="worktrunk-log-row"
            class="group flex items-center gap-2 border-b border-border-subtle/40 px-2 py-1.5 text-left hover:bg-bg-hover"
            onclick={() => openLogEntry(entry.path)}
          >
            <span class="flex-1 truncate font-mono text-[11px] text-text-primary"
              >{entry.file}</span
            >
            <span class="font-mono text-[10px] text-text-muted"
              >{formatSize(entry.size)}</span
            >
            <span class="font-mono text-[10px] text-text-muted"
              >{formatRelativeTime(entry.modifiedAt ?? null)}</span
            >
          </button>
        {/each}
      </ul>
    {/if}
  </div>
{/snippet}

{#snippet hookOutputList(entries: WorktrunkHookOutputEntry[])}
  <div class="p-3">
    {#if entries.length === 0}
      <div
        class="rounded border border-border-subtle bg-bg-surface/30 p-3 text-sm text-text-muted"
      >
        (none)
      </div>
    {:else}
      <ul class="flex flex-col">
        {#each entries as entry (entry.path)}
          <button
            data-testid="worktrunk-hook-output-row"
            class="group flex flex-col gap-0.5 border-b border-border-subtle/40 px-2 py-1.5 text-left hover:bg-bg-hover"
            onclick={() => openLogEntry(entry.path)}
          >
            <div class="flex items-center gap-2">
              <span
                class="rounded bg-accent-dim/20 px-1 text-[9px] font-semibold uppercase tracking-wider text-accent"
                >{entry.source}</span
              >
              {#if entry.hookType}
                <span class="font-mono text-[10px] text-text-muted"
                  >{entry.hookType}</span
                >
              {/if}
              <span class="font-mono text-[11px] text-text-primary"
                >{entry.name}</span
              >
              <span class="ml-auto font-mono text-[10px] text-text-muted"
                >{formatRelativeTime(entry.modifiedAt ?? null)}</span
              >
            </div>
            <div class="truncate font-mono text-[10px] text-text-muted">
              {entry.branch} · {formatSize(entry.size)}
            </div>
          </button>
        {/each}
      </ul>
    {/if}
  </div>
{/snippet}
