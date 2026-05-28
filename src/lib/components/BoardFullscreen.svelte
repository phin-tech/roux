<script lang="ts">
  import { derived } from "svelte/store";
  import X from "@lucide/svelte/icons/x";
  import {
    itemsByColumn,
    WORK_ITEM_COLUMNS,
    COLUMN_LABELS,
    moveWorkItem,
    startWorkItem,
    planWorkItem,
    acceptWorkItemReview,
    createWorkItem,
    pendingDecisionByItem,
    activePlanningRunByItem,
    type WorkItemStatus,
  } from "$lib/stores/workItems";
  import { sessionList } from "$lib/stores/sessions";
  import type { SessionStatus } from "$lib/types";
  import { closeBoardFullscreen, openWorkItemEditor, openWorkItemSessionStart } from "$lib/stores/ui";
  import { openSessionById } from "$lib/panes/openSession";
  import { formatWorkItemStartError } from "$lib/board/startErrors";
  import {
    deleteWorkItemWithMode,
    type WorkItemDeleteMode,
  } from "$lib/workItems/deleteFlow";
  import type { WorkItem } from "$lib/bindings";
  import {
    hasWorkItemDragData,
    readWorkItemDragData,
  } from "$lib/board/drag";
  import WorkItemCard from "./WorkItemCard.svelte";
  import AddCardInput from "./AddCardInput.svelte";
  import WorkItemDeleteDialog from "./WorkItemDeleteDialog.svelte";

  const sessionStatusMap = derived(sessionList, ($sessions) => {
    const m = new Map<string, SessionStatus>();
    for (const s of $sessions) m.set(s.id, s.status);
    return m;
  });

  // Column currently under a valid drag, for the drop-target highlight.
  let dragOverColumn = $state<WorkItemStatus | null>(null);
  let startingItemIds = $state<Record<string, boolean>>({});
  let planningItemIds = $state<Record<string, boolean>>({});
  let acceptingItemIds = $state<Record<string, boolean>>({});
  let startErrors = $state<Record<string, string>>({});
  let planErrors = $state<Record<string, string>>({});
  let deleteTarget = $state<WorkItem | null>(null);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);

  async function handleMove(id: string, status: WorkItemStatus) {
    await moveWorkItem(id, status, Date.now());
  }

  function withoutKey<T>(record: Record<string, T>, key: string): Record<string, T> {
    const { [key]: _removed, ...rest } = record;
    return rest;
  }

  function needsStartConfig(item: WorkItem): boolean {
    return !item.agentProfile || (!item.repoPath && !item.projectId);
  }

  async function handleStart(id: string, item: WorkItem) {
    if (needsStartConfig(item)) {
      openWorkItemSessionStart({ itemId: item.id, title: item.title });
      return;
    }
    if (startingItemIds[id]) return;
    startingItemIds = { ...startingItemIds, [id]: true };
    startErrors = withoutKey(startErrors, id);

    // Start creates the session/worktree and moves the card after prompt dispatch.
    try {
      await startWorkItem(id);
    } catch (err) {
      startErrors = { ...startErrors, [id]: formatWorkItemStartError(err) };
      console.error("Failed to start work item", err);
    } finally {
      startingItemIds = withoutKey(startingItemIds, id);
    }
  }

  function formatPlanError(err: unknown): string {
    const message = err instanceof Error ? err.message : String(err);
    return message ? `Plan failed: ${message}` : "Plan failed.";
  }

  async function handlePlan(id: string, _item: WorkItem, replaceActive = false) {
    if (planningItemIds[id]) return;
    planningItemIds = { ...planningItemIds, [id]: true };
    planErrors = withoutKey(planErrors, id);
    try {
      const sessionId = replaceActive
        ? await planWorkItem(id, { replaceActive: true })
        : await planWorkItem(id);
      await handleOpen(sessionId);
    } catch (err) {
      planErrors = { ...planErrors, [id]: formatPlanError(err) };
      console.error("Failed to plan work item", err);
    } finally {
      planningItemIds = withoutKey(planningItemIds, id);
    }
  }

  async function handleAcceptReview(id: string, _item: WorkItem) {
    if (acceptingItemIds[id]) return;
    acceptingItemIds = { ...acceptingItemIds, [id]: true };
    startErrors = withoutKey(startErrors, id);
    try {
      await acceptWorkItemReview(id);
    } catch (err) {
      startErrors = { ...startErrors, [id]: "Failed to accept review." };
      console.error("Failed to accept work item review", err);
    } finally {
      acceptingItemIds = withoutKey(acceptingItemIds, id);
    }
  }

  async function handleCreate(title: string, status: WorkItemStatus) {
    // The card lands in the store via the broadcast `created` event.
    await createWorkItem({ title, status, sortOrder: Date.now() });
  }

  function handleDelete(_id: string, item: WorkItem) {
    deleteTarget = item;
    deleteError = null;
  }

  async function confirmDelete(mode: WorkItemDeleteMode) {
    if (!deleteTarget) return;
    deleting = true;
    deleteError = null;
    try {
      await deleteWorkItemWithMode(deleteTarget, mode);
      deleteTarget = null;
    } catch (err) {
      deleteError = "Failed to delete card.";
      console.error("Failed to delete work item", err);
    } finally {
      deleting = false;
    }
  }

  async function handleOpen(sessionId: string) {
    const result = await openSessionById(sessionId);
    if (result === "gone") {
      console.error(`Session ${sessionId} is no longer running`);
      return;
    }
    // Reveal the terminal we just focused — the board overlay covers it.
    closeBoardFullscreen();
  }

  function handleDragOver(event: DragEvent, col: WorkItemStatus) {
    if (!hasWorkItemDragData(event.dataTransfer)) return;
    // preventDefault marks this element as a valid drop target.
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    dragOverColumn = col;
  }

  function handleDragLeave(col: WorkItemStatus) {
    if (dragOverColumn === col) dragOverColumn = null;
  }

  async function handleDrop(event: DragEvent, col: WorkItemStatus) {
    dragOverColumn = null;
    const payload = readWorkItemDragData(event.dataTransfer);
    if (!payload) return;
    event.preventDefault();
    if (payload.fromStatus === col) return;
    await handleMove(payload.itemId, col);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeBoardFullscreen();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="absolute inset-0 z-30 flex flex-col bg-bg-deep">
  <div
    class="flex h-9 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3"
  >
    <span class="text-sm font-semibold tracking-tight text-text-primary">Board</span>
    <button
      type="button"
      class="flex h-6 w-6 items-center justify-center rounded text-text-muted transition-colors hover:bg-surface-2 hover:text-text"
      onclick={closeBoardFullscreen}
      aria-label="Close board"
      title="Close board (Esc)"
    >
      <X size={14} />
    </button>
  </div>

  <div class="flex min-h-0 flex-1 flex-row gap-3 overflow-x-auto p-4">
    {#each WORK_ITEM_COLUMNS as col (col)}
      {@const items = $itemsByColumn.get(col) ?? []}
      <section
        class="flex w-72 shrink-0 flex-col rounded-lg border bg-bg-base/40 transition-colors"
        class:border-accent={dragOverColumn === col}
        class:border-border-subtle={dragOverColumn !== col}
        data-testid="board-column"
        data-column={col}
        role="group"
        aria-label={COLUMN_LABELS[col]}
        ondragover={(e) => handleDragOver(e, col)}
        ondragleave={() => handleDragLeave(col)}
        ondrop={(e) => handleDrop(e, col)}
      >
        <div class="flex items-center gap-1.5 px-3 py-2">
          <span
            class="text-[11px] font-semibold uppercase tracking-wide text-text-muted"
          >
            {COLUMN_LABELS[col]}
          </span>
          {#if items.length > 0}
            <span
              class="rounded-full bg-surface-2 px-1.5 py-0.5 text-[9px] font-medium text-text-muted"
            >
              {items.length}
            </span>
          {/if}
        </div>

        <div class="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto px-2 pt-1">
          {#if items.length > 0}
            {#each items as item (item.id)}
              {@const sessionStatus = item.sessionId
                ? ($sessionStatusMap.get(item.sessionId) ?? null)
                : null}
              {@const pendingDecision = $pendingDecisionByItem.get(item.id) ?? null}
              {@const planningRun = $activePlanningRunByItem.get(item.id) ?? null}
              <WorkItemCard
                {item}
                {sessionStatus}
                {pendingDecision}
                planningSessionId={planningRun?.sessionId ?? null}
                draggable
                onMove={handleMove}
                onStart={handleStart}
                onPlan={handlePlan}
                onOpen={handleOpen}
                onEdit={openWorkItemEditor}
                onDelete={handleDelete}
                onAcceptReview={handleAcceptReview}
                startPending={!!startingItemIds[item.id]}
                planPending={!!planningItemIds[item.id]}
                acceptPending={!!acceptingItemIds[item.id]}
                startError={startErrors[item.id] ?? planErrors[item.id] ?? item.startError ?? null}
              />
            {/each}
          {:else}
            <p class="px-1 py-2 text-xs text-text-muted/50">Empty</p>
          {/if}
        </div>

        <div class="shrink-0 px-2 pb-2 pt-1">
          <AddCardInput onCreate={(title) => handleCreate(title, col)} />
        </div>
      </section>
    {/each}
  </div>
</div>

<WorkItemDeleteDialog
  item={deleteTarget}
  {deleting}
  error={deleteError}
  onCancel={() => {
    if (!deleting) deleteTarget = null;
  }}
  onConfirm={confirmDelete}
/>
