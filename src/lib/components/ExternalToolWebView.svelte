<script module lang="ts">
  let nextNativeWebviewLabelId = 0;

  function nativeWebviewLabel(snapshot: {
    runId: string;
    runtimeId: string | null;
    launchedAtMs: number;
  }): string {
    return `external-tool-${snapshot.runId}-${snapshot.runtimeId ?? snapshot.launchedAtMs}-${++nextNativeWebviewLabelId}`.replace(
      /[^a-zA-Z0-9-/:_]/g,
      "_",
    );
  }
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Webview } from "@tauri-apps/api/webview";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { probeExternalToolUrl } from "$lib/tauri";
  import type { ExternalToolRun } from "$lib/stores/externalTools";
  import {
    failExternalToolRun,
    markExternalToolExited,
    markExternalToolReady,
    readExternalToolProcess,
    registerExternalToolViewCloser,
  } from "$lib/stores/externalTools";

  interface Props {
    run: ExternalToolRun;
  }

  interface WebviewBounds {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  interface PollSnapshot {
    key: string;
    run: ExternalToolRun;
    runId: string;
    runtimeId: string | null;
    url: string;
    webEmbedder: ExternalToolRun["webEmbedder"];
    launchedAtMs: number;
  }

  let { run }: Props = $props();
  let host = $state<HTMLDivElement | null>(null);
  let logs = $state("");
  let outputTruncated = $state(false);
  let webview: Webview | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let cleanupWindowResize: UnlistenFn | null = null;
  let cleanupWindowScale: UnlistenFn | null = null;
  let resizeFrame: ReturnType<typeof requestAnimationFrame> | null = null;
  let webviewLabel: string | null = null;
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let logTimer: ReturnType<typeof setInterval> | null = null;
  let creatingWebviewKey: string | null = null;
  let destroyed = false;
  let pollKey = "";

  onMount(() => {
    destroyed = false;
    resizeObserver = new ResizeObserver(() => schedulePositionWebview());
    if (host) resizeObserver.observe(host);
    const appWindow = getCurrentWindow();
    void appWindow
      .onResized(() => schedulePositionWebview())
      .then((unlisten) => {
        if (destroyed) {
          unlisten();
          return;
        }
        cleanupWindowResize?.();
        cleanupWindowResize = unlisten;
      })
      .catch(() => {});
    void appWindow
      .onScaleChanged(() => schedulePositionWebview())
      .then((unlisten) => {
        if (destroyed) {
          unlisten();
          return;
        }
        cleanupWindowScale?.();
        cleanupWindowScale = unlisten;
      })
      .catch(() => {});
    logTimer = setInterval(() => void refreshLogs(), 1500);
    void refreshLogs();
    void syncAfterLayout();
    return () => cleanup();
  });

  $effect(() => {
    if (resizeObserver && host) resizeObserver.observe(host);
    schedulePositionWebview();
  });

  $effect(() => registerExternalToolViewCloser(run.id, closeWebview));

  $effect(() => {
    run.logsOpen;
    void refreshLogs();
  });

  $effect(() => {
    const key = `${run.id}:${run.runtimeId ?? ""}:${run.rendered?.url ?? ""}:${run.launchedAtMs}:${run.webEmbedder}`;
    if (key === pollKey) return;
    pollKey = key;
    closeWebview();
    if (run.status === "launching" || run.status === "starting") {
      const snapshot = pollingSnapshot(key);
      if (snapshot) startPolling(snapshot);
    } else if (run.status === "ready" && run.webEmbedder === "webview") {
      clearPoll();
      const snapshot = pollingSnapshot(key);
      if (snapshot) void createWebview(snapshot);
    } else {
      clearPoll();
    }
  });
  function startPolling(snapshot: PollSnapshot): void {
    clearPoll();
    const startedPollingAt = Date.now();
    const tick = async () => {
      if (!pollIsCurrent(snapshot)) return;
      try {
        if (await probeExternalToolUrl(snapshot.url)) {
          if (!pollIsCurrent(snapshot)) return;
          if (snapshot.webEmbedder === "webview") {
            await createWebview(snapshot);
          } else {
            markExternalToolReady(snapshot.runId);
          }
          return;
        }
      } catch (err) {
        if (pollIsCurrent(snapshot)) {
          await failExternalToolRun(
            snapshot.runId,
            snapshot.runtimeId,
            `Failed to check ${snapshot.url}: ${formatError(err)}`,
          );
        }
        return;
      }

      try {
        const processSnapshot = await readExternalToolProcess(snapshot.run).catch(() => null);
        if (!pollIsCurrent(snapshot)) return;
        if (processSnapshot && !processSnapshot.record.running) {
          markExternalToolExited(
            snapshot.runId,
            processSnapshot.record.id,
            processSnapshot.record.exitCode,
            snapshot.run.runtimeGeneration,
          );
          logs = processSnapshot.output;
          outputTruncated = processSnapshot.record.outputTruncated;
          return;
        }
        if (Date.now() - startedPollingAt > 15_000) {
          await failExternalToolRun(
            snapshot.runId,
            snapshot.runtimeId,
            `Timed out waiting for ${snapshot.url}`,
          );
          return;
        }
      } catch (err) {
        if (pollIsCurrent(snapshot)) {
          await failExternalToolRun(snapshot.runId, snapshot.runtimeId, formatError(err));
        }
        return;
      }
      if (pollIsCurrent(snapshot)) pollTimer = setTimeout(tick, 500);
    };
    pollTimer = setTimeout(tick, 0);
  }

  async function createWebview(snapshot: PollSnapshot): Promise<void> {
    if (webview || creatingWebviewKey === snapshot.key || !host) return;
    creatingWebviewKey = snapshot.key;
    const bounds = webviewBounds();
    if (!bounds) {
      if (creatingWebviewKey === snapshot.key) creatingWebviewKey = null;
      return;
    }
    const label = nativeWebviewLabel(snapshot);
    webviewLabel = label;
    const next = new Webview(getCurrentWindow(), label, {
      url: snapshot.url,
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
      focus: true,
    });
    webview = next;
    try {
      await waitForWebviewCreated(next);
      if (!pollIsCurrent(snapshot)) {
        if (webview === next) webview = null;
        closeNativeWebview(next);
        return;
      }
      markExternalToolReady(snapshot.runId);
      await syncAfterLayout();
    } catch (err) {
      if (webview === next) webview = null;
      closeNativeWebview(next);
      if (pollIsCurrent(snapshot)) {
        await failExternalToolRun(
          snapshot.runId,
          snapshot.runtimeId,
          `Failed to open ${snapshot.url}: ${formatError(err)}`,
        );
      }
    } finally {
      if (creatingWebviewKey === snapshot.key) creatingWebviewKey = null;
    }
  }

  function pollingSnapshot(key: string): PollSnapshot | null {
    if (!run.rendered?.url) return null;
    return {
      key,
      run: { ...run, rendered: { ...run.rendered } },
      runId: run.id,
      runtimeId: run.runtimeId,
      url: run.rendered.url,
      webEmbedder: run.webEmbedder,
      launchedAtMs: run.launchedAtMs,
    };
  }

  function pollIsCurrent(snapshot: PollSnapshot): boolean {
    return !destroyed && pollKey === snapshot.key;
  }

  async function waitForWebviewCreated(next: Webview): Promise<void> {
    const unlisteners: UnlistenFn[] = [];
    let settled = false;

    const cleanupListeners = () => {
      for (const unlisten of unlisteners.splice(0)) {
        try {
          unlisten();
        } catch {
          // best-effort listener cleanup
        }
      }
    };

    try {
      await new Promise<void>((resolve, reject) => {
        const settle = (finish: () => void) => {
          if (settled) return;
          settled = true;
          cleanupListeners();
          finish();
        };

        void next
          .once("tauri://created", () => settle(resolve))
          .then((unlisten) => {
            if (settled) unlisten();
            else unlisteners.push(unlisten);
          })
          .catch((err) => settle(() => reject(err)));

        void next
          .once<unknown>("tauri://error", (event) => settle(() => reject(event.payload)))
          .then((unlisten) => {
            if (settled) unlisten();
            else unlisteners.push(unlisten);
          })
          .catch((err) => settle(() => reject(err)));
      });
    } finally {
      settled = true;
      cleanupListeners();
    }
  }

  async function positionWebview(): Promise<void> {
    if (!webview || !host) return;
    const bounds = webviewBounds();
    if (!bounds) return;
    const current = webview;
    try {
      await current.setPosition(new LogicalPosition(bounds.x, bounds.y));
      await current.setSize(new LogicalSize(bounds.width, bounds.height));
    } catch {
      // The native child webview can be closed while a resize is already queued.
    }
  }

  function webviewBounds(): WebviewBounds | null {
    if (!host) return null;
    const rect = host.getBoundingClientRect();
    const toolbarInset = mainViewToolbarInset();
    return {
      x: rect.left,
      y: rect.top + toolbarInset,
      width: Math.max(1, rect.width),
      height: Math.max(1, rect.height),
    };
  }

  function mainViewToolbarInset(): number {
    const toolbar = host
      ?.closest("[data-main-view-root]")
      ?.querySelector<HTMLElement>("[data-main-view-toolbar]");
    return toolbar?.getBoundingClientRect().height ?? 0;
  }

  function schedulePositionWebview(): void {
    if (resizeFrame != null) return;
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = null;
      void positionWebview();
    });
  }

  async function syncAfterLayout(): Promise<void> {
    await tick();
    await positionWebview();
    requestAnimationFrame(() => schedulePositionWebview());
  }

  function closeWebview(): void {
    const current = webview;
    const label = webviewLabel ?? current?.label ?? null;
    webview = null;
    webviewLabel = null;
    creatingWebviewKey = null;
    closeNativeWebview(current);
    if (label) {
      void Webview.getByLabel(label)
        .then((found) => {
          if (found && found !== current) closeNativeWebview(found);
        })
        .catch(() => {});
    }
  }

  function closeNativeWebview(current: Webview | null): void {
    if (!current) return;
    void current.hide().catch(() => {});
    void current.setSize(new LogicalSize(1, 1)).catch(() => {});
    void current.setPosition(new LogicalPosition(-32000, -32000)).catch(() => {});
    void current.close().catch(() => {});
  }

  async function refreshLogs(): Promise<void> {
    const currentRun = {
      id: run.id,
      runtimeId: run.runtimeId,
      runtimeGeneration: run.runtimeGeneration,
      logsOpen: run.logsOpen,
      status: run.status,
    };
    const snapshot = await readExternalToolProcess(run).catch(() => null);
    if (!snapshot) return;
    if (
      run.id !== currentRun.id ||
      run.runtimeId !== currentRun.runtimeId ||
      run.runtimeGeneration !== currentRun.runtimeGeneration
    ) {
      return;
    }
    if (currentRun.logsOpen || currentRun.status === "error") {
      logs = snapshot.output;
      outputTruncated = snapshot.record.outputTruncated;
    }
    if (!snapshot.record.running && currentRun.status !== "error") {
      markExternalToolExited(
        currentRun.id,
        snapshot.record.id,
        snapshot.record.exitCode,
        currentRun.runtimeGeneration,
      );
    }
  }

  function clearPoll(): void {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = null;
  }

  function cleanup(): void {
    destroyed = true;
    clearPoll();
    if (logTimer) clearInterval(logTimer);
    logTimer = null;
    if (resizeFrame != null) cancelAnimationFrame(resizeFrame);
    resizeFrame = null;
    resizeObserver?.disconnect();
    resizeObserver = null;
    cleanupWindowResize?.();
    cleanupWindowResize = null;
    cleanupWindowScale?.();
    cleanupWindowScale = null;
    closeWebview();
  }

  function formatError(err: unknown): string {
    if (err instanceof Error && err.message) return err.message;
    if (typeof err === "string" && err.trim()) return err;
    try {
      return JSON.stringify(err);
    } catch {
      return String(err);
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col bg-bg-base">
  <div class="relative min-h-0 flex-1">
    {#if run.status === "starting" || run.status === "launching"}
      <div class="absolute inset-0 z-10 flex items-center justify-center text-sm text-text-muted">
        Loading {run.rendered?.url ?? run.toolName}...
      </div>
    {/if}
    {#if run.status === "ready" && run.rendered?.url && run.webEmbedder === "iframe"}
      <iframe
        src={run.rendered.url}
        title={run.toolName}
        class="h-full w-full border-0 bg-bg-deep"
      ></iframe>
    {:else}
      <div bind:this={host} class="h-full w-full bg-bg-deep"></div>
    {/if}
  </div>

  {#if run.logsOpen || run.status === "error"}
    <div class="h-40 shrink-0 border-t border-border-subtle bg-bg-deep">
      <div class="flex h-7 items-center justify-between border-b border-hairline px-3 text-[11px] text-text-muted">
        <span>Process Logs</span>
        {#if outputTruncated}<span>truncated</span>{/if}
      </div>
      <pre class="app-scrollbar h-[calc(100%-1.75rem)] overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-[11px] text-text-secondary">{logs || "No output yet."}</pre>
    </div>
  {/if}
</div>
