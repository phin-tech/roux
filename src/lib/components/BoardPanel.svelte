<script lang="ts">
  import { derived } from "svelte/store";
  import {
    itemsByColumn,
    WORK_ITEM_COLUMNS,
    COLUMN_LABELS,
    moveWorkItem,
    type WorkItemStatus,
  } from "$lib/stores/workItems";
  import { sessionList } from "$lib/stores/sessions";
  import type { SessionStatus } from "$lib/types";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import PinButton from "./PinButton.svelte";
  import WorkItemCard from "./WorkItemCard.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();

  const sessionStatusMap = derived(sessionList, ($sessions) => {
    const m = new Map<string, SessionStatus>();
    for (const s of $sessions) m.set(s.id, s.status);
    return m;
  });

  async function handleMove(id: string, status: WorkItemStatus) {
    const col = $itemsByColumn.get(status) ?? [];
    const lastOrder = col.length > 0 ? col[col.length - 1].sortOrder : -1;
    await moveWorkItem(id, status, lastOrder + 1);
  }

  async function handleStart(id: string) {
    await handleMove(id, "doing");
  }
</script>

<div class="flex h-full w-full min-h-0 flex-col bg-bg-deep" class:hidden={!visible}>
  <SidebarPanelHeader title="Board">
    {#snippet actions()}
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
              <WorkItemCard
                {item}
                {sessionStatus}
                onMove={handleMove}
                onStart={handleStart}
              />
            {/each}
          </div>
        {:else}
          <p class="px-1 text-xs text-text-muted/50">Empty</p>
        {/if}
      </section>
    {/each}
  </div>
</div>
