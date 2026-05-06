<script lang="ts">
  import type { SmolMachine } from "$lib/bindings";
  import Check from "@lucide/svelte/icons/check";
  import Link from "@lucide/svelte/icons/link";
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import Trash from "@lucide/svelte/icons/trash";

  interface Props {
    machine: SmolMachine;
    busy: boolean;
    /**
     * `true` when the active session is already bound to this machine.
     * Renders the assign button as a checkmark/disabled, not a fresh
     * Link icon, so the user can see at-a-glance which machine the
     * active session lives in.
     */
    boundToActive: boolean;
    /**
     * `true` when there is an active session at all. When false, the
     * assign button is disabled with a tooltip explaining that there's
     * nothing to assign to.
     */
    hasActiveSession: boolean;
    onStart: () => void;
    onStop: () => void;
    onDelete: () => void;
    onAssign: () => void;
  }

  let {
    machine,
    busy,
    boundToActive,
    hasActiveSession,
    onStart,
    onStop,
    onDelete,
    onAssign,
  }: Props = $props();

  // smolvm reports state as a free-form lowercase string. Treat anything
  // matching "running" / "starting" as "the machine is up" for the action
  // affordance and the status pill colour.
  let isRunning = $derived(/running|starting/i.test(machine.state));

  function formatMemory(mib: number | null): string | null {
    if (mib == null) return null;
    if (mib >= 1024) return `${(mib / 1024).toFixed(1)} GiB`;
    return `${mib} MiB`;
  }

  let memLabel = $derived(formatMemory(machine.memoryMib));
</script>

<div
  class="flex items-center gap-2 border-b border-hairline px-3 py-2 text-[11px] text-text-primary hover:bg-bg-hover"
>
  <div class="flex min-w-0 flex-1 flex-col gap-0.5">
    <div class="flex items-center gap-2">
      <span class="truncate font-mono text-[12px] font-semibold">{machine.name}</span>
      <span
        class="rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider {isRunning
          ? 'bg-green/10 text-green'
          : 'bg-bg-surface text-text-muted'}"
      >
        {machine.state}
      </span>
      {#if machine.ephemeral}
        <span
          class="rounded bg-bg-surface px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-text-muted"
          title="Ephemeral — auto-cleaned when stopped"
        >
          ephemeral
        </span>
      {/if}
    </div>
    <div class="flex items-center gap-2 text-[10px] text-text-muted">
      {#if machine.image}
        <span class="truncate">{machine.image}</span>
      {/if}
      {#if machine.cpus != null}
        <span>·</span>
        <span>{machine.cpus} CPU</span>
      {/if}
      {#if memLabel}
        <span>·</span>
        <span>{memLabel}</span>
      {/if}
    </div>
  </div>
  <div class="flex shrink-0 items-center gap-1">
    <button
      type="button"
      class="flex h-6 w-6 items-center justify-center rounded disabled:opacity-40 {boundToActive
        ? 'bg-accent-dim/30 text-accent ring-1 ring-accent/40'
        : 'text-text-secondary hover:bg-bg-surface hover:text-text-primary'}"
      title={boundToActive
        ? "Active session is already bound to this machine"
        : hasActiveSession
          ? "Assign active session to this machine"
          : "Open a session first to assign it to a machine"}
      aria-label="Assign active session"
      aria-pressed={boundToActive}
      disabled={busy || !hasActiveSession || boundToActive}
      onclick={onAssign}
    >
      {#if boundToActive}
        <Check size={12} />
      {:else}
        <Link size={12} />
      {/if}
    </button>
    {#if isRunning}
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-secondary hover:bg-bg-surface hover:text-text-primary disabled:opacity-40"
        title="Stop machine"
        aria-label="Stop machine"
        disabled={busy}
        onclick={onStop}
      >
        <Square size={12} />
      </button>
    {:else}
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-secondary hover:bg-bg-surface hover:text-text-primary disabled:opacity-40"
        title="Start machine"
        aria-label="Start machine"
        disabled={busy}
        onclick={onStart}
      >
        <Play size={12} />
      </button>
    {/if}
    <button
      type="button"
      class="flex h-6 w-6 items-center justify-center rounded text-red/80 hover:bg-red/10 hover:text-red disabled:opacity-40"
      title="Delete machine"
      aria-label="Delete machine"
      disabled={busy}
      onclick={onDelete}
    >
      <Trash size={12} />
    </button>
  </div>
</div>
