<script lang="ts">
  import CircleStop from "@lucide/svelte/icons/circle-stop";
  import { fade, scale } from "svelte/transition";
  import { editingWorkItemId, newWorkItemEditor, closeWorkItemEditor } from "$lib/stores/ui";
  import {
    workItems,
    createWorkItem,
    updateWorkItem,
    WORK_ITEM_COLUMNS,
    COLUMN_LABELS,
    pendingDecisionByItem,
    resolveWorkItemDecision,
    runsByItem,
    stopWorkItemRun,
    type WorkItemStatus,
  } from "$lib/stores/workItems";
  import { projects } from "$lib/stores/projects";
  import { settings } from "$lib/stores/settings";
  import { profileList } from "$lib/panes/profiles";
  import { listWorktrees } from "$lib/tauri";
  import {
    deleteWorkItemWithMode,
    type WorkItemDeleteMode,
  } from "$lib/workItems/deleteFlow";
  import { logError } from "$lib/logging";
  import type { WorkItemInput, Worktree } from "$lib/bindings";
  import WorkItemDeleteDialog from "./WorkItemDeleteDialog.svelte";
  import RepoPickerField from "./RepoPickerField.svelte";

  type BranchBase = "main" | "originMain";

  const item = $derived(
    $editingWorkItemId
      ? ($workItems.find((i) => i.id === $editingWorkItemId) ?? null)
      : null,
  );
  const createRequest = $derived($newWorkItemEditor);
  const editorOpen = $derived(!!item || !!createRequest);
  const isCreating = $derived(!!createRequest);
  const pendingDecision = $derived(
    item ? ($pendingDecisionByItem.get(item.id) ?? null) : null,
  );
  const itemRuns = $derived(item ? ($runsByItem.get(item.id) ?? []) : []);

  let title = $state("");
  let body = $state("");
  let status = $state<WorkItemStatus>("todo");
  let projectId = $state<string | null>(null);
  let repoPath = $state("");
  let repoOverride = $state(true);
  let profileId = $state("claude");
  let worktreeTarget = $state("");
  let branchBase = $state<BranchBase>("main");
  let worktrees = $state<Worktree[]>([]);
  let worktreesLoading = $state(false);
  let error = $state("");
  let saving = $state(false);
  let deleteDialogOpen = $state(false);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);
  let resolvingDecision = $state<string | null>(null);
  let stoppingRunId = $state<string | null>(null);
  let worktreeLoadSeq = 0;

  const selectedProject = $derived(
    projectId ? ($projects.find((p) => p.id === projectId) ?? null) : null,
  );
  const projectRepoPath = $derived(selectedProject?.repoRoots?.[0] ?? null);
  const showRepoPicker = $derived(!projectId || repoOverride || !projectRepoPath);
  const effectiveRepoPath = $derived.by(() => {
    if (projectId && !repoOverride && projectRepoPath) return projectRepoPath;
    return repoPath.trim();
  });
  const profileOptions = $derived.by(() => {
    const profiles = [...$profileList];
    if (!profiles.some((profile) => profile.id === "claude")) {
      profiles.unshift({
        id: "claude",
        name: "Claude",
        setupCommand: null,
        startupCommand: null,
        startupBehavior: null,
        env: null,
        cwdOverride: null,
        icon: null,
        provider: "claude",
        nonoProfile: null,
        nonoAllowDirs: null,
        source: "builtin",
      });
    }
    return profiles;
  });

  let loadedKey = $state<string | null>(null);
  $effect(() => {
    if (item && item.id !== loadedKey) {
      loadedKey = item.id;
      title = item.title;
      body = item.body ?? "";
      status = item.status;
      projectId = item.projectId;
      const projectRoot = item.projectId
        ? ($projects.find((p) => p.id === item.projectId)?.repoRoots?.[0] ?? null)
        : null;
      repoOverride = !item.projectId || !!item.repoPath;
      repoPath = item.repoPath ?? projectRoot ?? $settings.defaultProjectPath ?? "";
      profileId = item.agentProfile ?? "claude";
      worktreeTarget = item.worktreePath ?? item.branch ?? "";
      branchBase = item.fetchFirst || item.baseBranch === "origin/main" ? "originMain" : "main";
      resetTransientState();
    } else if (createRequest && loadedKey !== `new:${createRequest.status}`) {
      loadedKey = `new:${createRequest.status}`;
      title = createRequest.title ?? "";
      body = "";
      status = createRequest.status;
      projectId = null;
      repoOverride = true;
      repoPath = $settings.defaultProjectPath ?? "";
      profileId = "claude";
      worktreeTarget = "";
      branchBase = "main";
      resetTransientState();
    } else if (!item && !createRequest) {
      loadedKey = null;
    }
  });

  $effect(() => {
    const repo = effectiveRepoPath;
    if (!repo) {
      worktrees = [];
      worktreesLoading = false;
      return;
    }
    const seq = ++worktreeLoadSeq;
    worktreesLoading = true;
    listWorktrees(repo)
      .then((items) => {
        if (seq === worktreeLoadSeq) worktrees = items;
      })
      .catch((e) => {
        if (seq === worktreeLoadSeq) {
          worktrees = [];
          logError(`work-item edit: list worktrees failed - ${e}`);
        }
      })
      .finally(() => {
        if (seq === worktreeLoadSeq) worktreesLoading = false;
      });
  });

  function resetTransientState() {
    error = "";
    saving = false;
    deleteDialogOpen = false;
    deleting = false;
    deleteError = null;
    resolvingDecision = null;
    stoppingRunId = null;
  }

  function handleProjectChange() {
    if (projectId) {
      repoOverride = false;
      repoPath = projectRepoPath ?? repoPath;
      return;
    }
    repoOverride = true;
    repoPath = repoPath || $settings.defaultProjectPath || "";
  }

  function repoPathForSave(): string | null {
    const repo = effectiveRepoPath.trim();
    if (projectId && !repoOverride && projectRepoPath) return null;
    return repo || null;
  }

  function applyTargetFields(payload: WorkItemInput): WorkItemInput {
    payload.repoPath = repoPathForSave();
    payload.agentProfile = profileId || "claude";
    const target = worktreeTarget.trim();
    const exactWorktree = worktrees.find((wt) => wt.path === target || wt.branch === target);
    if (!target) {
      payload.worktreePath = null;
      payload.branch = null;
      payload.baseBranch = null;
      payload.fetchFirst = null;
    } else if (exactWorktree) {
      payload.worktreePath = exactWorktree.path;
      payload.branch = null;
      payload.baseBranch = null;
      payload.fetchFirst = null;
    } else {
      const selectedBase = branchBase === "originMain" ? "origin/main" : "main";
      payload.worktreePath = null;
      payload.branch = target;
      payload.baseBranch = selectedBase;
      payload.fetchFirst = branchBase === "originMain";
    }
    return payload;
  }

  async function handleSave() {
    if (!editorOpen) return;
    if (!title.trim()) {
      error = "Title is required";
      return;
    }
    saving = true;
    error = "";
    try {
      const common = {
        title: title.trim(),
        body: body.trim() ? body : "",
        status,
        projectId,
      };
      if (isCreating) {
        await createWorkItem(
          applyTargetFields({
            ...common,
            sortOrder: Date.now(),
          }),
        );
      } else if (item) {
        await updateWorkItem(item.id, applyTargetFields(common));
      }
      closeWorkItemEditor();
    } catch (e) {
      error = String(e);
      logError(`work-item edit: save failed - ${e}`);
    } finally {
      saving = false;
    }
  }

  async function handleDelete(mode: WorkItemDeleteMode) {
    if (!item) return;
    deleting = true;
    deleteError = null;
    try {
      await deleteWorkItemWithMode(item, mode);
      closeWorkItemEditor();
    } catch (e) {
      deleteError = String(e);
      logError(`work-item edit: delete failed - ${e}`);
    } finally {
      deleting = false;
    }
  }

  async function handleResolveDecision(value: string) {
    if (!pendingDecision) return;
    resolvingDecision = value;
    error = "";
    try {
      await resolveWorkItemDecision(pendingDecision.id, value);
    } catch (e) {
      error = String(e);
      logError(`work-item decision: resolve failed - ${e}`);
    } finally {
      resolvingDecision = null;
    }
  }

  async function handleStopRun(runId: string) {
    stoppingRunId = runId;
    error = "";
    try {
      await stopWorkItemRun(runId);
    } catch (e) {
      error = String(e);
      logError(`work-item run: stop failed - ${e}`);
    } finally {
      stoppingRunId = null;
    }
  }

  function isStoppableRun(status: string): boolean {
    return status === "queued" || status === "starting" || status === "running" || status === "blocked";
  }

  function runLabel(createdAt: number): string {
    return new Date(createdAt * 1000).toLocaleString([], {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeWorkItemEditor();
    } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      void handleSave();
    }
  }

  const sectionLabel =
    "text-[11px] font-semibold uppercase tracking-wider text-text-muted";
  const inputClass =
    "w-full rounded-md border border-border-subtle bg-bg-deep px-3 py-2 text-[13px] text-text-primary outline-none placeholder:text-text-muted focus:border-accent-dim";
  const secondaryButton =
    "cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-5 py-2 text-[13px] font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary";
  const primaryButton =
    "cursor-pointer rounded-xl border border-accent-dim/20 bg-accent-dim/15 px-5 py-2 text-[13px] font-medium text-accent hover:bg-accent-dim/24 disabled:opacity-50";
