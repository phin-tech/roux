<script lang="ts">
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import MoreVertical from "@lucide/svelte/icons/more-vertical";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Terminal from "@lucide/svelte/icons/terminal";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { WorkItem, WorkItemStatus } from "$lib/bindings";
  import type { SessionStatus } from "$lib/types";
  import {
    clearDraggedWorkItem,
    writeWorkItemDragData,
  } from "$lib/board/drag";

  interface Props {
    item: WorkItem;
    sessionStatus?: SessionStatus | null;
    onMove?: (id: string, status: WorkItemStatus) => void;
    onStart?: (id: string, item: WorkItem) => void;
    /** Open the card's bound session (by session id). */
    onOpen?: (sessionId: string) => void;
    /** Open the card editor (by work item id). */
    onEdit?: (id: string) => void;
    /** Delete the card (by work item id). */
    onDelete?: (id: string, item: WorkItem) => void;
    startPending?: boolean;
    startError?: string | null;
    /** Opt-in card dragging. The full-screen board enables it; the sidebar leaves it off. */
    draggable?: boolean;
  }

  const {
    item,
    sessionStatus = null,
    onMove,
    onStart,
    onOpen,
    onEdit,
    onDelete,
    startPending = false,
    startError = null,
    draggable = false,
  }: Props = $props();

  const COLUMN_OPTIONS: WorkItemStatus[] = ["todo", "doing", "review", "done"];
  const COLUMN_LABELS: Record<WorkItemStatus, string> = {
    todo: "To Do",
    doing: "In Progress",
    review: "Review",
    done: "Done",
  };

  const statusDotClasses: Partial<Record<SessionStatus, string>> = {
    idle: "bg-green",
    generating: "bg-blue",
    thinking: "bg-blue",
    attention: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)]",
    error: "bg-red",
    disconnected: "bg-muted",
  };

  const isDispatched = $derived(!!item.sessionId);
  const hasMenuActions = $derived(!!onEdit || !!onDelete);
  const dotClass = $derived(
    sessionStatus ? (statusDotClasses[sessionStatus] ?? "bg-muted") : null,
  );

  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);

  function openMenuAt(clientX: number, clientY: number): void {
    if (!hasMenuActions) return;
    menuX = Math.max(8, clientX);
    menuY = Math.max(8, clientY);
    menuOpen = true;
  }

  function handleMenuButtonClick(event: MouseEvent): void {
    event.stopPropagation();
    const target = event.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    openMenuAt(rect.right - 160, rect.bottom + 4);
  }

  function handleContextMenu(event: MouseEvent): void {
    if (!hasMenuActions) return;
    event.preventDefault();
    openMenuAt(event.clientX, event.clientY);
  }

  function handleEdit(): void {
    menuOpen = false;
    onEdit?.(item.id);
  }

  function handleDelete(): void {
    menuOpen = false;
    onDelete?.(item.id, item);
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") menuOpen = false;
  }
</script>

<svelte:window onclick={() => (menuOpen = false)} onkeydown={handleWindowKeydown} />

<div
  role="presentation"
  class="work-card group relative flex flex-col gap-1.5 px-3 py-2.5 pl-3.5"
  class:cursor-grab={draggable}
  class:active:cursor-grabbing={draggable}
  {draggable}
  data-dispatched={isDispatched}
  data-error={!!startError}
  ondragstart={draggable
    ? (e) => writeWorkItemDragData(e.dataTransfer, item)
    : undefined}
  ondragend={draggable ? clearDraggedWorkItem : undefined}
  oncontextmenu={handleContextMenu}
  data-testid="work-item-card"
  data-item-id={item.id}
