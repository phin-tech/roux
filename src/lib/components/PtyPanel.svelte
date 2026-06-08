<script lang="ts">
  import { onDestroy } from "svelte";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { listAllPtys } from "$lib/tauri";
  import type { PtyInfo } from "$lib/types";
  import PinButton from "./PinButton.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();

  let ptys = $state<PtyInfo[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let loaded = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  const POLL_INTERVAL_MS = 2_000;

  async function refresh(): Promise<void> {
    if (loading) return;
    loading = true;
    error = null;
    try {
      ptys = await listAllPtys();
      loaded = true;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (visible && !loaded && !loading) void refresh();
  });

  $effect(() => {
    if (visible) {
      pollTimer ??= setInterval(() => {
        void refresh();
      }, POLL_INTERVAL_MS);
    } else if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  function statusLabel(pty: PtyInfo): string {
    switch (pty.status.type) {
      case "RunningAttached":
        return `attached to ${pty.status.pane_id}`;
      case "RunningDetached":
        return "detached";
      case "Exited":
        return pty.status.code == null
          ? "exited"
          : `exited ${pty.status.code}`;
    }
  }

  function statusClass(pty: PtyInfo): string {
    switch (pty.status.type) {
      case "RunningAttached":
        return "bg-green";
      case "RunningDetached":
        return pty.unread_output
          ? "bg-amber shadow-[0_0_6px_var(--color-amber-dim)]"
          : "bg-blue";
      case "Exited":
        return "bg-text-muted";
    }
  }

  function roleLabel(role: PtyInfo["role"]): string {
    return role === "sessionPrimary" ? "primary" : "secondary";
  }

  function shortId(id: string | null): string {
    return id ? id.slice(0, 8) : "none";
  }
</script>

<div
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <SidebarPanelHeader title="PTYs">
    {#snippet actions()}
      <button
        type="button"
        class="inline-flex h-6 w-6 items-center justify-center rounded border border-transparent text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
        onclick={() => void refresh()}
        disabled={loading}
        aria-label="Refresh PTYs"
        title="Refresh PTYs"
      >
        <RefreshCw size={13} class={loading ? "animate-spin" : ""} />
      </button>
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <CollapseSidebarButton
        onclick={onclose}
        label="Collapse PTYs sidebar"
        title="Collapse PTYs sidebar"
      />
    {/snippet}
  </SidebarPanelHeader>

  <div class="min-h-0 flex-1 overflow-y-auto p-2">
    {#if error}
      <div class="rounded border border-red/30 bg-red/10 p-2 text-xs text-red">
        {error}
      </div>
    {:else if loading && ptys.length === 0}
      <div class="flex h-full items-center justify-center text-sm text-text-muted">
        Loading PTYs...
      </div>
    {:else if ptys.length === 0}
      <div class="flex h-full items-center justify-center text-sm text-text-muted">
        No daemon PTYs
      </div>
    {:else}
      <div class="space-y-1">
        {#each ptys as pty (pty.id)}
          <div
            class="rounded-lg border border-border-subtle/60 bg-bg-surface/35 px-2 py-1.5 text-xs"
          >
            <div class="flex min-w-0 items-center gap-2">
              <span
                class="h-2 w-2 shrink-0 rounded-full {statusClass(pty)}"
              ></span>
              <span class="min-w-0 flex-1 truncate font-medium text-text-primary">
                {pty.name || pty.id}
              </span>
              <span class="shrink-0 text-[10px] uppercase text-text-muted">
                {roleLabel(pty.role)}
              </span>
            </div>
            <div class="mt-1 grid gap-0.5 text-[10px] text-text-muted">
              <div class="truncate">id: {pty.id}</div>
              <div class="truncate">session: {shortId(pty.session_id)}</div>
              <div class="truncate">status: {statusLabel(pty)}</div>
              {#if pty.profile}
                <div class="truncate">profile: {pty.profile}</div>
              {/if}
              {#if pty.working_dir}
                <div class="truncate" title={pty.working_dir}>
                  cwd: {pty.working_dir}
                </div>
              {/if}
              {#if pty.unread_output || pty.bell_pending}
                <div class="text-amber">
                  {pty.unread_output ? "unread output" : ""}
                  {pty.unread_output && pty.bell_pending ? " · " : ""}
                  {pty.bell_pending ? "bell pending" : ""}
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