</script>

{#if editorOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-md"
    transition:fade={{ duration: 120 }}
    onkeydown={onKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div
      class="ui-dialog flex max-h-[90vh] w-[640px] max-w-[92vw] flex-col overflow-hidden rounded-2xl"
      transition:scale={{ duration: 150, start: 0.96 }}
    >
      <div class="border-b border-hairline bg-bg-surface/30 px-6 pb-4 pt-5">
        <h2 class="mb-1 text-base font-semibold tracking-tight text-text-primary">
          {isCreating ? "New card" : "Edit card"}
        </h2>
        <p class="text-xs text-text-muted">
          Pick card details and where this task should run
        </p>
      </div>

      <div class="app-scrollbar flex-1 overflow-y-auto px-6 py-5">
        <div class="flex flex-col gap-5">
          <section class="flex flex-col gap-4">
            <div class="flex flex-col gap-1.5">
              <label for="wi-title" class={sectionLabel}>Title</label>
              <input id="wi-title" class={inputClass} bind:value={title} autocomplete="off" />
            </div>

            <div class="flex flex-col gap-1.5">
              <label for="wi-body" class={sectionLabel}>Body</label>
              <textarea
                id="wi-body"
                class={inputClass + " min-h-[104px] resize-y leading-5"}
                bind:value={body}
                rows="4"
              ></textarea>
            </div>

            <div class="flex flex-col gap-3">
              <label class="flex flex-col gap-1.5">
                <span class={sectionLabel}>Column</span>
                <select class={inputClass} bind:value={status}>
                  {#each WORK_ITEM_COLUMNS as col (col)}
                    <option value={col}>{COLUMN_LABELS[col]}</option>
                  {/each}
                </select>
              </label>
              <label class="flex flex-col gap-1.5">
                <span class={sectionLabel}>Project</span>
                <select class={inputClass} bind:value={projectId} onchange={handleProjectChange}>
                  <option value={null}>No project</option>
                  {#each $projects as p (p.id)}
                    <option value={p.id}>{p.name}</option>
                  {/each}
                </select>
              </label>
            </div>
          </section>

          <section class="flex flex-col gap-4">
            <div class="flex flex-col gap-1.5">
              <div class="flex items-center justify-between gap-3">
                <span class={sectionLabel}>Repository</span>
                {#if projectId && projectRepoPath}
                  <button
                    type="button"
                    class="text-[11px] font-medium text-accent transition-colors hover:text-accent-bright"
                    onclick={() => {
                      repoOverride = !repoOverride;
                      if (!repoOverride) repoPath = projectRepoPath;
                    }}
                  >
                    {repoOverride ? "Use project repo" : "Override"}
                  </button>
                {/if}
              </div>
              {#if showRepoPicker}
                <RepoPickerField
                  id="wi-repo"
                  label={null}
                  bind:value={repoPath}
                  placeholder="Type path or search configured repo roots"
                  enabled={editorOpen && showRepoPicker}
                  onselect={(path) => {
                    repoPath = path;
                  }}
                  onenter={(text) => {
                    repoPath = text.trim();
                  }}
                />
              {:else if projectRepoPath}
                <p class="truncate rounded-md border border-border-subtle bg-bg-deep px-3 py-2 font-mono text-[12px] text-text-secondary">
                  {projectRepoPath}
                </p>
              {/if}
            </div>

            <fieldset class="flex flex-col gap-1.5">
              <legend class={sectionLabel}>Worktree / Branch</legend>
              <p class="text-[11px] text-text-muted">
                Pick an existing worktree, or type a new branch name to create one. Leave empty to use the repo root.
              </p>
              <input
                id="wi-worktree-target"
                class={inputClass + " font-mono"}
                bind:value={worktreeTarget}
                list="wi-worktree-options"
                placeholder={worktreesLoading ? "Loading worktrees..." : "main, feat/my-branch, or existing path"}
                autocomplete="off"
                disabled={!effectiveRepoPath || worktreesLoading}
              />
              <datalist id="wi-worktree-options">
                {#if !worktreesLoading}
                  {#each worktrees as wt (wt.path)}
                    <option value={wt.branch || wt.path}>{wt.path}</option>
                  {/each}
                {/if}
              </datalist>
            </fieldset>

            <div class="flex flex-col gap-1">
              <label
                for="wi-start-point"
                class="text-[10px] font-semibold uppercase tracking-wider text-text-muted"
              >
                Branch from
              </label>
              <select
                id="wi-start-point"
                bind:value={branchBase}
                class={inputClass}
              >
                <option value="main">local main</option>
                <option value="originMain">origin/main (fetch first)</option>
              </select>
              <p class="text-[10px] text-text-muted/80">
                Only used when creating a new branch from the branch field above.
              </p>
            </div>

            <label class="flex flex-col gap-1.5">
              <span class={sectionLabel}>Spawn profile</span>
              <select class={inputClass} bind:value={profileId}>
                {#each profileOptions as profile (profile.id)}
                  <option value={profile.id}>{profile.name}</option>
                {/each}
              </select>
            </label>
          </section>

          {#if pendingDecision || itemRuns.length > 0}
            <section class="flex flex-col gap-3 border-t border-hairline pt-4">
              {#if pendingDecision}
                <div class="rounded-md border border-amber/30 bg-amber/10 p-3">
                  <p class={sectionLabel}>Blocked Decision</p>
                  <p class="mt-1 text-[13px] leading-5 text-text-primary">
                    {pendingDecision.question}
                  </p>
                  <div class="mt-3 flex flex-col gap-2">
                    {#each pendingDecision.options as option, index (option.value)}
                      <button
                        type="button"
                        class="flex items-center gap-2 rounded-md border border-border-subtle bg-bg-surface px-3 py-2 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary disabled:opacity-50"
                        onclick={() => handleResolveDecision(option.value)}
                        disabled={resolvingDecision !== null}
                      >
                        <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-amber/20 text-[10px] font-semibold text-amber">
                          {index + 1}
                        </span>
                        <span>{resolvingDecision === option.value ? "Resolving..." : option.label}</span>
                      </button>
                    {/each}
                  </div>
                </div>
              {/if}

              {#if itemRuns.length > 0}
                <section class="flex flex-col gap-2">
                  <p class={sectionLabel}>Run History</p>
                  <div class="flex flex-col overflow-hidden rounded-md border border-border-subtle">
                    {#each itemRuns as run (run.id)}
                      <div class="flex items-start gap-3 border-b border-hairline px-3 py-2 last:border-b-0">
                        <div class="min-w-0 flex-1">
                          <div class="flex flex-wrap items-center gap-2">
                            <span class="text-[12px] font-semibold capitalize text-text-primary">
                              {run.status}
                            </span>
                            {#if run.provider || run.profileId}
                              <span class="text-[11px] text-text-muted">
                                {run.provider ?? run.profileId}
                              </span>
                            {/if}
                          </div>
                          <p class="mt-0.5 truncate text-[11px] text-text-muted">
                            {run.branch ?? run.worktreePath ?? run.sessionId ?? run.id}
                          </p>
                        </div>
                        <div class="flex shrink-0 items-center gap-2">
                          <time class="text-[11px] text-text-muted" datetime={String(run.createdAt)}>
                            {runLabel(run.createdAt)}
                          </time>
                          {#if isStoppableRun(run.status)}
                            <button
                              type="button"
                              class="inline-flex h-6 items-center gap-1 rounded-md border border-red/30 bg-red/10 px-2 text-[11px] font-medium text-red transition-colors hover:bg-red/15 disabled:opacity-50"
                              onclick={() => handleStopRun(run.id)}
                              disabled={stoppingRunId !== null}
                              aria-label={`Stop run ${run.id}`}
                            >
                              <CircleStop size={12} strokeWidth={2.1} />
                              <span>{stoppingRunId === run.id ? "Stopping" : "Stop"}</span>
                            </button>
                          {/if}
                        </div>
                      </div>
                    {/each}
                  </div>
                </section>
              {/if}
            </section>
          {/if}

          {#if error}
            <p class="text-xs text-red">{error}</p>
          {/if}
        </div>
      </div>

      <div class="flex justify-end gap-2 border-t border-hairline px-6 py-4">
        <div class="mr-auto self-center text-[11px] text-text-muted">
          Esc to close • Cmd/Ctrl+Enter to {isCreating ? "create" : "save"}
        </div>
        {#if item}
          <button
            type="button"
            class="cursor-pointer self-center text-[12px] font-medium text-red/85 hover:text-red"
            onclick={() => {
              deleteDialogOpen = true;
              deleteError = null;
            }}
          >
            Delete
          </button>
        {/if}
        <button
          type="button"
          class={secondaryButton}
          onclick={closeWorkItemEditor}
        >
          Cancel
        </button>
        <button
          type="button"
          class={primaryButton}
          onclick={handleSave}
          disabled={saving || worktreesLoading}
        >
          {saving ? "Saving..." : isCreating ? "Create" : "Save"}
        </button>
      </div>
    </div>
  </div>

  <WorkItemDeleteDialog
    item={deleteDialogOpen ? item : null}
    deleting={deleting}
    error={deleteError}
    onCancel={() => {
      if (!deleting) deleteDialogOpen = false;
    }}
    onConfirm={handleDelete}
  />
{/if}
