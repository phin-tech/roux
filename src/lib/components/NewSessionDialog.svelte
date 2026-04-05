<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { createSession, listWorktrees } from "$lib/tauri";
  import { addSession } from "$lib/stores/sessions";
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

      const session = await createSession(
        repoPath,
        name,
        mode === "existing" ? selectedWorktree?.path ?? null : null,
        mode === "new" ? branchName.trim() : null
      );

      addSession(session);
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
    onclose();
  }
</script>

{#if visible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50"
    onclick={(e) => { if (e.target === e.currentTarget) resetAndClose(); }}
  >
    <div class="bg-bg-surface border border-border rounded-xl w-[480px] shadow-2xl">
      <!-- Header -->
      <div class="px-6 pt-5 pb-4 border-b border-border-subtle">
        <h2 class="text-base font-semibold text-text-primary mb-1">New Session</h2>
        <p class="text-xs text-text-muted">Create a new Claude Code session in a git repository</p>
      </div>

      <!-- Body -->
      <div class="px-6 py-5 flex flex-col gap-4">
        <!-- Repo picker -->
        <div class="flex flex-col gap-1.5">
          <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Repository</label>
          <div class="flex gap-2">
            <input
              class="flex-1 bg-bg-deep border border-border rounded-md px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
              value={repoPath}
              oninput={(e) => (repoPath = e.currentTarget.value)}
              placeholder="~/src/my-project"
            />
            <button
              class="px-3 py-2 bg-bg-elevated border border-border rounded-md text-text-secondary text-xs cursor-pointer hover:bg-bg-hover"
              onclick={pickRepo}
            >
              Browse
            </button>
          </div>
        </div>

        <!-- Mode toggle -->
        <div class="flex flex-col gap-1.5">
          <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Mode</label>
          <div class="flex bg-bg-deep rounded-md p-0.5 border border-border-subtle">
            <button
              class="flex-1 py-1.5 px-3 border-none text-xs font-medium rounded cursor-pointer transition-all
                {mode === 'new' ? 'bg-bg-active text-text-primary' : 'bg-transparent text-text-secondary'}"
              onclick={() => (mode = "new")}
            >
              New Worktree
            </button>
            <button
              class="flex-1 py-1.5 px-3 border-none text-xs font-medium rounded cursor-pointer transition-all
                {mode === 'existing' ? 'bg-bg-active text-text-primary' : 'bg-transparent text-text-secondary'}"
              onclick={() => { mode = "existing"; loadWorktrees(); }}
            >
              Existing Directory
            </button>
          </div>
        </div>

        <!-- New worktree: branch input -->
        {#if mode === "new"}
          <div class="flex flex-col gap-1.5">
            <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Branch name</label>
            <input
              class="bg-bg-deep border border-border rounded-md px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
              bind:value={branchName}
              placeholder="feature/my-feature"
            />
          </div>
        {/if}

        <!-- Existing worktree: picker -->
        {#if mode === "existing"}
          <div class="flex flex-col gap-1.5">
            <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Select worktree</label>
            <div class="flex flex-col gap-1 max-h-30 overflow-y-auto">
              {#each worktrees as wt}
                <button
                  class="flex items-center gap-2 px-2.5 py-2 rounded-md cursor-pointer transition-colors border text-left
                    {selectedWorktree?.path === wt.path
                      ? 'bg-bg-active border-accent-dim'
                      : 'border-transparent hover:bg-bg-hover'}"
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
          </div>
        {/if}

        <!-- Session name -->
        <div class="flex flex-col gap-1.5">
          <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
            Session name <span class="font-normal normal-case tracking-normal">(optional)</span>
          </label>
          <input
            class="bg-bg-deep border border-border rounded-md px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
            bind:value={sessionName}
            placeholder="roux-my-feature"
          />
        </div>

        {#if error}
          <p class="text-xs text-red">{error}</p>
        {/if}
      </div>

      <!-- Footer -->
      <div class="px-6 py-4 border-t border-border-subtle flex justify-end gap-2">
        <button
          class="px-5 py-2 bg-bg-elevated border border-border rounded-md text-text-secondary text-[13px] font-medium cursor-pointer hover:bg-bg-hover"
          onclick={resetAndClose}
        >
          Cancel
        </button>
        <button
          class="px-5 py-2 bg-accent border-none rounded-md text-bg-deep text-[13px] font-medium cursor-pointer hover:brightness-110 disabled:opacity-50"
          onclick={handleCreate}
          disabled={creating}
        >
          {creating ? "Creating..." : "Create Session"}
        </button>
      </div>
    </div>
  </div>
{/if}
