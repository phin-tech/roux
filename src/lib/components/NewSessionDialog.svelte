<script lang="ts">
  import { Command } from "bits-ui";
  import { tick } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    createSessionShell,
    listWorktrees,
    checkNonoInstalled,
    listNonoProfiles,
    checkIsGitRepo,
    gitInit,
    killSession,
  } from "$lib/tauri";
  import { addSession, removeSession } from "$lib/stores/sessions";
  import { layoutList, type LayoutSpec } from "$lib/panes/layouts";
  import { applyLayoutToSession, resolveFirstLeafNono, type LayoutApplyError } from "$lib/panes/layoutRunner";
  import { initSessionWithProfile } from "$lib/panes/actions";
  import { settings } from "$lib/stores/settings";
  import {
    profileList,
    type SpawnProfile,
    type SpawnProfileRef,
  } from "$lib/panes/profiles";
  import { runProfileInPane } from "$lib/panes/profileRunner";
  import { estimatePaneSize } from "$lib/panes/estimatePaneSize";
  import type { Worktree } from "$lib/types";
  import { log, logError } from "$lib/logging";
  import ProfileCustomEditor from "./ProfileCustomEditor.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();

  let repoPath = $state($settings.defaultProjectPath ?? "");
  let isGitRepo = $state(false);
  let sessionName = $state("");
  let worktrees = $state<Worktree[]>([]);
  let worktreeFilterInput = $state("");
  let worktreePickOpen = $state(true);
  let worktreeActiveIndex = $state(0);
  let selectedWorktree = $state<Worktree | null>(null);
  let error = $state("");
  let creating = $state(false);
  let rootRepoPaths = $state<string[]>([]);
  let rootReposLoading = $state(false);
  let rootReposError = $state("");
  let repoPickerOpen = $state(true);
  let layoutPickInput = $state("");
  let layoutPickOpen = $state(false);
  let profilePickInput = $state("");
  let profilePickOpen = $state(false);
  let nonoPickInput = $state("");
  let nonoPickOpen = $state(false);
  const pickerShellClass = "min-w-0 overflow-hidden rounded-md border border-border bg-bg-deep";
  const pickerInputRowClass = "flex min-w-0 items-center gap-2 border-b border-border px-2 py-1.5";
  const pickerInputClass =
    "min-w-0 flex-1 bg-transparent px-1 py-1 font-mono text-[12px] text-text-primary outline-none placeholder:text-text-muted";
  const pickerListClass = "app-scrollbar overflow-y-auto p-1";
  const pickerItemClass =
    "flex cursor-pointer items-center rounded-md border border-border-subtle bg-bg-surface/50 px-2.5 py-2 text-left transition-colors hover:bg-bg-hover";
  let quickPickOptions = $derived.by<{ label: string; path: string }[]>(() =>
    buildQuickPickOptions(rootRepoPaths),
  );
  let filteredWorktrees = $derived.by<Worktree[]>(() => {
    const q = worktreeFilterInput.trim().toLowerCase();
    if (!q) return worktrees;
    return worktrees.filter(
      (wt) =>
        wt.branch.toLowerCase().includes(q) ||
        wt.path.toLowerCase().includes(q),
    );
  });
  let layoutOptions = $derived.by<{ value: string; label: string }[]>(() => [
    { value: "", label: "None (single pane)" },
    ...$layoutList.map((layout) => ({ value: layout.id, label: layout.name })),
  ]);
  let profileOptions = $derived.by<{ value: string; label: string }[]>(() => {
    const options = $profileList.map((profile) => ({
      value: profile.id,
      label: `${profile.name}${profile.source === "user" ? " (user)" : ""}`,
    }));
    if (inlineProfile) {
      options.push({ value: "__inline__", label: `${inlineProfile.name} (custom)` });
    }
    options.push({ value: "__custom__", label: "Custom…" });
    return options;
  });
  let nonoOptions = $derived.by<{ value: string; label: string }[]>(() => [
    { value: "", label: "None (bare claude)" },
    ...nonoProfiles.map((profile) => ({ value: profile, label: profile })),
  ]);

  $effect(() => {
    const len = filteredWorktrees.length;
    if (len === 0) {
      worktreeActiveIndex = 0;
      return;
    }
    if (worktreeActiveIndex < 0) worktreeActiveIndex = 0;
    if (worktreeActiveIndex >= len) worktreeActiveIndex = len - 1;
  });

  $effect(() => {
    if (!isGitRepo) return;
    if (filteredWorktrees.length === 0) {
      selectedWorktree = null;
      return;
    }
    if (!selectedWorktree || !filteredWorktrees.some((wt) => wt.path === selectedWorktree?.path)) {
      selectedWorktree = filteredWorktrees[Math.min(worktreeActiveIndex, filteredWorktrees.length - 1)] ?? null;
    }
  });

  // Spawn profile selection. Defaults to the claude built-in so first-time
  // users see familiar behavior. An inline profile from the Custom… editor
  // sets `inlineProfile` and picks a synthetic id ("__inline__").
  let selectedProfileId = $state<string>("claude");
  let inlineProfile = $state<SpawnProfile | null>(null);
  let showCustomEditor = $state(false);

  // Layout selection
  let selectedLayoutId = $state<string>("");
  let selectedLayout = $derived.by<LayoutSpec | null>(() => {
    if (!selectedLayoutId) return null;
    return $layoutList.find((l) => l.id === selectedLayoutId) ?? null;
  });

  // Resolve the currently-selected profile object. Built-in / user profiles
  // come from the registry; inline ones from local state.
  let selectedProfile = $derived.by<SpawnProfile | null>(() => {
    if (selectedProfileId === "__inline__") return inlineProfile;
    return $profileList.find((p) => p.id === selectedProfileId) ?? null;
  });

  // Nono sandbox integration
  let nonoInstalled = $state(false);
  let nonoProfiles = $state<string[]>([]);
  let selectedNonoProfile = $state<string | null>(null);
  const compatSettings = $derived.by<{
    repoRoots: string[];
    excludeWorktreesFromRepoRoots: boolean;
  }>(() => {
    const raw = $settings as unknown as {
      repoRoots?: string[];
      excludeWorktreesFromRepoRoots?: boolean;
    };
    return {
      repoRoots: Array.isArray(raw.repoRoots) ? raw.repoRoots : [],
      excludeWorktreesFromRepoRoots:
        typeof raw.excludeWorktreesFromRepoRoots === "boolean"
          ? raw.excludeWorktreesFromRepoRoots
          : true,
    };
  });

  // Check for nono on mount and detect git repo for default path
  $effect(() => {
    if (visible) {
      checkNonoInstalled().then((installed) => {
        nonoInstalled = installed;
        if (installed) {
          listNonoProfiles().then((profiles) => {
            nonoProfiles = profiles;
          });
        }
      });
      if (repoPath) {
        detectGitRepo(repoPath);
      }
    }
  });

  $effect(() => {
    const rootsKey = compatSettings.repoRoots.join("\n");
    const excludeWorktrees = compatSettings.excludeWorktreesFromRepoRoots;
    if (!visible) return;
    void rootsKey;
    void excludeWorktrees;
    loadRootRepoOptions();
  });

  $effect(() => {
    if (!visible) return;
    void autofocusOnOpen();
  });

  $effect(() => {
    if (!visible) return;
    if (!layoutPickOpen) {
      layoutPickInput = selectedLayout?.name ?? "None (single pane)";
    }
    if (!profilePickOpen) {
      if (selectedProfileId === "__inline__" && inlineProfile) {
        profilePickInput = `${inlineProfile.name} (custom)`;
      } else {
        const selected = $profileList.find((p) => p.id === selectedProfileId);
        profilePickInput = selected
          ? `${selected.name}${selected.source === "user" ? " (user)" : ""}`
          : "Custom…";
      }
    }
    if (!nonoPickOpen) {
      nonoPickInput = selectedNonoProfile ?? "None (bare claude)";
    }
  });

  async function autofocusOnOpen() {
    await tick();
    const quickPickEl = document.getElementById("new-session-repo-picker") as HTMLInputElement | null;
    if (quickPickEl) {
      quickPickEl.focus();
      quickPickEl.select();
      return;
    }
    focusDirectoryInput();
  }

  async function detectGitRepo(path: string) {
    isGitRepo = await checkIsGitRepo(path);
    if (isGitRepo) {
      await loadWorktrees();
    } else {
      worktrees = [];
      selectedWorktree = null;
      worktreeFilterInput = "";
    }
  }

  async function pickRepo() {
    const selected = await open({ directory: true, title: "Select Directory" });
    if (selected) {
      repoPath = selected as string;
      await detectGitRepo(repoPath);
    }
  }

  async function loadWorktrees() {
    if (!repoPath) return;
    try {
      worktrees = await listWorktrees(repoPath);
      worktreePickOpen = true;
      worktreeActiveIndex = 0;
      selectedWorktree = worktrees.find((w) => w.isMain) ?? worktrees[0] ?? null;
    } catch {
      worktrees = [];
    }
  }

  async function loadRootRepoOptions() {
    const roots = compatSettings.repoRoots.map((r) => r.trim()).filter(Boolean);
    const excludeWorktrees = compatSettings.excludeWorktreesFromRepoRoots;
    if (roots.length === 0) {
      rootRepoPaths = [];
      rootReposError = "";
      return;
    }
    rootReposLoading = true;
    rootReposError = "";
    try {
      rootRepoPaths = await invoke<string[]>("list_git_repos_in_roots", {
        roots,
        excludeWorktrees,
      });
    } catch (e) {
      rootRepoPaths = [];
      rootReposError = String(e);
    } finally {
      rootReposLoading = false;
    }
  }

  async function pickRepoFromRoots(path: string) {
    if (!path) return;
    repoPath = path;
    await detectGitRepo(path);
  }

  function findQuickPickMatch(queryRaw: string): { label: string; path: string } | null {
    const query = queryRaw.trim();
    if (!query) return null;
    const lower = query.toLowerCase();
    const exactPath = quickPickOptions.find((o) => o.path === query);
    if (exactPath) return exactPath;
    const exactLabel = quickPickOptions.find((o) => o.label.toLowerCase() === lower);
    if (exactLabel) return exactLabel;
    return (
      quickPickOptions.find(
        (o) => o.label.toLowerCase().includes(lower) || o.path.toLowerCase().includes(lower),
      ) ?? null
    );
  }

  function focusDirectoryInput() {
    const inputEl = document.getElementById("new-session-repo-picker") as HTMLInputElement | null;
    inputEl?.focus();
    inputEl?.select();
  }

  async function selectQuickPick(path: string, label?: string) {
    await pickRepoFromRoots(path);
    if (label) repoPath = path;
    repoPickerOpen = false;
    focusDirectoryInput();
  }

  function findOptionMatch(
    queryRaw: string,
    options: { value: string; label: string }[],
  ): { value: string; label: string } | null {
    const query = queryRaw.trim();
    if (!query) return options[0] ?? null;
    const lower = query.toLowerCase();
    const exactValue = options.find((o) => o.value === query);
    if (exactValue) return exactValue;
    const exactLabel = options.find((o) => o.label.toLowerCase() === lower);
    if (exactLabel) return exactLabel;
    return options.find((o) => o.label.toLowerCase().includes(lower)) ?? null;
  }

  function selectLayoutOption(value: string, label: string) {
    selectedLayoutId = value;
    layoutPickInput = label;
    layoutPickOpen = false;
  }

  function selectProfileOption(value: string, label: string) {
    handleProfileSelect(value);
    profilePickInput = label;
    profilePickOpen = false;
  }

  function selectNonoOption(value: string, label: string) {
    selectedNonoProfile = value === "" ? null : value;
    nonoPickInput = label;
    nonoPickOpen = false;
  }

  function handleDialogKeydown(e: KeyboardEvent) {
    if (!visible || showCustomEditor) return;
    if (e.key === "Escape") {
      e.preventDefault();
      resetAndClose();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      if (!creating) void handleCreate();
    }
  }

  function moveWorktreeActive(delta: number) {
    if (filteredWorktrees.length === 0) return;
    const next = Math.max(0, Math.min(filteredWorktrees.length - 1, worktreeActiveIndex + delta));
    worktreeActiveIndex = next;
    selectedWorktree = filteredWorktrees[next] ?? null;
  }

  function selectActiveWorktree() {
    if (filteredWorktrees.length === 0) return;
    selectedWorktree = filteredWorktrees[worktreeActiveIndex] ?? null;
    if (selectedWorktree) worktreeFilterInput = selectedWorktree.branch;
    worktreePickOpen = false;
  }

  function resolveGitTarget(): {
    worktreePathArg: string | null;
    branchArg: string | null;
    label: string;
  } {
    const query = worktreeFilterInput.trim();
    const exact =
      worktrees.find((wt) => wt.path === query || wt.branch === query) ??
      (query.length === 0 ? selectedWorktree : null);
    if (exact) {
      return { worktreePathArg: exact.path, branchArg: null, label: exact.branch };
    }
    if (!query) {
      return { worktreePathArg: null, branchArg: null, label: "main" };
    }
    return { worktreePathArg: null, branchArg: query, label: query };
  }

  function formatRepoShortLabel(path: string, depth: number = 2): string {
    const normalized = path.replaceAll("\\", "/");
    const segments = normalized.split("/").filter(Boolean);
    if (segments.length === 0) return path;
    if (segments.length === 1) return segments[0];
    return segments.slice(Math.max(segments.length - depth, 0)).join("/");
  }

  function buildQuickPickOptions(paths: string[]): { label: string; path: string }[] {
    const firstPass = paths.map((path) => ({ path, label: formatRepoShortLabel(path, 2) }));
    const counts = new Map<string, number>();
    for (const item of firstPass) {
      counts.set(item.label, (counts.get(item.label) ?? 0) + 1);
    }
    return firstPass.map((item) => {
      if ((counts.get(item.label) ?? 0) === 1) return item;
      const deeper = formatRepoShortLabel(item.path, 3);
      return deeper === item.label ? { ...item, label: item.path } : { ...item, label: deeper };
    });
  }

  async function handleCreate() {
    if (!repoPath) {
      error = "Please select a directory";
      return;
    }
    if (!selectedLayout && !selectedProfile) {
      error = "Pick a spawn profile (or use Custom…).";
      return;
    }
    error = "";
    creating = true;

    try {
      const gitTarget = isGitRepo ? resolveGitTarget() : null;
      const name =
        sessionName ||
        (isGitRepo
          ? `${repoPath.split("/").pop() ?? "session"}-${gitTarget?.label ?? "main"}`
          : repoPath.split("/").pop() ?? "session");

      const worktreePathArg = gitTarget?.worktreePathArg ?? null;
      const branchArg = gitTarget?.branchArg ?? null;

      // Best-effort estimate of the pane's cell size so the backend spawns
      // the PTY at roughly the right dimensions. Eliminates the 80-col →
      // actual SIGWINCH that triggers `zle reset-prompt` and causes async
      // prompt frameworks to paint over typed input.
      const initialSize = estimatePaneSize({
        fontSize: $settings.fontSize,
        lineHeight: $settings.lineHeight,
      });

      if (selectedLayout) {
        log(
          `Creating new session: repo=${repoPath}, target=${gitTarget?.label ?? "plain"}, name=${name}, layout=${selectedLayout.id}`,
        );
        // Resolve the first leaf's effective nono up-front — the session's
        // primary PTY is spawned now by createSessionShell, not by the
        // layout walker (which only spawns PTYs for leaves 2..N).
        const firstLeafNono = resolveFirstLeafNono(selectedLayout);
        const session = await createSessionShell(
          repoPath,
          name,
          worktreePathArg,
          branchArg,
          firstLeafNono.nonoProfile ?? undefined,
          firstLeafNono.nonoAllowDirs ?? undefined,
          initialSize,
        );
        log(`Session created via layout: ${session.id}`);
        addSession(session);

        const layoutResult = await applyLayoutToSession(session, selectedLayout);
        if (!layoutResult.ok) {
          try { await killSession(session.id); } catch { /* best-effort */ }
          removeSession(session.id);
          error = renderLayoutError(layoutResult.error);
          return;
        }
        if (layoutResult.warnings.length > 0) {
          log(`Layout applied with ${layoutResult.warnings.length} warning(s): ${layoutResult.warnings.join("; ")}`);
        }
        resetAndClose();
        return;
      }

      // Past this point selectedLayout is null, so the validation guard
      // above guarantees selectedProfile is non-null.
      const profile = selectedProfile!;

      log(
        `Creating new session: repo=${repoPath}, target=${gitTarget?.label ?? "plain"}, name=${name}, profile=${profile.id}`,
      );

      // Effective nono: dialog dropdown takes precedence over the profile's
      // own nono_profile. In either case we thread along the profile's
      // allow_dirs — they describe what that profile needs to work, and
      // aren't tied to a specific sandbox profile name.
      const effectiveNono = resolveNonoForProfile(profile, selectedNonoProfile);

      // Spawn a shell (optionally nono-wrapped), then type the profile's
      // setup / startup commands into it after the PTY is attached.
      const session = await createSessionShell(
        repoPath,
        name,
        worktreePathArg,
        branchArg,
        effectiveNono.nonoProfile,
        effectiveNono.nonoAllowDirs,
        initialSize,
      );

      log(`Session created: ${session.id}`);
      addSession(session);

      // Attach the chosen profile to the session's primary pane. Inline
      // profiles travel as full captures so they survive restart.
      const profileRef: SpawnProfileRef =
        profile.source === "inline"
          ? { kind: "inline", profile: profile }
          : { kind: "registered", id: profile.id };

      const mainPaneId = initSessionWithProfile(session.id, profileRef, effectiveNono);
      const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
      initTerminal(mainPaneId);
      await attachPtyListeners(mainPaneId);

      // Type the profile's commands into the new shell.
      // session.id is also the PTY id for the session-owned shell.
      await runProfileInPane(session.id, profile);

      resetAndClose();
    } catch (e) {
      logError("Failed to create session", e);
      error = String(e);
    } finally {
      creating = false;
    }
  }

  /**
   * Resolve the effective nono for the non-layout creation path.
   *
   * - Dialog dropdown wins when set: the user is explicit about which
   *   sandbox profile wraps the shell. We still pass the profile's
   *   allow_dirs because they describe what the spawn profile itself
   *   needs (e.g. a scratch dir) — they aren't coupled to a specific
   *   sandbox profile name.
   * - Otherwise fall back to whatever nono the SpawnProfile declares.
   */
  function resolveNonoForProfile(
    profile: SpawnProfile,
    dialogNonoProfile: string | null,
  ): { nonoProfile: string | undefined; nonoAllowDirs: string[] | undefined } {
    if (dialogNonoProfile) {
      return {
        nonoProfile: dialogNonoProfile,
        nonoAllowDirs: profile.nonoAllowDirs?.length
          ? profile.nonoAllowDirs
          : undefined,
      };
    }
    if (profile.nonoProfile) {
      return {
        nonoProfile: profile.nonoProfile,
        nonoAllowDirs: profile.nonoAllowDirs?.length
          ? profile.nonoAllowDirs
          : undefined,
      };
    }
    return { nonoProfile: undefined, nonoAllowDirs: undefined };
  }

  function renderLayoutError(err: LayoutApplyError): string {
    switch (err.kind) {
      case "missingProfile":
        return `Layout references unknown profile "${err.profileId}"${err.paneName ? ` (pane "${err.paneName}")` : ""}`;
      case "spawnFailed":
        return `Failed to spawn pane${err.paneName ? ` "${err.paneName}"` : ""}: ${err.cause}`;
      case "empty":
        return "Layout is empty — no panes to create";
    }
  }

  function handleProfileSelect(value: string) {
    if (value === "__custom__") {
      showCustomEditor = true;
      return;
    }
    selectedProfileId = value;
    inlineProfile = null;
  }

  function handleInlineSubmit(profile: SpawnProfile) {
    inlineProfile = profile;
    selectedProfileId = "__inline__";
    showCustomEditor = false;
  }

  function resetAndClose() {
    sessionName = "";
    isGitRepo = false;
    error = "";
    selectedNonoProfile = null;
    selectedLayoutId = "";
    selectedProfileId = "claude";
    inlineProfile = null;
    showCustomEditor = false;
    rootReposError = "";
    repoPickerOpen = true;
    layoutPickInput = "";
    layoutPickOpen = false;
    profilePickInput = "";
    profilePickOpen = false;
    nonoPickInput = "";
    nonoPickOpen = false;
    worktreeFilterInput = "";
    worktreePickOpen = true;
    worktreeActiveIndex = 0;
    onclose();
  }
