<script lang="ts">
  import { onMount } from "svelte";
  import DoctorPanel from "$lib/components/DoctorPanel.svelte";
  import { getLogPath, setLoggingEnabled } from "$lib/logging";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { getRuntimeStatus, type RuntimeStatus } from "$lib/tauri";

  let runtimeStatus = $state<RuntimeStatus | null>(null);
  let runtimeStatusError = $state<string | null>(null);
  let runtimeStatusLoading = $state(false);
  let runtimeStatusRun = 0;

  onMount(() => {
    const run = ++runtimeStatusRun;
    void refreshRuntimeStatus(run);
  });

  async function refreshRuntimeStatus(run: number) {
    runtimeStatusLoading = true;
    runtimeStatusError = null;
    try {
      const result = await getRuntimeStatus();
      if (run === runtimeStatusRun) runtimeStatus = result;
    } catch (e) {
      if (run === runtimeStatusRun) {
        runtimeStatus = null;
        runtimeStatusError = e instanceof Error ? e.message : String(e);
      }
    } finally {
      if (run === runtimeStatusRun) runtimeStatusLoading = false;
    }
  }

  function runtimeModeLabel(status: RuntimeStatus): string {
    return status.mode === "daemon" ? "Daemon" : "Local fallback";
  }

  function formatTimestamp(ms: number): string {
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) return String(ms);
    return date.toLocaleString();
  }

  function formatDuration(ms: number): string {
    const totalSeconds = Math.max(0, Math.floor(ms / 1000));
    const days = Math.floor(totalSeconds / 86_400);
    const hours = Math.floor((totalSeconds % 86_400) / 3_600);
    const minutes = Math.floor((totalSeconds % 3_600) / 60);
    const seconds = totalSeconds % 60;
    if (days > 0) return `${days}d ${hours}h ${minutes}m`;
    if (hours > 0) return `${hours}h ${minutes}m`;
    if (minutes > 0) return `${minutes}m ${seconds}s`;
    return `${seconds}s`;
  }
</script>

<div
  data-testid="runtime-debug-panel"
  class="mb-5 rounded-lg border border-border-subtle bg-bg-surface/30 p-3"
>
  <div class="flex items-center justify-between gap-3">
    <div>
      <div class="text-[13px] font-semibold text-text-primary">Runtime</div>
      <div class="mt-0.5 text-[11px] text-text-muted">
        {#if runtimeStatus}
          {runtimeModeLabel(runtimeStatus)}
        {:else if runtimeStatusLoading}
          Checking…
        {:else}
          Unavailable
        {/if}
      </div>
    </div>
    <button
      aria-label="Refresh runtime status"
      class="rounded border border-border px-2.5 py-1 text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-50"
      disabled={runtimeStatusLoading}
      onclick={() => {
        const run = ++runtimeStatusRun;
        void refreshRuntimeStatus(run);
      }}
    >
      Refresh
    </button>
  </div>

  {#if runtimeStatusError}
    <div class="mt-3 text-[11px] text-red">{runtimeStatusError}</div>
  {:else if runtimeStatus}
    <div
      class="mt-3 grid grid-cols-[100px_minmax(0,1fr)] gap-x-3 gap-y-1 text-[11px]"
    >
      <div class="text-text-muted">Mode</div>
      <div class="text-text-primary">{runtimeModeLabel(runtimeStatus)}</div>

      <div class="text-text-muted">Desktop PID</div>
      <div class="font-mono text-text-secondary">
        pid {runtimeStatus.desktopPid}
      </div>

      <div class="text-text-muted">Started</div>
      <div class="text-text-secondary">
        {formatTimestamp(runtimeStatus.startedAtMs)}
      </div>

      <div class="text-text-muted">Uptime</div>
      <div class="text-text-secondary">
        {formatDuration(runtimeStatus.uptimeMs)}
      </div>

      {#if runtimeStatus.daemon}
        <div class="text-text-muted">Daemon PID</div>
        <div class="font-mono text-text-secondary">
          pid {runtimeStatus.daemon.pid}
        </div>

        <div class="text-text-muted">Socket</div>
        <div class="break-all font-mono text-text-secondary">
          {runtimeStatus.daemon.socket}
        </div>

        {#if runtimeStatus.daemon.logPath}
          <div class="text-text-muted">Daemon log</div>
          <div class="break-all font-mono text-text-secondary">
            {runtimeStatus.daemon.logPath}
          </div>
        {/if}

        <div class="text-text-muted">State</div>
        <div class="text-text-secondary">
          {runtimeStatus.daemon.sessionCount} sessions,
          {runtimeStatus.daemon.projectCount} projects,
          {runtimeStatus.daemon.watchCount ?? 0} watches,
          {runtimeStatus.daemon.processCount ?? 0} processes,
          {runtimeStatus.daemon.ptyCount ?? 0} PTYs
        </div>
      {:else if runtimeStatus.local}
        <div class="text-text-muted">State</div>
        <div class="text-text-secondary">
          {runtimeStatus.local.sessionCount} sessions,
          {runtimeStatus.local.projectCount} projects,
          {runtimeStatus.local.watchCount} watches,
          {runtimeStatus.local.processCount} processes,
          {runtimeStatus.local.ptyCount} PTYs
        </div>
      {/if}

      {#if runtimeStatus.statusError}
        <div class="text-text-muted">Status error</div>
        <div class="break-all text-amber">{runtimeStatus.statusError}</div>
      {/if}
    </div>
  {/if}
</div>

<div class="mt-4 flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Enable logging</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Write logs to disk for debugging
    </div>
  </div>
  <button
    aria-label="Toggle logging"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {$settings.enableLogging
      ? 'bg-accent-dim border-accent'
      : 'bg-bg-deep border-border'}"
    onclick={() => {
      const next = !$settings.enableLogging;
      setLoggingEnabled(next);
      updateSetting("enableLogging", next);
    }}
  >
    <div
      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {$settings.enableLogging
        ? 'left-[18px] bg-accent'
        : 'left-0.5 bg-text-secondary'}"
    ></div>
  </button>
</div>
{#if $settings.enableLogging}
  <div class="text-[11px] text-text-muted font-mono mt-1 break-all">
    {getLogPath()}
  </div>
{/if}

<div class="mt-6 border-t border-hairline pt-5">
  <DoctorPanel mode="settings" />
</div>
