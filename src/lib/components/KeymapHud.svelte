<script lang="ts">
  import { keymapState, activeTree } from "$lib/keymap/store";
  import { registry } from "$lib/commands";
  import type { Bind } from "$lib/bindings";

  interface Props {
    // Optional prompt props — used when onInput command is armed. The
    // keymap HUD and the leader prompt share one surface.
    promptLabel?: string | null;
    promptPlaceholder?: string | null;
    promptValue?: string;
    onPromptInput?: (value: string) => void;
    onPromptSubmit?: () => void;
  }

  let {
    promptLabel = null,
    promptPlaceholder = null,
    promptValue = "",
    onPromptInput,
    onPromptSubmit,
  }: Props = $props();

  let inputEl: HTMLInputElement | undefined = $state();
  let captureEl: HTMLInputElement | undefined = $state();

  // Grab a hidden capture-input so global keydown handlers can route through
  // this surface without losing focus elsewhere. Mirrors the old LeaderHud.
  $effect(() => {
    if (!promptLabel) requestAnimationFrame(() => captureEl?.focus());
  });
  $effect(() => {
    if (promptLabel) requestAnimationFrame(() => inputEl?.focus());
  });

  function actionLabel(bind: Bind): string {
    if (bind.action.kind === "enterTree") return bind.action.tree;
    const cmd = registry.get(bind.action.id);
    return cmd?.label ?? bind.action.id;
  }

  function keyLabel(bind: Bind): string {
    if (bind.key.kind === "character") return bind.key.key;
    // Strip common code prefixes for display (KeyH → h, Digit1 → 1).
    let body = bind.key.code;
    if (body.startsWith("Key")) body = body.slice(3).toLowerCase();
    if (body.startsWith("Digit")) body = body.slice(5);
    return body;
  }

  function isAvailable(bind: Bind): boolean {
    if (bind.action.kind === "enterTree") return true;
    const cmd = registry.get(bind.action.id);
    if (!cmd) return false;
    return !cmd.available || cmd.available();
  }

  const title = $derived($activeTree?.name ?? "Keymap");
  const path = $derived($keymapState.treePath);
  const tree = $derived($activeTree);
  const sticky = $derived(tree?.sticky ?? false);
  const passthrough = $derived(tree?.passthrough ?? false);
  const visibleBinds = $derived((tree?.binds ?? []).filter(isAvailable));
</script>

<input
  bind:this={captureEl}
  type="text"
  aria-hidden="true"
  tabindex="-1"
  class="pointer-events-none fixed -left-[9999px] top-0 h-0 w-0 opacity-0"
/>

<div
  class="pointer-events-none fixed inset-x-0 bottom-4 z-50 flex justify-center px-4"
>
  <div
    class="pointer-events-none flex max-w-[min(1080px,100%)] flex-wrap items-start gap-x-3 gap-y-2 rounded-2xl border border-border-subtle bg-bg-panel/92 px-4 py-3 shadow-[0_16px_40px_rgba(0,0,0,0.22),0_0_0_1px_rgba(255,255,255,0.03)] backdrop-blur-md"
  >
    <div
      class="flex items-center gap-2 text-[11px] uppercase tracking-[0.22em] text-text-muted"
    >
      <span
        class="rounded-full border border-border-subtle bg-bg-surface/70 px-2 py-1 text-text-secondary"
      >
        {title}
      </span>
      {#if sticky}
        <span
          class="rounded-full border {passthrough
            ? 'border-warning/50 text-warning'
            : 'border-accent-dim/30 text-accent'} bg-bg-surface/70 px-2 py-1 tracking-normal"
        >
          {passthrough ? "PASSTHROUGH" : "STICKY"}
        </span>
      {/if}
      {#if path.length > 1}
        <span
          class="rounded-full border border-border-subtle bg-bg-surface/70 px-2 py-1 tracking-normal text-text-secondary"
        >
          {path.join(" › ")}
        </span>
      {/if}
    </div>

    {#if promptLabel}
      <div
        class="pointer-events-auto flex min-w-[min(420px,100%)] flex-1 items-center gap-2 rounded-xl border border-accent-dim/20 bg-bg-surface/70 px-3 py-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]"
      >
        <span class="text-[12px] font-medium text-text-secondary"
          >{promptLabel}</span
        >
        <input
          bind:this={inputEl}
          type="text"
          value={promptValue}
          placeholder={promptPlaceholder ?? ""}
          class="min-w-0 flex-1 border-none bg-transparent text-[13px] text-text-primary outline-none placeholder:text-text-muted"
          oninput={(e) =>
            onPromptInput?.((e.currentTarget as HTMLInputElement).value)}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              onPromptSubmit?.();
            }
          }}
        />
      </div>
    {:else}
      <div
        class="flex min-w-0 flex-1 flex-wrap items-center gap-2 text-[12px] text-text-secondary"
      >
        {#each visibleBinds as bind (keyLabel(bind) + ":" + actionLabel(bind))}
          <span
            class="flex items-center gap-1.5 rounded-xl border border-border-subtle bg-bg-surface/70 px-2.5 py-1 shadow-[inset_0_1px_0_rgba(255,255,255,0.025)]"
          >
            <kbd
              class="rounded border border-accent-dim/20 bg-accent-dim/12 px-1.5 py-0.5 text-[11px] font-mono font-semibold text-accent"
            >
              {keyLabel(bind)}
            </kbd>
            <span>{actionLabel(bind)}</span>
          </span>
        {/each}
        <span class="rounded-xl bg-bg-surface/45 px-2.5 py-1 text-text-muted">
          Esc {sticky ? "exit mode" : "cancel"}
        </span>
      </div>
    {/if}
  </div>
</div>