</script>

<svelte:window on:keydown|capture={handleDialogKeydown} />

{#if visible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-md"
    onclick={(e) => { if (e.target === e.currentTarget) resetAndClose(); }}
    transition:fade={{ duration: 150 }}
  >
    <div
      class="ui-dialog w-[480px] rounded-2xl"
      transition:scale={{ duration: 150, start: 0.96 }}
    >
      <!-- Header -->
      <div class="border-b border-hairline bg-bg-surface/30 px-6 pt-5 pb-4">
        <h2 class="mb-1 text-base font-semibold tracking-tight text-text-primary">New Session</h2>
        <p class="text-xs text-text-muted">Pick a spawn profile and launch a pane</p>
      </div>

      <!-- Body -->
      <div class="px-6 py-5 flex flex-col gap-4">
        <!-- Repo picker -->
        <div class="flex flex-col gap-1.5">
          <label
            for="new-session-repo-picker"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Repository
          </label>
          <div class={pickerShellClass}>
            <Command.Root shouldFilter={true} loop={true} vimBindings={true}>
              <div class={pickerInputRowClass}>
                <Command.Input
                  id="new-session-repo-picker"
                  bind:value={repoPath}
                  placeholder="Type path or search configured repo roots"
                  class={pickerInputClass}
                  onfocus={() => { repoPickerOpen = true; }}
                  oninput={() => { repoPickerOpen = true; }}
                  onkeydown={(e) => {
                    if (e.key !== "Enter") return;
                    const match = findQuickPickMatch(repoPath);
                    e.preventDefault();
                    if (match) {
                      void selectQuickPick(match.path, match.label);
                      return;
                    }
                    repoPickerOpen = false;
                    void detectGitRepo(repoPath);
                  }}
                />
                <button
                  class="cursor-pointer rounded-md border border-border-subtle bg-bg-surface px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover hover:text-text-primary"
                  onclick={loadRootRepoOptions}
                  disabled={rootReposLoading}
                >
                  {rootReposLoading ? "..." : "Refresh"}
                </button>
                <button
                  class="cursor-pointer rounded-md border border-border-subtle bg-bg-surface px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover hover:text-text-primary"
                  onclick={pickRepo}
                >
                  Browse
                </button>
              </div>
              {#if repoPickerOpen && compatSettings.repoRoots.length > 0}
                <Command.List class={`${pickerListClass} max-h-36`}>
                  <Command.Empty class="px-3 py-2 text-[11px] text-text-muted">
                    No matching repositories
                  </Command.Empty>
                  <Command.Group>
                    <Command.GroupItems>
                      {#each quickPickOptions as opt (opt.path)}
                        <Command.Item
                          value={opt.label}
                          keywords={[opt.path]}
                          onSelect={() => {
                            void selectQuickPick(opt.path, opt.label);
                          }}
                          class={`${pickerItemClass} justify-between py-1.5 data-[selected]:bg-bg-active`}
                        >
                          <span class="truncate font-mono text-[12px] text-text-primary">{opt.label}</span>
                          <span class="ml-2 max-w-40 truncate font-mono text-[10px] text-text-muted">{opt.path}</span>
                        </Command.Item>
                      {/each}
                    </Command.GroupItems>
                  </Command.Group>
                </Command.List>
              {/if}
            </Command.Root>
          </div>
          {#if rootReposError}
            <p class="text-[11px] text-red">{rootReposError}</p>
          {:else if compatSettings.repoRoots.length > 0 && !rootReposLoading && rootRepoPaths.length === 0}
            <p class="text-[11px] text-text-muted">No git repositories found under configured roots.</p>
          {/if}
        </div>

        <!-- Non-git directory notice -->
        {#if repoPath && !isGitRepo}
          <div class="flex items-center gap-2 rounded-md border border-border-subtle bg-bg-deep/60 px-3 py-2">
            <span class="text-xs text-text-muted flex-1">Not a git repository</span>
            <button
              class="cursor-pointer rounded-md border border-accent-dim/20 bg-accent-dim/15 px-3 py-1.5 text-[11px] font-medium text-accent hover:bg-accent-dim/24"
              onclick={async () => {
                try {
                  await gitInit(repoPath);
                  await detectGitRepo(repoPath);
                } catch (e) {
                  error = String(e);
                }
              }}
            >
              Initialize Git
            </button>
          </div>
        {/if}

        {#if isGitRepo}
          <fieldset class="flex flex-col gap-1.5">
            <legend class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Worktree / Branch</legend>
            <p class="text-[11px] text-text-muted">Pick an existing worktree, or type a new branch name to create one.</p>
            <div class={pickerShellClass}>
              <div class={pickerInputRowClass}>
                <input
                  id="new-session-worktree-picker"
                  bind:value={worktreeFilterInput}
                  placeholder="e.g. feat/my-branch"
                  class={pickerInputClass}
                  onfocus={() => { worktreePickOpen = true; }}
                  oninput={() => { worktreePickOpen = true; }}
                  onkeydown={(e) => {
                    if (e.key === "ArrowDown") {
                      e.preventDefault();
                      moveWorktreeActive(1);
                      return;
                    }
                    if (e.key === "ArrowUp") {
                      e.preventDefault();
                      moveWorktreeActive(-1);
                      return;
                    }
                    if (e.key === "Enter") {
                      e.preventDefault();
                      if (filteredWorktrees.length > 0) selectActiveWorktree();
                      worktreePickOpen = false;
                    }
                  }}
                />
              </div>
              {#if worktreePickOpen}
                <div class={`${pickerListClass} max-h-30`}>
                  {#if worktrees.length === 0}
                    <p class="px-3 py-2 text-[11px] text-text-muted">No worktrees found.</p>
                  {:else if filteredWorktrees.length === 0}
                    <p class="px-3 py-2 text-[11px] text-text-muted">No matching worktrees.</p>
                  {:else}
                    {#each filteredWorktrees as wt, idx (wt.path)}
                      <button
                        class={`${pickerItemClass} w-full gap-2
                          {selectedWorktree?.path === wt.path
                            ? 'bg-bg-active border-border'
                            : idx === worktreeActiveIndex
                              ? 'border-border bg-bg-hover'
                              : 'border-border-subtle bg-bg-surface/50 hover:bg-bg-hover'}`}
                        onclick={() => {
                          worktreeActiveIndex = idx;
                          selectedWorktree = wt;
                          worktreeFilterInput = wt.branch;
                          worktreePickOpen = false;
                        }}
                      >
                        {#if wt.isMain}
                          <span class="text-[9px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded bg-green/10 text-green">main</span>
                        {/if}
                        <span class="font-mono text-xs text-accent">{wt.branch}</span>
                        <span class="ml-auto max-w-40 truncate font-mono text-[10px] text-text-muted">{wt.path}</span>
                      </button>
                    {/each}
                  {/if}
                </div>
              {/if}
            </div>
          </fieldset>
        {/if}

        <!-- Layout picker -->
        <div class="flex flex-col gap-1.5">
          <label
            for="new-session-layout"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Layout
          </label>
          <div class={pickerShellClass}>
            <Command.Root shouldFilter={true} loop={true} vimBindings={true}>
              <div class={pickerInputRowClass}>
                <Command.Input
                  id="new-session-layout"
                  bind:value={layoutPickInput}
                  placeholder="Pick layout"
                  class={pickerInputClass}
                  onfocus={() => { layoutPickOpen = true; }}
                  oninput={() => { layoutPickOpen = true; }}
                  onkeydown={(e) => {
                    if (e.key !== "Enter") return;
                    const match = findOptionMatch(layoutPickInput, layoutOptions);
                    if (!match) return;
                    e.preventDefault();
                    selectLayoutOption(match.value, match.label);
                  }}
                />
              </div>
              {#if layoutPickOpen}
                <Command.List class={`${pickerListClass} max-h-32`}>
                  <Command.Empty class="px-3 py-2 text-[11px] text-text-muted">
                    No matching layouts
                  </Command.Empty>
                  <Command.Group>
                    <Command.GroupItems>
                      {#each layoutOptions as option (option.value)}
                        <Command.Item
                          value={option.label}
                          keywords={[option.value]}
                          onSelect={() => selectLayoutOption(option.value, option.label)}
                          class={`${pickerItemClass} justify-between py-1.5 data-[selected]:bg-bg-active`}
                        >
                          <span class="truncate font-mono text-[12px] text-text-primary">{option.label}</span>
                          {#if selectedLayoutId === option.value}
                            <span class="ml-2 text-[10px] text-accent">selected</span>
                          {/if}
                        </Command.Item>
                      {/each}
                    </Command.GroupItems>
                  </Command.Group>
                </Command.List>
              {/if}
            </Command.Root>
          </div>
          {#if selectedLayout?.description}
            <p class="text-[11px] text-text-muted">{selectedLayout.description}</p>
          {/if}
        </div>

        {#if !selectedLayout}
          <!-- Spawn profile picker -->
          <div class="flex flex-col gap-1.5">
            <label
              for="new-session-profile"
              class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
            >
              Spawn profile
            </label>
            <div class={pickerShellClass}>
              <Command.Root shouldFilter={true} loop={true} vimBindings={true}>
                <div class={pickerInputRowClass}>
                  <Command.Input
                    id="new-session-profile"
                    bind:value={profilePickInput}
                    placeholder="Pick spawn profile"
                    class={pickerInputClass}
                    onfocus={() => { profilePickOpen = true; }}
                    oninput={() => { profilePickOpen = true; }}
                    onkeydown={(e) => {
                      if (e.key !== "Enter") return;
                      const match = findOptionMatch(profilePickInput, profileOptions);
                      if (!match) return;
                      e.preventDefault();
                      selectProfileOption(match.value, match.label);
                    }}
                  />
                </div>
                {#if profilePickOpen}
                  <Command.List class={`${pickerListClass} max-h-36`}>
                    <Command.Empty class="px-3 py-2 text-[11px] text-text-muted">
                      No matching profiles
                    </Command.Empty>
                    <Command.Group>
                      <Command.GroupItems>
                        {#each profileOptions as option (option.value)}
                          <Command.Item
                            value={option.label}
                            keywords={[option.value]}
                            onSelect={() => selectProfileOption(option.value, option.label)}
                            class={`${pickerItemClass} justify-between py-1.5 data-[selected]:bg-bg-active`}
                          >
                            <span class="truncate font-mono text-[12px] text-text-primary">{option.label}</span>
                            {#if selectedProfileId === option.value}
                              <span class="ml-2 text-[10px] text-accent">selected</span>
                            {/if}
                          </Command.Item>
                        {/each}
                      </Command.GroupItems>
                    </Command.Group>
                  </Command.List>
                {/if}
              </Command.Root>
            </div>
            {#if selectedProfile && selectedProfile.startupCommand}
              <p class="truncate font-mono text-[11px] text-text-muted">
                $ {selectedProfile.startupCommand}
              </p>
            {/if}
          </div>

          <!-- Nono sandbox profile -->
          {#if nonoInstalled}
            <div class="flex flex-col gap-1.5">
              <label
                for="new-session-nono"
                class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
              >
                Sandbox Profile
                <span class="font-normal normal-case tracking-normal">(nono.sh)</span>
              </label>
              <div class={pickerShellClass}>
                <Command.Root shouldFilter={true} loop={true} vimBindings={true}>
                  <div class={pickerInputRowClass}>
                    <Command.Input
                      id="new-session-nono"
                      bind:value={nonoPickInput}
                      placeholder="Pick sandbox profile"
                      class={pickerInputClass}
                      onfocus={() => { nonoPickOpen = true; }}
                      oninput={() => { nonoPickOpen = true; }}
                      onkeydown={(e) => {
                        if (e.key !== "Enter") return;
                        const match = findOptionMatch(nonoPickInput, nonoOptions);
                        if (!match) return;
                        e.preventDefault();
                        selectNonoOption(match.value, match.label);
                      }}
                    />
                  </div>
                  {#if nonoPickOpen}
                    <Command.List class={`${pickerListClass} max-h-32`}>
                      <Command.Empty class="px-3 py-2 text-[11px] text-text-muted">
                        No matching sandbox profiles
                      </Command.Empty>
                      <Command.Group>
                        <Command.GroupItems>
                          {#each nonoOptions as option (option.value)}
                            <Command.Item
                              value={option.label}
                              keywords={[option.value]}
                              onSelect={() => selectNonoOption(option.value, option.label)}
                              class={`${pickerItemClass} justify-between py-1.5 data-[selected]:bg-bg-active`}
                            >
                              <span class="truncate font-mono text-[12px] text-text-primary">{option.label}</span>
                              {#if (selectedNonoProfile ?? "") === option.value}
                                <span class="ml-2 text-[10px] text-accent">selected</span>
                              {/if}
                            </Command.Item>
                          {/each}
                        </Command.GroupItems>
                      </Command.Group>
                    </Command.List>
                  {/if}
                </Command.Root>
              </div>
            </div>
          {/if}

        {/if}

        <!-- Session name -->
        <div class="flex flex-col gap-1.5">
          <label
            for="new-session-name"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Session name <span class="font-normal normal-case tracking-normal">(optional)</span>
          </label>
          <input
            id="new-session-name"
            class="rounded-md border border-border-subtle bg-bg-deep px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
            bind:value={sessionName}
            placeholder="roux-my-feature"
          />
        </div>

        {#if error}
          <p class="text-xs text-red">{error}</p>
        {/if}
      </div>

      <!-- Footer -->
      <div class="flex justify-end gap-2 border-t border-hairline px-6 py-4">
        <div class="mr-auto self-center text-[11px] text-text-muted">Esc to close • Cmd/Ctrl+Enter to create</div>
        <button
          class="cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-5 py-2 text-[13px] font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary"
          onclick={resetAndClose}
        >
          Cancel
        </button>
        <button
          class="cursor-pointer rounded-xl border border-accent-dim/20 bg-accent-dim/15 px-5 py-2 text-[13px] font-medium text-accent hover:bg-accent-dim/24 disabled:opacity-50"
          onclick={handleCreate}
          disabled={creating}
        >
          {creating ? "Creating..." : "Create Session"}
        </button>
      </div>
    </div>
  </div>

  <ProfileCustomEditor
    visible={showCustomEditor}
    onclose={() => (showCustomEditor = false)}
    onsubmit={handleInlineSubmit}
  />
{/if}