>
  <!-- Live status dot -->
  {#if dotClass}
    <span
      class="absolute top-3 flex h-2 w-2"
      class:right-8={hasMenuActions}
      class:right-2={!hasMenuActions}
      role="img"
      aria-label="live status"
    >
      <span
        class="absolute inline-flex h-2 w-2 animate-ping rounded-full opacity-60 {dotClass}"
      ></span>
      <span class="relative inline-flex h-2 w-2 rounded-full {dotClass}"></span>
    </span>
  {/if}

  {#if hasMenuActions}
    <button
      type="button"
      class="absolute right-1.5 top-1.5 z-10 flex h-6 w-6 items-center justify-center rounded text-text-muted/70 opacity-0 transition-colors hover:bg-bg-hover hover:text-text-primary focus:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 group-hover:opacity-100"
      aria-label="Card actions"
      aria-haspopup="menu"
      aria-expanded={menuOpen}
      onclick={handleMenuButtonClick}
    >
      <MoreVertical size={14} strokeWidth={2.1} />
    </button>
  {/if}

  {#if onEdit}
    <button
      type="button"
      class="pr-4 text-left text-[13px] font-semibold leading-snug text-text-primary transition-colors hover:text-accent focus-visible:outline-none"
      onclick={() => onEdit?.(item.id)}
      aria-label="Edit card"
    >
      {item.title}
    </button>
  {:else}
    <p class="pr-4 text-[13px] font-semibold leading-snug text-text-primary">{item.title}</p>
  {/if}

  {#if item.body}
    <p class="line-clamp-2 text-[11px] leading-4 text-text-muted">{item.body}</p>
  {/if}

  {#if startError}
    <p class="text-[11px] leading-snug text-red" role="alert">{startError}</p>
  {/if}

  <div class="flex items-center gap-1.5 pt-0.5">
    <!-- Column quick-move buttons -->
    {#each COLUMN_OPTIONS.filter((c) => c !== item.status) as col (col)}
      <button
        class="inline-flex h-5 items-center gap-1 rounded px-1.5 text-[10px] text-text-muted/80 transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={() => onMove?.(item.id, col)}
        aria-label="Move to {COLUMN_LABELS[col]}"
      >
        <ArrowRight size={10} strokeWidth={2.25} />
        <span>{COLUMN_LABELS[col]}</span>
      </button>
    {/each}

    {#if isDispatched && onOpen}
      <button
        class="ml-auto inline-flex h-6 items-center gap-1.5 rounded-md border border-accent-dim/30 bg-accent-dim/15 px-2 text-[10px] font-semibold text-accent shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors hover:bg-accent-dim/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/60"
        onclick={() => item.sessionId && onOpen?.(item.sessionId)}
        aria-label="Open terminal"
      >
        <Terminal size={11} strokeWidth={2.2} />
        <span>Open terminal</span>
      </button>
    {:else if !isDispatched && onStart}
      <button
        class="ml-auto inline-flex h-6 items-center gap-1.5 rounded-md border border-accent-dim/30 bg-accent-dim/15 px-2 text-[10px] font-semibold text-accent shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-colors hover:bg-accent-dim/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/60 disabled:cursor-wait disabled:opacity-60"
        onclick={() => onStart?.(item.id, item)}
        aria-label="Start work item"
        aria-busy={startPending}
        disabled={startPending}
      >
        <Play size={10} fill="currentColor" strokeWidth={2.2} />
        <span>{startPending ? "Starting..." : "Start"}</span>
      </button>
    {/if}
  </div>
</div>

{#if menuOpen}
  <div
    class="fixed z-[80] min-w-40 rounded-lg border border-border-subtle bg-bg-surface/95 p-1 shadow-[0_18px_48px_rgba(0,0,0,0.45),inset_0_1px_0_rgba(255,255,255,0.06)] backdrop-blur"
    style={`left: ${menuX}px; top: ${menuY}px;`}
    role="menu"
    aria-label="Card actions"
    tabindex="-1"
  >
    {#if onEdit}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={handleEdit}
      >
        <Pencil size={13} strokeWidth={2.1} />
        <span>Edit card</span>
      </button>
    {/if}
    {#if onDelete}
      <button
        type="button"
        role="menuitem"
        class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-red transition-colors hover:bg-red/12 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red/50"
        onclick={handleDelete}
      >
        <Trash2 size={13} strokeWidth={2.1} />
        <span>Delete card</span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .work-card {
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--color-border-subtle) 82%, transparent);
    border-radius: 8px;
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--color-bg-surface) 88%, var(--color-bg-elevated) 12%) 0%,
        color-mix(in srgb, var(--color-bg-surface) 74%, var(--color-bg-deep) 26%) 100%
      );
    box-shadow:
      0 10px 24px rgba(0, 0, 0, 0.22),
      inset 0 1px 0 rgba(255, 255, 255, 0.045);
    transition:
      transform 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease,
      background 140ms ease;
  }

  .work-card::before {
    position: absolute;
    inset: 0 auto 0 0;
    width: 2px;
    content: "";
    background: color-mix(in srgb, var(--color-border) 72%, transparent);
  }

  .work-card::after {
    position: absolute;
    inset: 0;
    pointer-events: none;
    content: "";
    background: linear-gradient(
      120deg,
      rgba(255, 255, 255, 0.055),
      transparent 34%
    );
    opacity: 0.58;
  }

  .work-card:hover {
    transform: translateY(-1px);
    border-color: color-mix(in srgb, var(--color-border) 82%, var(--color-accent) 18%);
    box-shadow:
      0 16px 34px rgba(0, 0, 0, 0.3),
      0 0 0 1px color-mix(in srgb, var(--color-accent-dim) 16%, transparent),
      inset 0 1px 0 rgba(255, 255, 255, 0.06);
  }

  .work-card[data-dispatched="true"]::before {
    background: var(--color-accent);
    box-shadow: 0 0 18px color-mix(in srgb, var(--color-accent) 44%, transparent);
  }

  .work-card[data-error="true"] {
    border-color: color-mix(in srgb, var(--color-red) 42%, var(--color-border));
  }

  .work-card[data-error="true"]::before {
    background: var(--color-red);
    box-shadow: 0 0 18px color-mix(in srgb, var(--color-red) 38%, transparent);
  }

  @media (prefers-reduced-motion: reduce) {
    .work-card {
      transition: border-color 140ms ease, box-shadow 140ms ease;
    }

    .work-card:hover {
      transform: none;
    }
  }
</style>
