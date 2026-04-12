<script lang="ts">
  import { fade, scale } from "svelte/transition";
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
  import { applyLayoutToSession, type LayoutApplyError } from "$lib/panes/layoutRunner";
  import { initSessionWithProfile } from "$lib/panes/actions";
  import { settings } from "$lib/stores/settings";
  import {
    profileList,
    type SpawnProfile,
    type SpawnProfileRef,
  } from "$lib/panes/profiles";
  import { runProfileInPane } from "$lib/panes/profileRunner";
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
  let mode = $state<"new" | "existing" | "plain">("plain");
  let branchName = $state("");
  let sessionName = $state("");
  let worktrees = $state<Worktree[]>([]);
  let selectedWorktree = $state<Worktree | null>(null);
  let error = $state("");
  let creating = $state(false);

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

  async function detectGitRepo(path: string) {
    isGitRepo = await checkIsGitRepo(path);
    if (isGitRepo) {
      mode = "new";
      await loadWorktrees();
    } else {
      mode = "plain";
      worktrees = [];
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
      selectedWorktree = worktrees.find((w) => w.isMain) ?? worktrees[0] ?? null;
    } catch {
      worktrees = [];
    }
  }

  async function handleCreate() {
    if (!repoPath) {
      error = "Please select a directory";
      return;
    }
    if (mode === "new" && !branchName.trim()) {
      error = "Branch name is required for new worktrees";
      return;
    }
    if (!selectedLayout && !selectedProfile) {
      error = "Pick a spawn profile (or use Custom…).";
      return;
    }
    error = "";
    creating = true;

    try {
      const name =
        sessionName ||
        (mode === "plain"
          ? repoPath.split("/").pop() ?? "session"
          : repoPath.split("/").pop() +
              "-" +
              (mode === "new" ? branchName : selectedWorktree?.branch ?? "main"));

      const worktreePathArg =
        mode === "existing" ? selectedWorktree?.path ?? null : null;
      const branchArg = mode === "new" ? branchName.trim() : null;

      if (selectedLayout) {
        log(
          `Creating new session: repo=${repoPath}, mode=${mode}, name=${name}, layout=${selectedLayout.id}`,
        );
        const session = await createSessionShell(
          repoPath,
          name,
          worktreePathArg,
          branchArg,
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
        `Creating new session: repo=${repoPath}, mode=${mode}, name=${name}, profile=${profile.id}`,
      );

      // Spawn a shell (optionally nono-wrapped), then type the profile's
      // setup / startup commands into it after the PTY is attached.
      const session = await createSessionShell(
        repoPath,
        name,
        worktreePathArg,
        branchArg,
        selectedNonoProfile ?? undefined,
        undefined,
      );

      log(`Session created: ${session.id}`);
      addSession(session);

      // Attach the chosen profile to the session's primary pane. Inline
      // profiles travel as full captures so they survive restart.
      const profileRef: SpawnProfileRef =
        profile.source === "inline"
          ? { kind: "inline", profile: profile }
          : { kind: "registered", id: profile.id };

      const mainPaneId = initSessionWithProfile(session.id, profileRef);
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
    branchName = "";
    sessionName = "";
    mode = "plain";
    isGitRepo = false;
    error = "";
    selectedNonoProfile = null;
    selectedLayoutId = "";
    selectedProfileId = "claude";
    inlineProfile = null;
    showCustomEditor = false;
    onclose();
  }
</script>

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
            for="new-session-repo"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Directory
          </label>
          <div class="flex gap-2">
            <input
              id="new-session-repo"
              class="flex-1 rounded-md border border-border-subtle bg-bg-deep px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
              value={repoPath}
              oninput={(e) => (repoPath = e.currentTarget.value)}
              placeholder="~/src/my-project"
            />
            <button
              class="cursor-pointer rounded-md border border-border-subtle bg-bg-surface px-3 py-2 text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary"
              onclick={pickRepo}
            >
              Browse
            </button>
          </div>
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

        <!-- Mode toggle (only for git repos) -->
        {#if isGitRepo}
          <fieldset class="flex flex-col gap-1.5">
            <legend class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Mode</legend>
            <div class="flex rounded-xl border border-border-subtle bg-bg-deep/80 p-1">
              <button
                class="flex-1 rounded-lg border-none px-3 py-2 text-xs font-medium cursor-pointer transition-all
                  {mode === 'new' ? 'bg-bg-active text-text-primary shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]' : 'bg-transparent text-text-secondary hover:text-text-primary'}"
                onclick={() => (mode = "new")}
              >
                New Worktree
              </button>
              <button
                class="flex-1 rounded-lg border-none px-3 py-2 text-xs font-medium cursor-pointer transition-all
                  {mode === 'existing' ? 'bg-bg-active text-text-primary shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]' : 'bg-transparent text-text-secondary hover:text-text-primary'}"
                onclick={() => { mode = "existing"; loadWorktrees(); }}
              >
                Existing Directory
              </button>
            </div>
          </fieldset>
        {/if}

        <!-- New worktree: branch input -->
        {#if mode === "new"}
          <div class="flex flex-col gap-1.5">
            <label
              for="new-session-branch"
              class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
            >
              Branch name
            </label>
            <input
              id="new-session-branch"
              class="rounded-md border border-border-subtle bg-bg-deep px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
              bind:value={branchName}
              placeholder="feature/my-feature"
            />
          </div>
        {/if}

        <!-- Existing worktree: picker -->
        {#if mode === "existing"}
          <fieldset class="flex flex-col gap-1.5">
            <legend class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Select worktree</legend>
            <div class="flex flex-col gap-1 max-h-30 overflow-y-auto">
              {#each worktrees as wt}
                <button
                  class="flex items-center gap-2 rounded-md border px-2.5 py-2 text-left cursor-pointer transition-colors
                    {selectedWorktree?.path === wt.path
                      ? 'bg-bg-active border-border'
                      : 'border-border-subtle bg-bg-surface/50 hover:bg-bg-hover'}"
                  onclick={() => (selectedWorktree = wt)}
                >
                  {#if wt.isMain}
                    <span class="text-[9px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded bg-green/10 text-green">main</span>
                  {/if}
                  <span class="font-mono text-xs text-accent">{wt.branch}</span>
                  <span class="font-mono text-[10px] text-text-muted ml-auto truncate max-w-40">{wt.path}</span>
                </button>
              {/each}
              {#if worktrees.length === 0}
                <p class="text-xs text-text-muted py-2 text-center">No worktrees found.</p>
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
          <select
            id="new-session-layout"
            class="bg-bg-deep border border-border rounded-md px-3 py-2 text-[13px] text-text-primary outline-none focus:border-accent-dim appearance-none cursor-pointer"
            onchange={(e) => { selectedLayoutId = e.currentTarget.value; }}
          >
            <option value="">None (single pane)</option>
            {#each $layoutList as layout}
              <option
                value={layout.id}
                selected={selectedLayoutId === layout.id}
              >
                {layout.name}
              </option>
            {/each}
          </select>
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
            <select
              id="new-session-profile"
              class="bg-bg-deep border border-border rounded-md px-3 py-2 text-[13px] text-text-primary outline-none focus:border-accent-dim appearance-none cursor-pointer"
              onchange={(e) => handleProfileSelect(e.currentTarget.value)}
            >
              {#each $profileList as profile}
                <option
                  value={profile.id}
                  selected={selectedProfileId === profile.id}
                >
                  {profile.name} {profile.source === "user" ? "(user)" : ""}
                </option>
              {/each}
              {#if inlineProfile}
                <option value="__inline__" selected={selectedProfileId === "__inline__"}>
                  {inlineProfile.name} (custom)
                </option>
              {/if}
              <option value="__custom__">Custom…</option>
            </select>
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
              <select
                id="new-session-nono"
                class="bg-bg-deep border border-border rounded-md px-3 py-2 text-[13px] text-text-primary outline-none focus:border-accent-dim appearance-none cursor-pointer"
                onchange={(e) => {
                  const val = e.currentTarget.value;
                  selectedNonoProfile = val === "" ? null : val;
                }}
              >
                <option value="">None (bare claude)</option>
                {#each nonoProfiles as profile}
                  <option value={profile} selected={selectedNonoProfile === profile}>{profile}</option>
                {/each}
              </select>
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
