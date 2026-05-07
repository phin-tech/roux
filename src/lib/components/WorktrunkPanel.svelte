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
    sessionList,
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
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
  import Search from "@lucide/svelte/icons/search";
  import Trash from "@lucide/svelte/icons/trash";
  import X from "@lucide/svelte/icons/x";

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
  let headerMenuOpen = $state(false);
  let bulkMenuOpen = $state(false);
  let bulkCopiedFlash = $state(false);
  // Pending timer handle for the "Copied" flash. We clear and replace
  // it on each copy so a slow second click doesn't hide the new flash
  // because an earlier timer is still racing to fire.
  let bulkCopiedFlashTimer: ReturnType<typeof setTimeout> | null = null;
  let filterText = $state("");
  let selected = $state(new Set<string>());
  let selectAllCheckbox = $state<HTMLInputElement | null>(null);
  let bulkPending = $state(false);
  let bulkError = $state<string | null>(null);

  // Above this many selected items, "Reveal in Finder" / "Open in
  // terminal" prompt for confirmation — they spawn one external window
  // per worktree, and a stray select-all could carpet the desktop.
  const BULK_WINDOW_CONFIRM_THRESHOLD = 5;

  // Close the kebab menu on any click that isn't inside the currently
  // open row. `pointerdown` fires before `onclick`, so toggling the
  // kebab button on a different row still works: this handler closes
  // the old menu, then the button's own click opens the new one.
  $effect(() => {
    if (menuOpenFor == null) return;
    const openPath = menuOpenFor;
    const onPointerDown = (ev: PointerEvent) => {
      const target = ev.target;
      if (!(target instanceof Element)) return;
      const row = target.closest<HTMLElement>("[data-worktrunk-menu-root]");
      if (!row || row.dataset.worktrunkMenuRoot !== openPath) {
        menuOpenFor = null;
      }
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => window.removeEventListener("pointerdown", onPointerDown, true);
  });

  $effect(() => {
    if (!headerMenuOpen) return;
    const onPointerDown = (ev: PointerEvent) => {
      const target = ev.target;
      if (!(target instanceof Element)) return;
      if (!target.closest("[data-worktrunk-header-menu]")) {
        headerMenuOpen = false;
      }
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => window.removeEventListener("pointerdown", onPointerDown, true);
  });

  $effect(() => {
    if (!bulkMenuOpen) return;
    const onPointerDown = (ev: PointerEvent) => {
      const target = ev.target;
      if (!(target instanceof Element)) return;
      if (!target.closest("[data-worktrunk-bulk-menu]")) {
        bulkMenuOpen = false;
      }
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => window.removeEventListener("pointerdown", onPointerDown, true);
  });

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

  let currentRepo = $derived(visible ? ($activeSession?.repoRoot ?? null) : null);

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

  function clearRepoScopedState(): void {
    diagnostics = null;
    worktrees = [];
    error = null;
    worktreesError = null;
    readerPath = null;
    readerContent = null;
    readerError = null;
    loading = false;
    worktreesLoading = false;
    readerLoading = false;
    menuOpenFor = null;
    headerMenuOpen = false;
    bulkMenuOpen = false;
    bulkCopiedFlash = false;
    if (bulkCopiedFlashTimer != null) {
      clearTimeout(bulkCopiedFlashTimer);
      bulkCopiedFlashTimer = null;
    }
    filterText = "";
    selected = new Set();
    bulkPending = false;
    bulkError = null;
    contextMenuFor = null;
    contextBusy = false;
    contextError = null;
  }

  // Map of worktree path → first active (non-archived) session that
  // owns it. Used both to disable remove on rows with a running session
  // AND to pick between Focus / New-session buttons.
  let sessionByWorktreePath = $derived.by(() => {
    const map = new Map<string, Session>();
    if (!visible) return map;
    for (const s of $sessionList) {
      if (s.archived) continue;
      if (!map.has(s.worktreePath)) map.set(s.worktreePath, s);
    }
    return map;
  });
  let worktreePathsWithSession = $derived(
    new Set(sessionByWorktreePath.keys()),
  );

  function isRemovableWorktree(wt: Worktree): boolean {
    return !wt.isMain && !worktreePathsWithSession.has(wt.path);
  }

  let filteredWorktrees = $derived.by(() => {
    const q = filterText.trim().toLowerCase();
    if (!q) return worktrees;
    return worktrees.filter((wt) => {
      return (
        wt.branch.toLowerCase().includes(q) ||
        wt.path.toLowerCase().includes(q)
      );
    });
  });
  let visibleRemovableWorktrees = $derived(
    filteredWorktrees.filter((wt) => isRemovableWorktree(wt)),
  );
  let allVisibleSelected = $derived(
    visibleRemovableWorktrees.length > 0 &&
      visibleRemovableWorktrees.every((wt) => selected.has(wt.path)),
  );
  let someVisibleSelected = $derived(
    visibleRemovableWorktrees.some((wt) => selected.has(wt.path)) &&
      !allVisibleSelected,
  );
  let selectedWorktrees = $derived(
    worktrees.filter((wt) => selected.has(wt.path)),
  );
  let removableSelectedWorktrees = $derived(
    selectedWorktrees.filter((wt) => isRemovableWorktree(wt)),
  );
  let hasSelection = $derived(selected.size > 0);
  let visibleMergedWorktrees = $derived(
    visibleRemovableWorktrees.filter(
      (wt) => wt.worktrunk?.mainState === "integrated",
    ),
  );
  let visiblePrunableWorktrees = $derived(
    visibleRemovableWorktrees.filter((wt) => wt.worktrunk?.prunable === true),
  );

  $effect(() => {
    if (selectAllCheckbox) {
      selectAllCheckbox.indeterminate = someVisibleSelected;
    }
  });

  // The bulk toolbar (and its More menu) only render while
  // `hasSelection` is true. If the user clears the selection while the
  // menu is open, the toolbar unmounts but `bulkMenuOpen` stays true —
  // the next time a selection appears, the dropdown would render
  // already-open. Reset the menu/flash state alongside the selection.
  $effect(() => {
    if (selected.size === 0) {
      bulkMenuOpen = false;
      bulkCopiedFlash = false;
    }
  });

  $effect(() => {
    const removablePaths = new Set(
      worktrees.filter((wt) => isRemovableWorktree(wt)).map((wt) => wt.path),
    );
    let changed = false;
    const next = new Set<string>();
    for (const path of selected) {
      if (removablePaths.has(path)) {
        next.add(path);
      } else {
        changed = true;
      }
    }
    if (changed) selected = next;
  });

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
      if (profile)
        await runProfileInPane(newSession.id, profile, {
          smolMachineName: newSession.smolMachineName ?? null,
        });
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
    if (!visible) {
      clearRepoScopedState();
      return;
    }
    if (!currentRepo) {
      clearRepoScopedState();
      return;
    }
    const repo = currentRepo;
    clearRepoScopedState();
    void loadDiagnostics(repo);
    void loadWorktrees(repo);
  });

  function isCurrentRepoRequest(repoPath: string): boolean {
    return visible && currentRepo === repoPath;
  }

  async function loadDiagnostics(repoPath: string) {
    loading = true;
    error = null;
    try {
      const result = await commands.cmdWorktrunkDiagnostics(repoPath);
      if (!isCurrentRepoRequest(repoPath)) return;
      if (result.status === "ok") {
        diagnostics = result.data;
      } else {
        diagnostics = null;
        error = result.error;
      }
    } catch (err) {
      if (!isCurrentRepoRequest(repoPath)) return;
      diagnostics = null;
      error = typeof err === "string" ? err : String(err);
    } finally {
      if (isCurrentRepoRequest(repoPath) || !visible) loading = false;
    }
  }

  async function loadWorktrees(repoPath: string) {
    worktreesLoading = true;
    worktreesError = null;
    try {
      const entries = await listWorktrees(repoPath);
      if (!isCurrentRepoRequest(repoPath)) return;
      worktrees = entries;
      // Feed the shared store so chips on session cards pick up any
      // freshly-listed metadata too.
      upsertWorktreeMetadata(entries);
    } catch (err) {
      if (!isCurrentRepoRequest(repoPath)) return;
      worktrees = [];
      worktreesError = typeof err === "string" ? err : String(err);
    } finally {
      if (isCurrentRepoRequest(repoPath) || !visible) worktreesLoading = false;
    }
  }

  function toggleSelection(wt: Worktree) {
    if (!isRemovableWorktree(wt) || bulkPending) return;
    const next = new Set(selected);
    if (next.has(wt.path)) next.delete(wt.path);
    else next.add(wt.path);
    selected = next;
  }

  function clearSelection() {
    selected = new Set();
  }

  function toggleAllVisible() {
    if (bulkPending || visibleRemovableWorktrees.length === 0) return;
    const next = new Set(selected);
    if (allVisibleSelected) {
      for (const wt of visibleRemovableWorktrees) next.delete(wt.path);
    } else {
      for (const wt of visibleRemovableWorktrees) next.add(wt.path);
    }
    selected = next;
  }

  function addSelection(entries: Worktree[]) {
    if (bulkPending || entries.length === 0) return;
    const next = new Set(selected);
    for (const wt of entries) {
      if (isRemovableWorktree(wt)) next.add(wt.path);
    }
    selected = next;
    headerMenuOpen = false;
  }

  function describeBulkResult(
    verb: string,
    succeeded: number,
    failures: { branch: string; error: string }[],
  ): string | null {
    if (failures.length === 0) return null;
    const sample = failures[0];
    if (failures.length === 1 && succeeded === 0) {
      return `Failed to ${verb} ${sample.branch}: ${sample.error}`;
    }
    return `${verb}: ${succeeded} succeeded, ${failures.length} failed (e.g. ${sample.branch}: ${sample.error})`;
  }

  // Backend reports dirty worktrees as `WorktreeError::UncommittedChanges`,
  // whose Display impl starts with "worktree has uncommitted changes".
  // We match the substring so a wt-vs-git phrasing drift doesn't silently
  // re-enable the data-loss footgun (fall through to git --force).
  function isDirtyError(err: unknown): boolean {
    const msg = typeof err === "string" ? err : String(err);
    return /uncommitted changes/i.test(msg);
  }

  function formatDirtyBranchList(dirty: Worktree[]): string {
    const preview = dirty.slice(0, 5).map((wt) => `  • ${wt.branch}`);
    const more =
      dirty.length > 5 ? `\n  …and ${dirty.length - 5} more` : "";
    return `${preview.join("\n")}${more}`;
  }

  async function handleBulkRemove(alsoBranch: boolean) {
    if (!currentRepo || bulkPending || removableSelectedWorktrees.length === 0) {
      return;
    }
    const repo = currentRepo;
    const targets = [...removableSelectedWorktrees];
    const count = targets.length;
    // Close transient menus BEFORE the confirm so the dropdown isn't
    // left dangling behind the modal prompt (or after the user
    // cancels). The dropdown is non-interactive while the native
    // confirm is up, but visually it's still rendered and gets
    // dismissed only on next pointerdown — feels janky.
    menuOpenFor = null;
    headerMenuOpen = false;
    bulkMenuOpen = false;
    const confirmed = window.confirm(
      alsoBranch
        ? `Delete ${count} worktree${count === 1 ? "" : "s"} AND ${count} local branch${count === 1 ? "" : "es"}?\n\nBoth the on-disk worktrees and local branches will be deleted.`
        : `Delete ${count} worktree${count === 1 ? "" : "s"} on disk?\n\nThe branches stay in the repo.`,
    );
    if (!confirmed) return;
    bulkError = null;
    worktreesError = null;
    bulkPending = true;
    const succeeded: string[] = [];
    const dirty: Worktree[] = [];
    const failures: { branch: string; error: string }[] = [];
    try {
      for (const wt of targets) {
        try {
          await removeWorktree(repo, wt.path, alsoBranch, false);
          succeeded.push(wt.path);
        } catch (err) {
          if (isDirtyError(err)) {
            dirty.push(wt);
          } else {
            failures.push({
              branch: wt.branch,
              error: typeof err === "string" ? err : String(err),
            });
          }
        }
      }
      if (succeeded.length > 0 && isCurrentRepoRequest(repo)) {
        await loadWorktrees(repo);
      }
      if (!isCurrentRepoRequest(repo)) return;

      // Trim selection to what's left undeleted so the toolbar still
      // makes sense if the user dismisses the dirty prompt.
      {
        const remaining = new Set(selected);
        for (const path of succeeded) remaining.delete(path);
        selected = remaining;
      }
      bulkError = describeBulkResult(
        alsoBranch ? "remove worktrees + branches" : "remove worktrees",
        succeeded.length,
        failures,
      );

      if (dirty.length === 0) return;

      // Phase 2: review dirty worktrees and ask whether to force-delete.
      const forceConfirmed = window.confirm(
        `${dirty.length} worktree${dirty.length === 1 ? "" : "s"} ${dirty.length === 1 ? "has" : "have"} uncommitted changes:\n\n${formatDirtyBranchList(dirty)}\n\nForce-delete and DISCARD local changes?`,
      );
      if (!forceConfirmed) {
        const branches = dirty.map((wt) => wt.branch).join(", ");
        const skippedMsg = `${dirty.length} skipped (uncommitted changes): ${branches}`;
        bulkError = bulkError ? `${bulkError}. ${skippedMsg}` : skippedMsg;
        return;
      }

      const forceSucceeded: string[] = [];
      const forceFailures: { branch: string; error: string }[] = [];
      for (const wt of dirty) {
        try {
          await removeWorktree(repo, wt.path, alsoBranch, true);
          forceSucceeded.push(wt.path);
        } catch (err) {
          forceFailures.push({
            branch: wt.branch,
            error: typeof err === "string" ? err : String(err),
          });
        }
      }
      if (forceSucceeded.length > 0 && isCurrentRepoRequest(repo)) {
        await loadWorktrees(repo);
      }
      if (!isCurrentRepoRequest(repo)) return;
      {
        const remaining = new Set(selected);
        for (const path of forceSucceeded) remaining.delete(path);
        selected = remaining;
      }
      const forceBanner = describeBulkResult(
        "force-delete",
        forceSucceeded.length,
        forceFailures,
      );
      // Combine the two phases' banners so the user sees both outcomes.
      if (bulkError && forceBanner) {
        bulkError = `${bulkError}. ${forceBanner}`;
      } else if (forceBanner) {
        bulkError = forceBanner;
      } else if (failures.length === 0) {
        // Everything ultimately succeeded — clear the banner.
        bulkError = null;
      }
    } finally {
      bulkPending = false;
    }
  }

  async function handleBulkCopyPaths() {
    if (selectedWorktrees.length === 0 || bulkPending) return;
    const text = selectedWorktrees.map((wt) => wt.path).join("\n");
    bulkError = null;
    try {
      await navigator.clipboard.writeText(text);
      bulkMenuOpen = false;
      bulkCopiedFlash = true;
      if (bulkCopiedFlashTimer != null) clearTimeout(bulkCopiedFlashTimer);
      bulkCopiedFlashTimer = setTimeout(() => {
        bulkCopiedFlash = false;
        bulkCopiedFlashTimer = null;
      }, 1500);
    } catch (err) {
      const msg = typeof err === "string" ? err : String(err);
      bulkError = `Copy failed: ${msg}`;
    }
  }

  async function handleBulkRevealInFinder() {
    if (selectedWorktrees.length === 0 || bulkPending) return;
    const repo = currentRepo;
    if (!repo) return;
    const targets = [...selectedWorktrees];
    const count = targets.length;
    if (count > BULK_WINDOW_CONFIRM_THRESHOLD) {
      const confirmed = window.confirm(
        `Reveal ${count} worktrees in Finder?\n\nEach one opens a separate Finder window.`,
      );
      if (!confirmed) return;
    }
    bulkError = null;
    bulkMenuOpen = false;
    bulkPending = true;
    const failures: { branch: string; error: string }[] = [];
    let succeeded = 0;
    try {
      for (const wt of targets) {
        try {
          await revealItemInDir(wt.path);
          succeeded += 1;
        } catch (err) {
          failures.push({
            branch: wt.branch,
            error: typeof err === "string" ? err : String(err),
          });
        }
      }
      if (!isCurrentRepoRequest(repo)) return;
      bulkError = describeBulkResult("reveal", succeeded, failures);
    } finally {
      bulkPending = false;
    }
  }

  async function handleBulkOpenTerminal() {
    if (selectedWorktrees.length === 0 || bulkPending) return;
    const repo = currentRepo;
    if (!repo) return;
    const targets = [...selectedWorktrees];
    const count = targets.length;
    if (count > BULK_WINDOW_CONFIRM_THRESHOLD) {
      const confirmed = window.confirm(
        `Open ${count} worktrees in terminal?\n\nEach one opens a separate terminal window.`,
      );
      if (!confirmed) return;
    }
    bulkError = null;
    bulkMenuOpen = false;
    bulkPending = true;
    const failures: { branch: string; error: string }[] = [];
    let succeeded = 0;
    try {
      for (const wt of targets) {
        try {
          const res = await commands.cmdOpenTerminalAt(wt.path);
          if (res.status === "error") {
            failures.push({ branch: wt.branch, error: res.error });
          } else {
            succeeded += 1;
          }
        } catch (err) {
          failures.push({
            branch: wt.branch,
            error: typeof err === "string" ? err : String(err),
          });
        }
      }
      if (!isCurrentRepoRequest(repo)) return;
      bulkError = describeBulkResult("open in terminal", succeeded, failures);
    } finally {
      bulkPending = false;
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
    bulkError = null;
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
    const repo = currentRepo;
    removing = wt.path;
    try {
      try {
        await removeWorktree(repo, wt.path, alsoBranch, false);
      } catch (err) {
        if (!isDirtyError(err)) throw err;
        const force = window.confirm(
          `"${wt.branch}" has uncommitted changes.\n\nForce-delete and DISCARD local changes?`,
        );
        if (!force) {
          if (!isCurrentRepoRequest(repo)) return;
          worktreesError = `Skipped ${wt.branch}: uncommitted changes`;
          return;
        }
        await removeWorktree(repo, wt.path, alsoBranch, true);
      }
      if (!isCurrentRepoRequest(repo)) return;
      const next = new Set(selected);
      next.delete(wt.path);
      selected = next;
      await loadWorktrees(repo);
    } catch (err) {
      if (!isCurrentRepoRequest(repo)) return;
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
      if (!currentRepo) {
        readerError = "No active repo; cannot read worktrunk log.";
        return;
      }
      const result = await commands.cmdWorktrunkReadLog(currentRepo, path);
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
      <div class="flex flex-1 min-h-0 flex-col overflow-hidden">
        <div
          class="relative flex shrink-0 items-center gap-2 px-2 py-1.5"
          data-worktrunk-header-menu
        >
          <span class="text-[10px] uppercase tracking-wider text-text-muted"
            >{#if filterText.trim()}{filteredWorktrees.length} of {/if}{worktrees.length}
            {worktrees.length === 1 ? "worktree" : "worktrees"}</span
          >
          <div class="ml-auto flex items-center gap-1">
            <button
              type="button"
              class="inline-flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded text-text-secondary transition-colors duration-150 hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-not-allowed disabled:opacity-40"
              title="More worktree selection actions"
              aria-label="More worktree selection actions"
              data-testid="worktrunk-worktrees-header-menu"
              disabled={worktrees.length === 0 || bulkPending}
              onclick={() => {
                menuOpenFor = null;
                headerMenuOpen = !headerMenuOpen;
              }}
            >
              <MoreHorizontal size={13} />
            </button>
            {#if headerMenuOpen}
              <div
                class="absolute right-28 top-8 z-20 flex min-w-44 flex-col rounded border border-border bg-bg-elevated p-1 shadow-lg"
                data-testid="worktrunk-worktrees-header-menu-content"
              >
                <button
                  type="button"
                  data-testid="worktrunk-select-merged"
                  class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-bg-hover disabled:opacity-40"
                  disabled={visibleMergedWorktrees.length === 0 || bulkPending}
                  title={visibleMergedWorktrees.length === 0
                    ? "No visible merged worktrees can be selected"
                    : `Select ${visibleMergedWorktrees.length} visible merged worktree${visibleMergedWorktrees.length === 1 ? "" : "s"}`}
                  onclick={() => addSelection(visibleMergedWorktrees)}
                >
                  <GitBranch size={12} />
                  <span>Select merged</span>
                  {#if visibleMergedWorktrees.length > 0}
                    <span class="ml-auto text-[10px] text-text-muted"
                      >{visibleMergedWorktrees.length}</span
                    >
                  {/if}
                </button>
                <button
                  type="button"
                  data-testid="worktrunk-select-prunable"
                  class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-bg-hover disabled:opacity-40"
                  disabled={visiblePrunableWorktrees.length === 0 || bulkPending}
                  title={visiblePrunableWorktrees.length === 0
                    ? "No visible prunable worktrees can be selected"
                    : `Select ${visiblePrunableWorktrees.length} visible prunable worktree${visiblePrunableWorktrees.length === 1 ? "" : "s"}`}
                  onclick={() => addSelection(visiblePrunableWorktrees)}
                >
                  <Trash size={12} />
                  <span>Select prunable</span>
                  {#if visiblePrunableWorktrees.length > 0}
                    <span class="ml-auto text-[10px] text-text-muted"
                      >{visiblePrunableWorktrees.length}</span
                    >
                  {/if}
                </button>
              </div>
            {/if}
            <button
              type="button"
              data-testid="worktrunk-new-worktree-open"
              class="inline-flex h-6 cursor-pointer items-center rounded border border-border-subtle bg-bg-elevated px-2 text-[10px] text-text-primary transition-colors duration-150 hover:border-accent hover:text-accent disabled:cursor-not-allowed disabled:opacity-40"
              disabled={bulkPending}
              onclick={() => (newFormOpen = !newFormOpen)}
            >{newFormOpen ? "Cancel" : "+ New worktree"}</button>
          </div>
        </div>

        <div class="px-2 pb-1">
          <div class="relative">
            <Search
              size={11}
              class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-text-muted"
            />
            <input
              type="text"
              class="w-full rounded border border-border bg-bg-deep py-1 pl-6 pr-6 text-[11px] text-text-primary placeholder:text-text-muted outline-none focus:border-accent-dim"
              placeholder="Filter worktrees…"
              bind:value={filterText}
              data-testid="worktrunk-filter-input"
            />
            {#if filterText}
              <button
                type="button"
                class="absolute right-1 top-1/2 inline-flex h-4 w-4 -translate-y-1/2 cursor-pointer items-center justify-center rounded text-text-muted hover:bg-bg-hover hover:text-text-primary"
                aria-label="Clear filter"
                title="Clear filter"
                data-testid="worktrunk-filter-clear"
                onclick={() => (filterText = "")}
              >
                <X size={11} />
              </button>
            {/if}
          </div>
        </div>

        {#if newFormOpen}
          <div
            data-testid="worktrunk-new-worktree-form"
            class="mx-2 mb-2 rounded border border-accent-dim/40 bg-bg-surface/40 p-2"
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

        {#if hasSelection}
          <div
            class="relative mx-2 mb-1 flex flex-wrap items-center gap-1 rounded border border-accent-dim/40 bg-accent-dim/10 px-2 py-1 text-[10px] text-text-secondary"
            data-testid="worktrunk-bulk-toolbar"
            data-worktrunk-bulk-menu
          >
            <span class="text-text-primary">{selected.size} selected</span>
            {#if bulkCopiedFlash}
              <span
                class="text-accent"
                data-testid="worktrunk-bulk-copied-flash"
                aria-live="polite"
              >Copied</span>
            {/if}
            <button
              type="button"
              data-testid="worktrunk-bulk-more"
              class="ml-auto inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded text-text-secondary transition-colors duration-150 hover:bg-bg-hover hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-40"
              title="More bulk actions"
              aria-label="More bulk actions"
              disabled={bulkPending}
              onclick={() => (bulkMenuOpen = !bulkMenuOpen)}
            >
              <MoreHorizontal size={11} />
            </button>
            {#if bulkMenuOpen}
              <div
                class="absolute right-2 top-7 z-20 flex min-w-44 flex-col rounded border border-border bg-bg-elevated p-1 shadow-lg"
                data-testid="worktrunk-bulk-menu-content"
              >
                <button
                  type="button"
                  data-testid="worktrunk-bulk-copy-paths"
                  class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-bg-hover disabled:opacity-40"
                  disabled={bulkPending}
                  onclick={handleBulkCopyPaths}
                  title={`Copy ${selected.size} path${selected.size === 1 ? "" : "s"} to clipboard`}
                >
                  <span>Copy paths</span>
                  <span class="ml-auto text-[10px] text-text-muted"
                    >{selected.size}</span
                  >
                </button>
                <button
                  type="button"
                  data-testid="worktrunk-bulk-reveal"
                  class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-bg-hover disabled:opacity-40"
                  disabled={bulkPending}
                  onclick={handleBulkRevealInFinder}
                  title={`Reveal ${selected.size} worktree${selected.size === 1 ? "" : "s"} in Finder`}
                >
                  <span>Reveal in Finder</span>
                  <span class="ml-auto text-[10px] text-text-muted"
                    >{selected.size}</span
                  >
                </button>
                <button
                  type="button"
                  data-testid="worktrunk-bulk-open-terminal"
                  class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary enabled:cursor-pointer enabled:hover:bg-bg-hover disabled:opacity-40"
                  disabled={bulkPending}
                  onclick={handleBulkOpenTerminal}
                  title={`Open ${selected.size} worktree${selected.size === 1 ? "" : "s"} in terminal`}
                >
                  <span>Open in terminal</span>
                  <span class="ml-auto text-[10px] text-text-muted"
                    >{selected.size}</span
                  >
                </button>
              </div>
            {/if}
            <button
              type="button"
              data-testid="worktrunk-bulk-remove"
              class="inline-flex h-5 cursor-pointer items-center gap-1 rounded border border-border-subtle bg-bg-elevated px-1.5 text-[10px] text-text-secondary transition-colors duration-150 hover:bg-amber/20 hover:text-amber disabled:cursor-not-allowed disabled:opacity-40"
              disabled={removableSelectedWorktrees.length === 0 || bulkPending}
              title={removableSelectedWorktrees.length === 0
                ? "No selected worktrees can be removed"
                : `Remove ${removableSelectedWorktrees.length} selected worktree${removableSelectedWorktrees.length === 1 ? "" : "s"} on disk`}
              onclick={() => handleBulkRemove(false)}
            >
              <Trash size={10} />
              <span>Delete</span>
            </button>
            <button
              type="button"
              data-testid="worktrunk-bulk-remove-and-branch"
              class="inline-flex h-5 cursor-pointer items-center gap-1 rounded border border-border-subtle bg-bg-elevated px-1.5 text-[10px] text-text-secondary transition-colors duration-150 hover:bg-red/20 hover:text-red disabled:cursor-not-allowed disabled:opacity-40"
              disabled={removableSelectedWorktrees.length === 0 || bulkPending}
              title={removableSelectedWorktrees.length === 0
                ? "No selected worktrees can be removed"
                : `Remove ${removableSelectedWorktrees.length} selected worktree${removableSelectedWorktrees.length === 1 ? "" : "s"} and local branch${removableSelectedWorktrees.length === 1 ? "" : "es"}`}
              onclick={() => handleBulkRemove(true)}
            >
              <GitBranch size={10} />
              <span>Delete + branch</span>
            </button>
            <button
              type="button"
              data-testid="worktrunk-bulk-clear"
              class="inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded text-text-muted hover:bg-bg-hover hover:text-text-primary"
              title="Clear selection"
              aria-label="Clear selection"
              onclick={clearSelection}
            >
              <X size={11} />
            </button>
          </div>
        {/if}

        <div class="app-scrollbar min-h-0 flex-1 overflow-y-auto px-1 pb-2">
          {#if worktreesLoading && worktrees.length === 0}
            <div class="px-2 py-1 text-[11px] text-text-muted">Loading…</div>
          {:else if worktreesError}
            <div
              data-testid="worktrunk-worktrees-error"
              class="mb-1 flex items-center gap-2 border border-red/30 bg-red/10 px-2 py-1 text-[11px] text-red"
            >
              <span class="min-w-0 flex-1 truncate">{worktreesError}</span>
              <button
                type="button"
                class="flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center text-red/80 hover:text-red focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/50"
                aria-label="Dismiss worktree error"
                title="Dismiss"
                onclick={() => (worktreesError = null)}
              >
                <X size={11} />
              </button>
            </div>
          {/if}
          {#if bulkError}
            <div class="mb-1 flex items-center gap-2 border border-red/30 bg-red/10 px-2 py-1 text-[11px] text-red">
              <span
                class="min-w-0 flex-1 truncate"
                data-testid="worktrunk-bulk-error">{bulkError}</span
              >
              <button
                type="button"
                class="flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center text-red/80 hover:text-red focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/50"
                aria-label="Dismiss bulk action error"
                title="Dismiss"
                onclick={() => (bulkError = null)}
              >
                <X size={11} />
              </button>
            </div>
          {/if}
          {#if worktrees.length === 0 && !worktreesLoading}
            <div class="px-2 py-1 text-[11px] text-text-muted">
              No worktrees.
            </div>
          {:else if filteredWorktrees.length === 0}
            <div
              class="px-2 py-1 text-[11px] text-text-muted"
              data-testid="worktrunk-filter-empty"
            >
              No worktrees match "{filterText}".
            </div>
          {:else}
            <label
              class="mb-1 flex cursor-pointer items-center gap-2 px-2 py-1 text-[10px] text-text-muted hover:text-text-secondary has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-50"
            >
              <input
                type="checkbox"
                class="h-3 w-3 cursor-pointer rounded border border-border bg-bg-deep accent-accent disabled:cursor-not-allowed"
                bind:this={selectAllCheckbox}
                checked={allVisibleSelected}
                disabled={visibleRemovableWorktrees.length === 0 || bulkPending}
                onchange={toggleAllVisible}
                data-testid="worktrunk-select-all"
              />
              <span>
                {#if filterText}
                  Select {visibleRemovableWorktrees.length} removable match{visibleRemovableWorktrees.length === 1 ? "" : "es"}
                {:else}
                  Select removable
                {/if}
              </span>
            </label>
            {#each filteredWorktrees as wt (wt.path)}
              {@const session = sessionByWorktreePath.get(wt.path)}
              {@const hasSession = session !== undefined}
              {@const cannotRemove = wt.isMain || hasSession}
              {@const isRemoving = removing === wt.path}
              {@const isSpawning = spawning === wt.path}
              {@const isSelected = selected.has(wt.path)}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                data-testid="worktrunk-worktree-row"
                data-worktrunk-menu-root={wt.path}
                class="group relative mb-1 border px-2 py-1.5 text-left text-sm transition-colors duration-150 {isSelected
                  ? 'border-accent-dim/60 bg-accent-dim/10'
                  : 'border-transparent hover:border-border-subtle hover:bg-bg-active/40 focus-within:border-border-subtle focus-within:bg-bg-active/40'}"
                oncontextmenu={(e) => openContextMenu(e, wt)}
              >
                <div class="flex min-h-6 items-center gap-2">
                  <input
                    type="checkbox"
                    class="h-3 w-3 shrink-0 cursor-pointer rounded border border-border bg-bg-deep accent-accent disabled:cursor-not-allowed disabled:opacity-40"
                    checked={isSelected}
                    disabled={cannotRemove || bulkPending}
                    onchange={() => toggleSelection(wt)}
                    aria-label={`Select ${wt.branch}`}
                    title={wt.isMain
                      ? "Cannot remove the main worktree"
                      : hasSession
                        ? "A Roux session is active — close it first"
                        : `Select ${wt.branch}`}
                    data-testid="worktrunk-row-checkbox"
                  />
                  <div class="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
                    <WorktreeRowContent
                      {wt}
                      showPath={false}
                      repoRoot={currentRepo}
                      session={session ?? null}
                    />
                  </div>
                  <div class="flex shrink-0 items-center gap-1">
                    {#if hasSession}
                      <button
                        type="button"
                        data-testid="worktrunk-worktree-focus"
                        class="inline-flex h-6 cursor-pointer items-center rounded border border-accent-dim/40 bg-accent-dim/20 px-2 text-[10px] text-accent hover:bg-accent-dim/40"
                        onclick={() => handleFocus(session!)}
                        title={`Focus the "${session!.name}" session`}
                      >Focus</button>
                    {:else}
                      <button
                        type="button"
                        data-testid="worktrunk-worktree-new-session"
                        class="inline-flex h-6 items-center rounded border border-border-subtle bg-bg-elevated px-2 text-[10px] text-text-secondary
                          enabled:cursor-pointer enabled:hover:border-accent enabled:hover:text-accent
                          disabled:opacity-40"
                        disabled={isSpawning || bulkPending}
                        onclick={() => handleNewSessionHere(wt)}
                        title="Start a new Claude session in this worktree"
                      >{isSpawning ? "Starting…" : "New session"}</button>
                    {/if}
                    <button
                      type="button"
                      data-testid="worktrunk-worktree-menu"
                      class="inline-flex h-6 w-6 items-center justify-center rounded border border-border-subtle bg-bg-elevated text-text-secondary
                        enabled:cursor-pointer enabled:hover:bg-bg-hover
                        disabled:opacity-40"
                      disabled={isRemoving || isSpawning || bulkPending}
                      onclick={() =>
                        (menuOpenFor = menuOpenFor === wt.path ? null : wt.path)}
                      aria-label="More actions"
                    >
                      <MoreHorizontal size={13} />
                    </button>
                  </div>
                </div>
                <div class="ml-5 mt-0.5 flex min-h-5 items-center gap-1.5 overflow-hidden text-[10px] text-text-muted">
                  <span class="min-w-0 truncate" title={wt.path}>{wt.path}</span>
                  {#if hasSession}
                    <span
                      class="shrink-0 rounded bg-accent-dim/15 px-1 py-0.5 text-accent"
                      title={`Roux session "${session!.name}" is active in this worktree`}
                    >active session</span>
                  {/if}
                </div>
                {#if menuOpenFor === wt.path}
                  <div
                    data-testid="worktrunk-worktree-menu-content"
                    class="absolute right-2 top-8 z-10 flex min-w-40 flex-col rounded border border-border bg-bg-elevated p-1 shadow-lg"
                  >
                    <button
                      type="button"
                      data-testid="worktrunk-worktree-remove"
                      class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary
                        enabled:cursor-pointer enabled:hover:bg-red/20
                        disabled:opacity-40"
                      disabled={cannotRemove}
                      onclick={() => handleRemove(wt, false)}
                      title={wt.isMain
                        ? "Cannot remove the main worktree"
                        : hasSession
                          ? "A Roux session is active — close it first"
                          : "Remove the worktree on disk (keep the branch)"}
                    >
                      <Trash size={12} />
                      <span>Remove worktree</span>
                    </button>
                    <button
                      type="button"
                      data-testid="worktrunk-worktree-remove-and-branch"
                      class="flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] text-text-primary
                        enabled:cursor-pointer enabled:hover:bg-red/20
                        disabled:opacity-40"
                      disabled={cannotRemove}
                      onclick={() => handleRemove(wt, true)}
                    >
                      <GitBranch size={12} />
                      <span>Remove worktree + branch</span>
                    </button>
                  </div>
                {/if}
              </div>
            {/each}
          {/if}
        </div>
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
                  rel="noopener noreferrer"
                  class="text-blue underline"
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
                  rel="noopener noreferrer"
                  class="text-blue underline"
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
                        <span class="text-xs text-text-primary"
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
    <div class="border-b border-border-subtle/40 px-2 py-1 text-[10px] text-text-muted">
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
            <span class="text-[10px] text-text-muted"
              >{formatSize(entry.size)}</span
            >
            <span class="text-[10px] text-text-muted"
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
                <span class="text-[10px] text-text-muted"
                  >{entry.hookType}</span
                >
              {/if}
              <span class="text-[11px] text-text-primary"
                >{entry.name}</span
              >
              <span class="ml-auto text-[10px] text-text-muted"
                >{formatRelativeTime(entry.modifiedAt ?? null)}</span
              >
            </div>
            <div class="truncate text-[10px] text-text-muted">
              {entry.branch} · {formatSize(entry.size)}
            </div>
          </button>
        {/each}
      </ul>
    {/if}
  </div>
{/snippet}
