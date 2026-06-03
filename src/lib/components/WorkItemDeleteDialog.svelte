<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import type { WorkItem } from "$lib/bindings";
  import type { WorkItemDeleteMode } from "$lib/workItems/deleteFlow";

  interface Props {
    item: WorkItem | null;
    deleting?: boolean;
    error?: string | null;
    onCancel: () => void;
    onConfirm: (mode: WorkItemDeleteMode) => void;
  }

  let {
    item,
    deleting = false,
    error = null,
    onCancel,
    onConfirm,
  }: Props = $props();

  function onKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && !deleting) {
      event.preventDefault();
      onCancel();
    }
  }
</script>

{#if item}
  <div
    class="fixed inset-0 z-[90] flex items-center justify-center bg-black/60 px-4 backdrop-blur-sm"
    transition:fade={{ duration: 120 }}
    onkeydown={onKeydown}
    role="dialog"
    aria-modal="true"
    aria-label="Delete card"
    tabindex="-1"
  >
    <div
      class="w-[min(420px,100%)] rounded-2xl border border-border bg-bg-surface shadow-[0_24px_80px_rgba(0,0,0,0.55),inset_0_1px_0_rgba(255,255,255,0.06)]"
      transition:scale={{ duration: 120, start: 0.98 }}
    >
      <div class="border-b border-hairline px-5 py-4">
        <h2 class="text-[15px] font-semibold text-text-primary">Delete card</h2>
        <p class="mt-1 text-[12px] leading-5 text-text-muted">{item.title}</p>
      </div>

      <div class="space-y-3 px-5 py-4">
        {#if item.sessionId}
          <p class="text-[13px] leading-5 text-text-secondary">
            This card is linked to a terminal session. Choose whether to keep
            that terminal running or stop it with the card.
          </p>
        {:else}
          <p class="text-[13px] leading-5 text-text-secondary">
            This removes the card from the board.
          </p>
        {/if}

        {#if error}
          <p
            class="rounded-md border border-red/30 bg-red/10 px-3 py-2 text-[12px] text-red"
            role="alert"
          >
            {error}
          </p>
        {/if}
      </div>

      <div
        class="flex flex-wrap justify-end gap-2 border-t border-hairline px-5 py-4"
      >
        <button
          type="button"
          class="cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-4 py-2 text-[13px] font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary disabled:opacity-50"
          onclick={onCancel}
          disabled={deleting}
        >
          Cancel
        </button>
        <button
          type="button"
          class="cursor-pointer rounded-xl border border-red/25 bg-red/12 px-4 py-2 text-[13px] font-medium text-red hover:bg-red/20 disabled:opacity-50"
          onclick={() => onConfirm("card-only")}
          disabled={deleting}
        >
          {deleting ? "Deleting..." : "Delete card only"}
        </button>
        {#if item.sessionId}
          <button
            type="button"
            class="cursor-pointer rounded-xl border border-red/40 bg-red/20 px-4 py-2 text-[13px] font-semibold text-red hover:bg-red/30 disabled:opacity-50"
            onclick={() => onConfirm("card-and-stop-session")}
            disabled={deleting}
          >
            Delete card and stop terminal
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}
