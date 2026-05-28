<script lang="ts">
  import CircleStop from "@lucide/svelte/icons/circle-stop";
  import { fade, scale } from "svelte/transition";
  import { editingWorkItemId, closeWorkItemEditor } from "$lib/stores/ui";
  import {
    workItems,
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
  import {
    deleteWorkItemWithMode,
    type WorkItemDeleteMode,
  } from "$lib/workItems/deleteFlow";
  import { logError } from "$lib/logging";
  import WorkItemDeleteDialog from "./WorkItemDeleteDialog.svelte";

  // The card being edited, looked up live so external updates/deletes reflect
  // here (a delete elsewhere closes the modal by emptying this).
  const item = $derived(
    $editingWorkItemId
      ? ($workItems.find((i) => i.id === $editingWorkItemId) ?? null)
      : null,
  );
  const pendingDecision = $derived(
    item ? ($pendingDecisionByItem.get(item.id) ?? null) : null,
  );
  const itemRuns = $derived(item ? ($runsByItem.get(item.id) ?? []) : []);

  let title = $state("");
  let body = $state("");
  let status = $state<WorkItemStatus>("todo");
  let projectId = $state<string | null>(null);
  let error = $state("");
  let saving = $state(false);
  let deleteDialogOpen = $state(false);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);
  let resolvingDecision = $state<string | null>(null);
  let stoppingRunId = $state<string | null>(null);

  // Re-seed the form whenever a different card opens.
  let loadedId = $state<string | null>(null);
  $effect(() => {
    if (item && item.id !== loadedId) {
      loadedId = item.id;
      title = item.title;
      body = item.body ?? "";
      status = item.status;
      projectId = item.projectId;
      error = "";
      saving = false;
      deleteDialogOpen = false;
      deleting = false;
      deleteError = null;
      resolvingDecision = null;
      stoppingRunId = null;
    } else if (!item) {
      loadedId = null;
    }
  });

  async function handleSave() {
    if (!item) return;
    if (!title.trim()) {
      error = "Title is required";
      return;
    }
    saving = true;
    error = "";
    try {
      // The update SQL COALESCEs nullable columns, so a null leaves them
      // unchanged. We never clear the project (the picker hides "None" once a
      // card is assigned), and the description clears via an empty *string*
      // (COALESCE only keeps on SQL NULL), so emptying the textarea persists.
      await updateWorkItem(item.id, {
        title: title.trim(),
        body: body.trim() ? body : "",
        status,
        projectId,
      });
      closeWorkItemEditor();
    } catch (e) {
      error = String(e);
      logError(`work-item edit: save failed — ${e}`);
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
      logError(`work-item edit: delete failed — ${e}`);
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
      logError(`work-item decision: resolve failed — ${e}`);
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
      logError(`work-item run: stop failed — ${e}`);
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
    "w-full rounded-md border border-border-subtle bg-bg-deep px-3 py-2 text-[13px] text-text-primary outline-none focus:border-accent-dim";
</script>

{#if item}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-md"
    transition:fade={{ duration: 120 }}
    onkeydown={onKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div
      class="flex max-h-[90vh] w-[480px] max-w-[92vw] flex-col rounded-2xl border border-border bg-bg-surface shadow-xl"
      transition:scale={{ duration: 120, start: 0.97 }}
    >
      <div class="border-b border-hairline px-6 py-4">
        <h2 class="text-[15px] font-semibold text-text-primary">Edit card</h2>
      </div>

      <div class="app-scrollbar flex-1 overflow-y-auto px-6 py-5">
        <div class="flex flex-col gap-5">
          <div class="flex flex-col gap-1.5">
            <label for="wi-title" class={sectionLabel}>Title</label>
            <input id="wi-title" class={inputClass} bind:value={title} autocomplete="off" />
          </div>

          <div class="flex flex-col gap-1.5">
            <label for="wi-body" class={sectionLabel}>Description</label>
            <textarea
              id="wi-body"
              class={inputClass + " min-h-[96px] resize-y"}
              bind:value={body}
              rows="4"
            ></textarea>
          </div>

          <div class="grid grid-cols-2 gap-3">
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
              <select class={inputClass} bind:value={projectId}>
                <!-- "None" is offered only while the card is unassigned;
                     once it has a project you can reassign but not clear it. -->
                {#if projectId === null}
                  <option value={null}>None</option>
                {/if}
                {#each $projects as p (p.id)}
                  <option value={p.id}>{p.name}</option>
                {/each}
              </select>
            </label>
          </div>

          {#if projectId === null}
            <p class="text-[11px] text-text-muted">
              Assign a project to make “Start” work — it resolves the repo the
              session runs in.
            </p>
          {/if}

          {#if pendingDecision}
            <section class="rounded-lg border border-amber/30 bg-amber/10 p-3">
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
                    <span>{resolvingDecision === option.value ? "Resolving…" : option.label}</span>
                  </button>
                {/each}
              </div>
            </section>
          {/if}

          {#if itemRuns.length > 0}
            <section class="flex flex-col gap-2">
              <p class={sectionLabel}>Run History</p>
              <div class="flex flex-col overflow-hidden rounded-lg border border-border-subtle">
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

          {#if error}
            <p class="text-xs text-red">{error}</p>
          {/if}
        </div>
      </div>

      <div class="flex justify-end gap-2 border-t border-hairline px-6 py-4">
        <button
          class="mr-auto cursor-pointer self-center text-[12px] font-medium text-red/85 hover:text-red"
          onclick={() => {
            deleteDialogOpen = true;
            deleteError = null;
          }}
        >
          Delete
        </button>
        <button
          class="cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-5 py-2 text-[13px] font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary"
          onclick={closeWorkItemEditor}
        >
          Cancel
        </button>
        <button
          class="cursor-pointer rounded-xl border border-accent-dim/20 bg-accent-dim/15 px-5 py-2 text-[13px] font-medium text-accent hover:bg-accent-dim/24 disabled:opacity-50"
          onclick={handleSave}
          disabled={saving}
        >
          {saving ? "Saving…" : "Save"}
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
