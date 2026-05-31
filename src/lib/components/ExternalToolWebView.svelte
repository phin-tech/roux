<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Webview } from "@tauri-apps/api/webview";
  import { getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
  import type { ExternalToolRun } from "$lib/stores/externalTools";
  import {
    markExternalToolExited,
    markExternalToolReady,
    readExternalToolProcess,
    restartExternalToolRun,
    setExternalToolRunError,
  } from "$lib/stores/externalTools";

  interface Props {
    run: ExternalToolRun;
  }

  let { run }: Props = $props();
  let host = $state<HTMLDivElement | null>(null);
  let logs = $state("");
  let outputTruncated = $state(false);
  let webview: Webview | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let logTimer: ReturnType<typeof setInterval> | null = null;
  let startedPollingAt = 0;

  onMount(() => {
    resizeObserver = new ResizeObserver(() => void positionWebview());
    if (host) resizeObserver.observe(host);
    startPolling();
    logTimer = setInterval(() => void refreshLogs(), 1500);
    void refreshLogs();
    return () => cleanup();
  });

  $effect(() => {
    run.logsOpen;
    void refreshLogs();
    void positionWebview();
  });

  onDestroy(cleanup);

  function startPolling(): void {
    clearPoll();
    startedPollingAt = Date.now();
    const tick = async () => {
      if (!run.rendered?.url) return;
      try {
        await fetch(run.rendered.url, { method: "GET", mode: "no-cors", cache: "no-store" });
        markExternalToolReady(run.id);
        await createWebview();
        return;
      } catch {
        const snapshot = await readExternalToolProcess(run).catch(() => null);
        if (snapshot && !snapshot.record.running) {
          markExternalToolExited(run.id, snapshot.record.id, snapshot.record.exitCode);
          logs = snapshot.output;
          outputTruncated = snapshot.record.outputTruncated;
          return;
        }
        if (Date.now() - startedPollingAt > 15_000) {
          setExternalToolRunError(run.id, `Timed out waiting for ${run.rendered.url}`);
          return;
        }
      }
      pollTimer = setTimeout(tick, 500);
    };
    pollTimer = setTimeout(tick, 0);
  }

  async function createWebview(): Promise<void> {
    if (webview || !host || !run.rendered?.url) return;
    const rect = host.getBoundingClientRect();
    const label = `external-tool-${run.id.replace(/[^a-zA-Z0-9-/:_]/g, "_")}`;
    webview = new Webview(getCurrentWindow(), label, {
      url: run.rendered.url,
      x: rect.left,
      y: rect.top,
      width: Math.max(1, rect.width),
      height: Math.max(1, rect.height),
      focus: true,
    });
    await positionWebview();
  }

  async function positionWebview(): Promise<void> {
    if (!webview || !host) return;
    const rect = host.getBoundingClientRect();
    await webview.setPosition(new LogicalPosition(rect.left, rect.top));
    await webview.setSize(new LogicalSize(Math.max(1, rect.width), Math.max(1, rect.height)));
  }

  async function refreshLogs(): Promise<void> {
    const snapshot = await readExternalToolProcess(run).catch(() => null);
    if (!snapshot) return;
    if (run.logsOpen || run.status === "error" || run.status === "exited") {
      logs = snapshot.output;
      outputTruncated = snapshot.record.outputTruncated;
    }
    if (!snapshot.record.running && run.status !== "exited") {
      markExternalToolExited(run.id, snapshot.record.id, snapshot.record.exitCode);
    }
  }

  function clearPoll(): void {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = null;
  }

  function cleanup(): void {
    clearPoll();
    if (logTimer) clearInterval(logTimer);
    logTimer = null;
    resizeObserver?.disconnect();
    resizeObserver = null;
    const current = webview;
    webview = null;
    void current?.close();
  }
</script>

<div class="flex h-full min-h-0 flex-col bg-bg-base">
  <div class="relative min-h-0 flex-1">
    {#if run.status === "starting" || run.status === "launching"}
      <div class="absolute inset-0 z-10 flex items-center justify-center text-sm text-text-muted">
        Loading {run.rendered?.url ?? run.toolName}...
      </div>
    {:else if run.status === "exited"}
      <div class="absolute inset-0 z-10 flex items-center justify-center bg-bg-base/90 p-6">
        <div class="rounded border border-border-subtle bg-bg-surface/80 p-4 text-sm text-text-secondary">
          {run.toolName} exited{run.exitCode == null ? "" : ` with code ${run.exitCode}`}.
          <button
            type="button"
            class="ml-3 rounded border border-border-subtle bg-bg-elevated px-3 py-1 text-xs text-text-primary hover:bg-bg-hover"
            onclick={() => void restartExternalToolRun(run.id)}
          >
            Relaunch
          </button>
        </div>
      </div>
    {/if}
    <div bind:this={host} class="h-full w-full bg-bg-deep"></div>
  </div>

  {#if run.logsOpen || run.status === "error" || run.status === "exited"}
    <div class="h-40 shrink-0 border-t border-border-subtle bg-bg-deep">
      <div class="flex h-7 items-center justify-between border-b border-hairline px-3 text-[11px] text-text-muted">
        <span>Process Logs</span>
        {#if outputTruncated}<span>truncated</span>{/if}
      </div>
      <pre class="app-scrollbar h-[calc(100%-1.75rem)] overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-[11px] text-text-secondary">{logs || "No output yet."}</pre>
    </div>
  {/if}
</div>
