<script lang="ts">
  import { onMount } from "svelte";
  import {
    attachPtyOutput,
    createPtyOutputChannel,
    onSessionExit,
    resizeSession,
    writeToSession,
    type SessionExitPayload,
  } from "$lib/tauri";
  import { createXtermTerminalController } from "$lib/panes/xtermController";
  import { createResizeScheduler } from "$lib/panes/resizeScheduler";
  import type { TerminalController } from "$lib/panes/terminalRuntime";
  import type { ExternalToolRun } from "$lib/stores/externalTools";
  import {
    markExternalToolExited,
    restartExternalToolRun,
    setExternalToolRunError,
  } from "$lib/stores/externalTools";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    run: ExternalToolRun;
  }

  let { run }: Props = $props();
  let container = $state<HTMLDivElement | null>(null);
  let controller = $state<TerminalController | null>(null);
  let cleanupInput: (() => void) | null = null;
  let cleanupExit: UnlistenFn | null = null;
  let resizeObserver: ResizeObserver | null = null;

  const scheduler = createResizeScheduler({
    fit: () => controller?.fit() ?? null,
    getPtyId: () => run.runtimeId,
    onResize: (ptyId, cols, rows) => {
      resizeSession(ptyId, cols, rows).catch((err) => {
        setExternalToolRunError(run.id, err instanceof Error ? err.message : String(err));
      });
    },
  });

  onMount(() => {
    if (!container) return;
    controller = createXtermTerminalController({
      allowKeyboardEvent: () => true,
    });
    controller.attach(container);
    controller.setInputEnabled(true);
    cleanupInput = controller.onInput((data) => {
      const ptyId = run.runtimeId;
      if (!ptyId) return;
      writeToSession(ptyId, data).catch((err) => {
        setExternalToolRunError(run.id, err instanceof Error ? err.message : String(err));
      });
    });
    resizeObserver = new ResizeObserver(() => scheduler.schedule());
    resizeObserver.observe(container);
    scheduler.schedule({ afterFit: () => controller?.focus() });

    return () => cleanup();
  });

  $effect(() => {
    const ptyId = run.runtimeId;
    if (!ptyId || !controller) return;

    const outputChannel = createPtyOutputChannel((bytes) => {
      controller?.write(bytes);
    });
    void attachPtyOutput(ptyId, outputChannel).catch((err) => {
      setExternalToolRunError(run.id, err instanceof Error ? err.message : String(err));
    });
    void onSessionExit(ptyId, (payload: SessionExitPayload) => {
      markExternalToolExited(run.id, payload.code ?? null);
    }).then((unlisten) => {
      cleanupExit?.();
      cleanupExit = unlisten;
    });
    scheduler.schedule({ afterFit: () => controller?.focus() });

    return () => {
      cleanupExit?.();
      cleanupExit = null;
    };
  });

  function cleanup(): void {
    scheduler.cancel();
    resizeObserver?.disconnect();
    resizeObserver = null;
    cleanupExit?.();
    cleanupExit = null;
    cleanupInput?.();
    cleanupInput = null;
    controller?.dispose();
    controller = null;
  }
</script>

<div class="relative h-full min-h-0 bg-bg-base">
  {#if run.status === "launching" || !run.runtimeId}
    <div class="absolute inset-0 flex items-center justify-center text-sm text-text-muted">
      Launching {run.toolName}...
    </div>
  {:else if run.status === "exited"}
    <div class="absolute right-3 top-3 z-10 rounded border border-border-subtle bg-bg-surface/90 px-3 py-2 text-xs text-text-secondary shadow-lg">
      Exited{run.exitCode == null ? "" : ` with code ${run.exitCode}`}
      <button
        type="button"
        class="ml-2 rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-text-primary hover:bg-bg-hover"
        onclick={() => void restartExternalToolRun(run.id)}
      >
        Relaunch
      </button>
    </div>
  {/if}
  <div bind:this={container} class="h-full w-full overflow-hidden p-1"></div>
</div>
