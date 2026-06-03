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
  import type { BackgroundThrottlingPolicy } from "@tauri-apps/api/window";
  import { probeExternalToolUrl } from "$lib/tauri";
  import { commandSurface } from "$lib/stores/commandSurface";
  import {
    closeNativeWebview,
    closeRetainedExternalToolWebview,
    retainExternalToolWebview,
    takeRetainedExternalToolWebview,
  } from "$lib/externalTools/nativeWebviews";
  import type { ExternalToolRun } from "$lib/stores/externalTools";
  import {
    failExternalToolRun,
    markExternalToolExited,
    markExternalToolReady,
    readExternalToolProcess,
    registerExternalToolViewCloser,
    restartExternalToolRun,
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

  const KEEP_ACTIVE_BACKGROUND_THROTTLING =
    "disabled" as BackgroundThrottlingPolicy;

  let { run }: Props = $props();
  let host = $state<HTMLDivElement | null>(null);
  let iframe = $state<HTMLIFrameElement | null>(null);
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
  let webviewHiddenForPalette = false;
  let syncingPaletteVisibility = false;
  let pendingPaletteHidden: boolean | null = null;

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

  $effect(() => registerExternalToolViewCloser(run.id, () => closeWebview()));

  $effect(() => {
    run.logsOpen;
    void refreshLogs();
  });

  $effect(() => {
    if (
      run.status !== "ready" ||
      run.webEmbedder !== "iframe" ||
      !run.rendered?.url
    )
      return;
    const frame = requestAnimationFrame(() => iframe?.focus());
    return () => cancelAnimationFrame(frame);
  });

  $effect(() => {
    void syncWebviewPaletteVisibility(
      $commandSurface.open && $commandSurface.mode === "palette",
    );
  });

  $effect(() => {
    if (run.status === "error") {
      clearPoll();
      closeWebview();
    }
  });

  $effect(() => {
    const key = `${run.id}:${run.runtimeId ?? ""}:${run.rendered?.url ?? ""}:${run.launchedAtMs}:${run.webEmbedder}`;
    if (key === pollKey) {
      ensureReadyWebview(key);
      return;
    }
    pollKey = key;
    closeWebview({ closeRetained: false });
    if (run.status === "launching" || run.status === "starting") {
      const snapshot = pollingSnapshot(key);
      if (snapshot) startPolling(snapshot);
    } else if (run.status === "ready" && run.webEmbedder === "webview") {
      clearPoll();
      ensureReadyWebview(key);
    } else {
      clearPoll();
    }
  });

  function ensureReadyWebview(key: string): void {
    if (
      run.status !== "ready" ||
      run.webEmbedder !== "webview" ||
      webview ||
      creatingWebviewKey
    ) {
      return;
    }
    const snapshot = pollingSnapshot(key);
    if (snapshot) void createWebview(snapshot);
  }

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
        const processSnapshot = await readExternalToolProcess(
          snapshot.run,
        ).catch(() => null);
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
          await failExternalToolRun(
            snapshot.runId,
            snapshot.runtimeId,
            formatError(err),
          );
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
    const retained = snapshot.run.keepWebviewAlive
      ? takeRetainedExternalToolWebview(
          snapshot.runId,
          nativeWebviewCacheKey(snapshot),
        )
      : null;
    if (retained) {
      const current = retained.webview;
      webview = current;
      webviewLabel = retained.label;
      const hiddenForPalette =
        $commandSurface.open && $commandSurface.mode === "palette";
      webviewHiddenForPalette = hiddenForPalette;
      try {
        if (!hiddenForPalette) {
          await current.show();
        }
        if (!webviewSnapshotIsCurrent(snapshot)) {
          discardCurrentWebview(current);
          return;
        }
        await syncAfterLayout();
        if (!webviewSnapshotIsCurrent(snapshot)) {
          discardCurrentWebview(current);
          return;
        }
        void focusNativeWebview(current);
        markExternalToolReady(snapshot.runId);
      } catch (err) {
        discardCurrentWebview(current);
        if (webviewSnapshotIsCurrent(snapshot)) {
          await failExternalToolRun(
            snapshot.runId,
            snapshot.runtimeId,
            `Failed to open ${snapshot.url}: ${formatError(err)}`,
          );
        }
      } finally {
        if (creatingWebviewKey === snapshot.key) creatingWebviewKey = null;
      }
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
      backgroundThrottling: snapshot.run.keepWebviewAlive
        ? KEEP_ACTIVE_BACKGROUND_THROTTLING
        : undefined,
    });
    webview = next;
    try {
      await waitForWebviewCreated(next);
      if (!webviewSnapshotIsCurrent(snapshot)) {
        discardCurrentWebview(next);
        return;
      }
      await syncWebviewPaletteVisibility(
        $commandSurface.open && $commandSurface.mode === "palette",
      );
      await syncAfterLayout();
      if (!webviewSnapshotIsCurrent(snapshot)) {
        discardCurrentWebview(next);
        return;
      }
      void focusNativeWebview(next);
      markExternalToolReady(snapshot.runId);
    } catch (err) {
      discardCurrentWebview(next);
      if (webviewSnapshotIsCurrent(snapshot)) {
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

  function webviewSnapshotIsCurrent(snapshot: PollSnapshot): boolean {
    return (
      pollIsCurrent(snapshot) &&
      run.id === snapshot.runId &&
      run.runtimeId === snapshot.runtimeId &&
      run.rendered?.url === snapshot.url &&
      run.webEmbedder === snapshot.webEmbedder &&
      run.launchedAtMs === snapshot.launchedAtMs &&
      run.status !== "error"
    );
  }

  function nativeWebviewCacheKey(
    snapshot: Pick<
      PollSnapshot,
      "runId" | "runtimeId" | "url" | "launchedAtMs"
    >,
  ): string {
    return `${snapshot.runId}:${snapshot.runtimeId ?? ""}:${snapshot.url}:${snapshot.launchedAtMs}`;
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
          .once<unknown>("tauri://error", (event) =>
            settle(() => reject(event.payload)),
          )
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
    if (!webview || !host || webviewHiddenForPalette) return;
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

  async function syncWebviewPaletteVisibility(hidden: boolean): Promise<void> {
    if (syncingPaletteVisibility) {
      pendingPaletteHidden = hidden;
      return;
    }
    syncingPaletteVisibility = true;
    try {
      let nextHidden = hidden;
      do {
        pendingPaletteHidden = null;
        await applyWebviewPaletteVisibility(nextHidden);
        if (pendingPaletteHidden == null || pendingPaletteHidden === nextHidden)
          break;
        nextHidden = pendingPaletteHidden;
      } while (true);
    } finally {
      syncingPaletteVisibility = false;
    }
  }

  async function applyWebviewPaletteVisibility(hidden: boolean): Promise<void> {
    const current = webview;
    if (!current) {
      return;
    }
    try {
      if (hidden) {
        if (webviewHiddenForPalette) return;
        webviewHiddenForPalette = true;
        await current.hide();
        return;
      }
      if (!webviewHiddenForPalette) return;
      await current.show();
      webviewHiddenForPalette = false;
      await syncAfterLayout();
      void focusNativeWebview(current);
    } catch {
      // Palette visibility is best-effort; the native child webview may close mid-sync.
    }
  }

  async function focusNativeWebview(current: Webview): Promise<void> {
    if (current !== webview || webviewHiddenForPalette) return;
    try {
      await current.setFocus();
    } catch {
      // Focus is best-effort; some platforms can reject if the child webview is closing.
    }
  }

  function closeWebview({
    closeRetained = true,
  }: { closeRetained?: boolean } = {}): void {
    const current = webview;
    const label = webviewLabel ?? current?.label ?? null;
    webview = null;
    webviewLabel = null;
    creatingWebviewKey = null;
    webviewHiddenForPalette = false;
    if (closeRetained) closeRetainedExternalToolWebview(run.id);
    closeNativeWebview(current);
    if (label) {
      void Webview.getByLabel(label)
        .then((found) => {
          if (found && found !== current) closeNativeWebview(found);
        })
        .catch(() => {});
    }
  }

  function discardCurrentWebview(current: Webview): void {
    if (webview === current) {
      webview = null;
      webviewLabel = null;
      webviewHiddenForPalette = false;
    }
    closeNativeWebview(current);
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
      logs = snapshot.output;
      outputTruncated = snapshot.record.outputTruncated;
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
    retainOrCloseWebview();
  }

  function retainOrCloseWebview(): void {
    const current = webview;
    if (
      current &&
      run.keepWebviewAlive &&
      run.status === "ready" &&
      run.webEmbedder === "webview" &&
      run.rendered?.url
    ) {
      const label = webviewLabel ?? current.label;
      webview = null;
      webviewLabel = null;
      creatingWebviewKey = null;
      webviewHiddenForPalette = false;
      retainExternalToolWebview({
        runId: run.id,
        key: nativeWebviewCacheKey({
          runId: run.id,
          runtimeId: run.runtimeId,
          url: run.rendered.url,
          launchedAtMs: run.launchedAtMs,
        }),
        label,
        webview: current,
      });
      return;
    }
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
    {#if run.status === "error"}
      <div
        class="absolute inset-0 z-10 flex items-center justify-center bg-bg-deep p-6"
      >
        <div
          class="w-full max-w-2xl rounded border border-red/30 bg-red/10 p-4"
        >
          <div class="text-sm font-semibold text-red">
            Failed to launch {run.toolName}
          </div>
          <div
            class="mt-2 whitespace-pre-wrap break-words font-mono text-[11px] text-text-secondary"
          >
            {run.error}
          </div>
          <button
            type="button"
            class="mt-4 rounded border border-border-subtle bg-bg-elevated px-3 py-1.5 text-xs text-text-primary hover:bg-bg-hover"
            onclick={() => void restartExternalToolRun(run.id)}
          >
            Retry
          </button>
        </div>
      </div>
    {:else if run.status === "starting" || run.status === "launching"}
      <div
        class="absolute inset-0 z-10 flex items-center justify-center text-sm text-text-muted"
      >
        Loading {run.rendered?.url ?? run.toolName}...
      </div>
    {/if}
    {#if run.status === "ready" && run.rendered?.url && run.webEmbedder === "iframe"}
      <iframe
        bind:this={iframe}
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
      <div
        class="flex h-7 items-center justify-between border-b border-hairline px-3 text-[11px] text-text-muted"
      >
        <span>Process Logs</span>
        {#if outputTruncated}<span>truncated</span>{/if}
      </div>
      <pre
        class="app-scrollbar h-[calc(100%-1.75rem)] overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-[11px] text-text-secondary">{logs ||
          "No output yet."}</pre>
    </div>
  {/if}
</div>
