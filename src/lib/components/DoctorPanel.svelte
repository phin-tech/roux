<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import {
    checkDoctorStatus,
    installAllMissing,
    reinstallCli,
    reinstallHooks,
    reinstallSkill,
  } from "$lib/tauri";
  import type { DoctorItem, DoctorStatus } from "$lib/bindings";

  interface Props {
    /** "onboarding" shows as a modal with welcome header + install-all button.
     *  "settings" renders inline with just the rows. */
    mode: "onboarding" | "settings";
    /** Whether the onboarding modal should be visible. Ignored in settings mode. */
    visible?: boolean;
    /** Called when the user dismisses onboarding (Skip or after a successful
     *  install-all). Ignored in settings mode. */
    ondone?: () => void;
  }

  let { mode, visible = true, ondone }: Props = $props();

  let loading = $state(true);
  let status = $state<DoctorStatus | null>(null);
  let busy = $state<string | null>(null);
  let error = $state("");

  async function refresh() {
    loading = true;
    error = "";
    try {
      status = await checkDoctorStatus();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  const installers: Record<string, () => Promise<void>> = {
    cli: reinstallCli,
    hooks: reinstallHooks,
    skill: reinstallSkill,
  };

  async function handleReinstall(item: DoctorItem) {
    const runner = installers[item.id];
    if (!runner) return;
    busy = item.id;
    error = "";
    try {
      await runner();
      await refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = null;
    }
  }

  async function handleInstallAll() {
    busy = "all";
    error = "";
    try {
      await installAllMissing();
      await refresh();
      ondone?.();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = null;
    }
  }

  function statusLabel(s: string) {
    switch (s) {
      case "installed":
        return "Installed";
      case "missing":
        return "Not installed";
      case "unavailable":
        return "Not found";
      default:
        return s;
    }
  }

  function statusColor(s: string) {
    switch (s) {
      case "installed":
        return "text-green-400";
      case "missing":
        return "text-amber";
      case "unavailable":
        return "text-text-muted";
      default:
        return "text-text-secondary";
    }
  }

  const hasMissingInstallable = $derived(
    status?.items.some((i) => i.installable && i.status !== "installed") ?? false,
  );

  $effect(() => {
    if (mode === "settings" || visible) {
      void refresh();
    }
  });
</script>

{#snippet rows()}
  <div class="flex flex-col divide-y divide-hairline">
    {#if loading}
      <div class="py-4 text-sm text-text-muted">Checking…</div>
    {:else if !status}
      <div class="py-4 text-sm text-red-400">{error || "Failed to load status"}</div>
    {:else}
      {#each status.items as item (item.id)}
        <div class="flex items-center justify-between gap-3 py-3">
          <div class="min-w-0 flex-1">
            <div class="text-sm font-medium text-text-primary">{item.label}</div>
            <div class="flex items-center gap-2 text-xs">
              <span class={statusColor(item.status)}>{statusLabel(item.status)}</span>
              {#if item.detail}
                <span class="truncate text-text-muted" title={item.detail}>— {item.detail}</span>
              {/if}
            </div>
          </div>
          {#if item.installable}
            <button
              class="ui-btn-ghost shrink-0 rounded-lg px-3 py-1.5 text-xs"
              onclick={() => handleReinstall(item)}
              disabled={busy !== null}
            >
              {busy === item.id
                ? "Installing…"
                : item.status === "installed"
                  ? "Reinstall"
                  : "Install"}
            </button>
          {/if}
        </div>
      {/each}
    {/if}
    {#if error && !loading}
      <div class="py-2 text-xs text-red-400">{error}</div>
    {/if}
  </div>
{/snippet}

{#if mode === "settings"}
  <div class="flex flex-col gap-2">
    <div class="flex items-baseline justify-between">
      <h3 class="text-sm font-semibold text-text-primary">Doctor</h3>
      <button
        class="text-xs text-text-muted hover:text-text-primary"
        onclick={refresh}
        disabled={loading || busy !== null}
      >
        Refresh
      </button>
    </div>
    <p class="text-xs text-text-muted">
      Status of Roux's integrations. Reinstall if something looks stale.
    </p>
    {@render rows()}
  </div>
{:else if visible}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-md"
    transition:fade={{ duration: 150 }}
  >
    <div
      class="ui-dialog w-[480px] rounded-2xl"
      transition:scale={{ duration: 150, start: 0.96 }}
    >
      <div class="border-b border-hairline bg-bg-surface/30 px-6 pt-5 pb-4">
        <div
          class="mb-3 flex h-10 w-10 items-center justify-center rounded-xl border border-border-subtle bg-bg-surface/80 text-accent"
        >
          <span class="text-xl">&#9095;</span>
        </div>
        <h2 class="mb-1 text-base font-semibold tracking-tight text-text-primary">
          Welcome to Roux
        </h2>
        <p class="text-xs text-text-muted">
          Let's get the Claude Code integrations set up
        </p>
      </div>

      <div class="px-6 py-5">
        {@render rows()}
      </div>

      <div class="flex items-center justify-end gap-2 border-t border-hairline px-6 py-4">
        <button
          class="ui-btn-ghost rounded-lg px-4 py-2 text-sm"
          onclick={() => ondone?.()}
          disabled={busy !== null}
        >
          {hasMissingInstallable ? "Skip for now" : "Close"}
        </button>
        {#if hasMissingInstallable}
          <button
            class="ui-btn-primary rounded-lg px-4 py-2 text-sm font-medium"
            onclick={handleInstallAll}
            disabled={busy !== null}
          >
            {busy === "all" ? "Installing…" : "Install all"}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}
