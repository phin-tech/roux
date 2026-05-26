<script lang="ts">
  import { derived } from "svelte/store";
  import {
    itemsByColumn,
    WORK_ITEM_COLUMNS,
    COLUMN_LABELS,
    moveWorkItem,
    dispatchWorkItem,
    createWorkItem,
    pendingDecisionByItem,
    type WorkItemStatus,
  } from "$lib/stores/workItems";
  import { sessionList } from "$lib/stores/sessions";
  import type { SessionStatus } from "$lib/types";
  import Maximize2 from "@lucide/svelte/icons/maximize-2";
  import { openBoardFullscreen, openWorkItemEditor, openWorkItemSessionStart } from "$lib/stores/ui";
  import { openSessionById } from "$lib/panes/openSession";
  import { formatWorkItemStartError } from "$lib/board/startErrors";
  import {
    deleteWorkItemWithMode,
    type WorkItemDeleteMode,
  } from "$lib/workItems/deleteFlow";
  import type { WorkItem } from "$lib/bindings";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import PinButton from "./PinButton.svelte";
  import WorkItemCard from "./WorkItemCard.svelte";
  import AddCardInput from "./AddCardInput.svelte";
  import WorkItemDeleteDialog from "./WorkItemDeleteDialog.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();
  let startingItemIds = $state<Record<string, boolean>>({});
  let startErrors = $state<Record<string, string>>({});
  let deleteTarget = $state<WorkItem | null>(null);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);

  const sessionStatusMap = derived(sessionList, ($sessions) => {
    const m = new Map<string, SessionStatus>();
    for (const s of $sessions) m.set(s.id, s.status);
    return m;
  });

  async function handleMove(id: string, status: WorkItemStatus) {
    await moveWorkItem(id, status, Date.now());
  }

  function withoutKey<T>(record: Record<string, T>, key: string): Record<string, T> {
    const { [key]: _removed, ...rest } = record;
    return rest;
  }

  async function handleStart(id: string, item: WorkItem) {
    if (!item.projectId) {
      openWorkItemSessionStart({ itemId: item.id, title: item.title });
      return;
    }
    if (startingItemIds[id]) return;
    startingItemIds = { ...startingItemIds, [id]: true };
    startErrors = withoutKey(startErrors, id);

    // Dispatch creates + binds a session and atomically moves the card to Doing.
    try {
      await dispatchWorkItem(id);
    } catch (err) {
      startErrors = { ...startErrors, [id]: formatWorkItemStartError(err) };
      console.error("Failed to dispatch work item", err);
    } finally {
      startingItemIds = withoutKey(startingItemIds, id);
    }
  }

  async function handleOpen(sessionId: string) {
    const result = await openSessionById(sessionId);
    if (result === "gone") {
      console.error(`Session ${sessionId} is no longer running`);
    }
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

  async function handleCreate(title: string, status: WorkItemStatus) {
    // The card lands in the store via the broadcast `created` event.
    await createWorkItem({ title, status, sortOrder: Date.now() });
  }
</script>

<div class="flex h-full w-full min-h-0 flex-col bg-bg-deep" class:hidden={!visible}>
  <SidebarPanelHeader title="Board">
    {#snippet actions()}
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-muted transition-colors hover:bg-surface-2 hover:text-text"
        onclick={openBoardFullscreen}
        aria-label="Open board fullscreen"
        title="Open board fullscreen"
      >
        <Maximize2 size={14} />
      </button>
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <CollapseSidebarButton
        onclick={onclose}
        label="Collapse board sidebar"
        title="Collapse board sidebar"
      />
    {/snippet}
  </SidebarPanelHeader>

  <div class="flex flex-1 min-h-0 flex-col overflow-y-auto px-2 py-2 gap-4">
    {#each WORK_ITEM_COLUMNS as col (col)}
      {@const items = $itemsByColumn.get(col) ?? []}
      <section>
        <div class="mb-1.5 flex items-center gap-1.5 px-1">
          <span class="text-[11px] font-semibold uppercase tracking-wide text-text-muted">
            {COLUMN_LABELS[col]}
          </span>
          {#if items.length > 0}
            <span class="rounded-full bg-surface-2 px-1.5 py-0.5 text-[9px] font-medium text-text-muted">
              {items.length}
            </span>
          {/if}
        </div>
        {#if items.length > 0}
          <div class="flex flex-col gap-1.5">
            {#each items as item (item.id)}
              {@const sessionStatus = item.sessionId ? ($sessionStatusMap.get(item.sessionId) ?? null) : null}
              {@const pendingDecision = $pendingDecisionByItem.get(item.id) ?? null}
              <WorkItemCard
                {item}
                {sessionStatus}
                {pendingDecision}
                onMove={handleMove}
                onStart={handleStart}
                onOpen={handleOpen}
                onEdit={openWorkItemEditor}
                onDelete={handleDelete}
                startPending={!!startingItemIds[item.id]}
                startError={startErrors[item.id] ?? null}
              />
            {/each}
          </div>
        {:else}
          <p class="px-1 text-xs text-text-muted/50">Empty</p>
        {/if}
        <div class="mt-1.5">
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
