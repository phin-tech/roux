<script lang="ts">
  interface Props {
    /** Create a card with this title. Resolves when persisted. */
    onCreate: (title: string) => void | Promise<void>;
    placeholder?: string;
  }

  let { onCreate, placeholder = "Card title…" }: Props = $props();

  let open = $state(false);
  let title = $state("");
  let inputEl = $state<HTMLInputElement | null>(null);

  // Focus the field whenever it opens.
  $effect(() => {
    if (open) inputEl?.focus();
  });

  async function submit() {
    const trimmed = title.trim();
    if (!trimmed) return;
    title = "";
    await onCreate(trimmed);
    // Stay open + focused so several cards can be added in a row.
    inputEl?.focus();
  }

  function cancel() {
    title = "";
    open = false;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      void submit();
    } else if (event.key === "Escape") {
      event.preventDefault();
      cancel();
    }
  }

  function onBlur() {
    // Collapse only when nothing's been typed, so a stray blur mid-edit
    // doesn't discard work.
    if (!title.trim()) open = false;
  }
</script>

{#if open}
  <input
    bind:this={inputEl}
    bind:value={title}
    type="text"
    {placeholder}
    class="w-full rounded border border-border bg-surface-1 px-2 py-1 text-sm text-text outline-none focus:border-accent"
    aria-label="New card title"
    onkeydown={onKeydown}
    onblur={onBlur}
  />
{:else}
  <button
    type="button"
    class="w-full rounded px-2 py-1 text-left text-[11px] text-text-muted/70 transition-colors hover:bg-surface-2 hover:text-text"
    onclick={() => (open = true)}
    aria-label="Add card"
  >
    + Add card
  </button>
{/if}
