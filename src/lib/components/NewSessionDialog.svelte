<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { open } from "@tauri-apps/plugin-dialog";
  import { createSession, listWorktrees, checkNonoInstalled, listNonoProfiles } from "$lib/tauri";
  import { addSession } from "$lib/stores/sessions";
  import { initSessionPanes } from "$lib/stores/panes";
  import { settings } from "$lib/stores/settings";
  import type { Worktree } from "$lib/types";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();

  let repoPath = $state($settings.defaultProjectPath ?? "");
  let mode = $state<"new" | "existing">("new");
  let branchName = $state("");
  let sessionName = $state("");
  let worktrees = $state<Worktree[]>([]);
  let selectedWorktree = $state<Worktree | null>(null);
  let error = $state("");
  let creating = $state(false);

  // Nono sandbox integration
  let nonoInstalled = $state(false);
  let nonoProfiles = $state<string[]>([]);
  let selectedNonoProfile = $state<string | null>(null);
  let skipPermissions = $state(false);

  // Check for nono on mount
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
    }
  });

  async function pickRepo() {
    const selected = await open({ directory: true, title: "Select Git Repository" });
    if (selected) {
      repoPath = selected as string;
      await loadWorktrees();
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
      error = "Please select a repository";
      return;
    }
    if (mode === "new" && !branchName.trim()) {
      error = "Branch name is required for new worktrees";
      return;
    }
    error = "";
    creating = true;

    try {
      const name =
        sessionName ||
        repoPath.split("/").pop() +
          "-" +
          (mode === "new" ? branchName : selectedWorktree?.branch ?? "main");

      const extraFlags: string[] = [];
      if (skipPermissions) {
        extraFlags.push("--dangerously-skip-permissions");
      }

      const session = await createSession(
        repoPath,
        name,
        mode === "existing" ? selectedWorktree?.path ?? null : null,
        mode === "new" ? branchName.trim() : null,
        extraFlags.length > 0 ? extraFlags : undefined,
        selectedNonoProfile,
      );

      addSession(session);
      initSessionPanes(session.id);
      resetAndClose();
    } catch (e) {
      error = String(e);
    } finally {
      creating = false;
    }
  }

  function resetAndClose() {
    branchName = "";
    sessionName = "";
    mode = "new";
    error = "";
    selectedNonoProfile = null;
    skipPermissions = false;
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
        <p class="text-xs text-text-muted">Create a new Claude Code session in a git repository</p>
      </div>

      <!-- Body -->
      <div class="px-6 py-5 flex flex-col gap-4">
        <!-- Repo picker -->
        <div class="flex flex-col gap-1.5">
          <label
            for="new-session-repo"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Repository
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

        <!-- Mode toggle -->
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
                <p class="text-xs text-text-muted py-2 text-center">No worktrees found. Select a git repository first.</p>
              {/if}
            </div>
          </fieldset>
        {/if}

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

        <!-- Skip permissions checkbox -->
        <label class="flex items-center gap-2.5 cursor-pointer group">
          <input
            type="checkbox"
            bind:checked={skipPermissions}
            class="w-4 h-4 rounded border border-border bg-bg-deep accent-amber-500 cursor-pointer"
          />
          <span class="text-[13px] text-text-secondary group-hover:text-text-primary transition-colors">
            Skip permission prompts
          </span>
          <span class="text-[10px] text-amber-400/70 font-mono">--dangerously-skip-permissions</span>
        </label>

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
{/if}
