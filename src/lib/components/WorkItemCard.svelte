<script lang="ts">
  import type { WorkItem, WorkItemStatus } from "$lib/bindings";
  import type { SessionStatus } from "$lib/types";

  interface Props {
    item: WorkItem;
    sessionStatus?: SessionStatus | null;
    onMove?: (id: string, status: WorkItemStatus) => void;
    onStart?: (id: string) => void;
  }

  const { item, sessionStatus = null, onMove, onStart }: Props = $props();

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
  const dotClass = $derived(
    sessionStatus ? (statusDotClasses[sessionStatus] ?? "bg-muted") : null,
  );
</script>

<div
  class="group relative flex flex-col gap-1.5 rounded-md border border-border bg-surface-1 px-3 py-2.5 shadow-sm transition-shadow hover:shadow-md"
  data-testid="work-item-card"
  data-item-id={item.id}
>
  <!-- Live status dot -->
  {#if dotClass}
    <span class="absolute right-2 top-2 flex h-2 w-2" aria-label="live status">
      <span
        class="absolute inline-flex h-2 w-2 animate-ping rounded-full opacity-60 {dotClass}"
      ></span>
      <span class="relative inline-flex h-2 w-2 rounded-full {dotClass}"></span>
    </span>
  {/if}

  <p class="pr-4 text-sm font-medium leading-snug text-text">{item.title}</p>

  {#if item.body}
    <p class="line-clamp-2 text-xs text-text-muted">{item.body}</p>
  {/if}

  <div class="flex items-center gap-1.5 pt-0.5">
    <!-- Column quick-move buttons -->
    {#each COLUMN_OPTIONS.filter((c) => c !== item.status) as col (col)}
      <button
        class="rounded px-1.5 py-0.5 text-[10px] text-text-muted transition-colors hover:bg-surface-2 hover:text-text"
        onclick={() => onMove?.(item.id, col)}
        aria-label="Move to {COLUMN_LABELS[col]}"
      >
        → {COLUMN_LABELS[col]}
      </button>
    {/each}

    {#if !isDispatched && onStart}
      <button
        class="ml-auto rounded bg-accent/10 px-2 py-0.5 text-[10px] font-medium text-accent transition-colors hover:bg-accent/20"
        onclick={() => onStart?.(item.id)}
        aria-label="Start work item"
      >
        Start
      </button>
    {/if}
  </div>
</div>
