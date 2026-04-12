<script lang="ts">
  import type { LeaderHint } from "$lib/commands/leader";

  interface Props {
    title: string;
    sequence: string[];
    hints: LeaderHint[];
    promptLabel?: string | null;
    promptPlaceholder?: string | null;
    promptValue?: string;
    onPromptInput?: (value: string) => void;
    onPromptSubmit?: () => void;
  }

  let {
    title,
    sequence,
    hints,
    promptLabel = null,
    promptPlaceholder = null,
    promptValue = "",
    onPromptInput,
    onPromptSubmit,
  }: Props = $props();
  let inputEl: HTMLInputElement | undefined = $state();

  function formatKey(key: string): string {
    return key === "SPC" ? "Space" : key;
  }

  function formatSequenceKey(key: string): string {
    return key === "space" ? "Space" : key;
  }

  $effect(() => {
    if (promptLabel) {
      requestAnimationFrame(() => inputEl?.focus());
    }
  });
</script>

<div class="pointer-events-none fixed inset-x-0 bottom-4 z-50 flex justify-center px-4">
  <div class="pointer-events-none flex max-w-[min(1080px,100%)] flex-wrap items-start gap-x-3 gap-y-2 rounded-2xl border border-border-subtle bg-bg-panel/92 px-4 py-3 shadow-[0_16px_40px_rgba(0,0,0,0.22),0_0_0_1px_rgba(255,255,255,0.03)] backdrop-blur-md">
    <div class="flex items-center gap-2 text-[11px] uppercase tracking-[0.22em] text-text-muted">
      {#if title !== "Leader"}
        <span class="rounded-full border border-border-subtle bg-bg-surface/70 px-2 py-1 text-text-secondary">
          {title}
        </span>
      {/if}
      {#if sequence.length > 0}
        <span class="rounded-full border border-border-subtle bg-bg-surface/70 px-2 py-1 tracking-normal text-text-secondary">
          {sequence.map(formatSequenceKey).join(" ")}
        </span>
      {/if}
    </div>

    {#if promptLabel}
      <div class="pointer-events-auto flex min-w-[min(420px,100%)] flex-1 items-center gap-2 rounded-xl border border-accent-dim/20 bg-bg-surface/70 px-3 py-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
        <span class="text-[12px] font-medium text-text-secondary">{promptLabel}</span>
        <input
          bind:this={inputEl}
          type="text"
          value={promptValue}
          placeholder={promptPlaceholder ?? ""}
          class="min-w-0 flex-1 border-none bg-transparent text-[13px] text-text-primary outline-none placeholder:text-text-muted"
          oninput={(e) => onPromptInput?.((e.currentTarget as HTMLInputElement).value)}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              onPromptSubmit?.();
            }
          }}
        />
      </div>
    {:else}
      <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2 text-[12px] text-text-secondary">
        {#each hints as hint (hint.key)}
          <span class="flex items-center gap-1.5 rounded-xl border border-border-subtle bg-bg-surface/70 px-2.5 py-1 shadow-[inset_0_1px_0_rgba(255,255,255,0.025)]">
            <kbd class="rounded border border-accent-dim/20 bg-accent-dim/12 px-1.5 py-0.5 text-[11px] font-mono font-semibold text-accent">
              {formatKey(hint.key)}
            </kbd>
            <span>{hint.label}</span>
          </span>
        {/each}
        <span class="rounded-xl bg-bg-surface/45 px-2.5 py-1 text-text-muted">
          Esc cancel
        </span>
      </div>
    {/if}
  </div>
</div>
