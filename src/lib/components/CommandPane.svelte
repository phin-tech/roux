<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { attachPtyOutput, createPtyOutputChannel, spawnTask, killSession, onSessionExit, resizeSession, type SessionExitPayload } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import { getXtermTheme } from "$lib/themes";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type { Channel } from "@tauri-apps/api/core";
  import { registerCommandPane, unregisterCommandPane } from "$lib/panes/commandPaneRegistry";
  import { createResizeScheduler } from "$lib/panes/resizeScheduler";

  interface Props {
    command: string;
    workingDir: string;
    paneId: string;
    initialPtyId: string;
    active?: boolean;
    isFocused?: boolean;
    visible?: boolean;
    focusRequestVersion?: number;
  }

  let { command, workingDir, paneId, initialPtyId, active = true, isFocused = false, visible = true, focusRequestVersion = 0 }: Props = $props();

  let containerEl: HTMLDivElement;
  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;

  let currentPtyId = $state("");
  let status = $state<"running" | "succeeded" | "failed">("running");
  let exitCode = $state<number | null>(null);
  let startedAt = $state(Date.now());
  let elapsed = $state("0s");

  let destroyed = false;
  let unlisteners: UnlistenFn[] = [];
  let elapsedTimer: ReturnType<typeof setInterval> | null = null;
  let outputChannel: Channel<ArrayBuffer | Uint8Array | number[]> | null = null;
  const resizeScheduler = createResizeScheduler({
    getFitAddon: () => fitAddon,
    getPtyId: () => currentPtyId,
    onResize: (ptyId, cols, rows) => {
      void resizeSession(ptyId, cols, rows);
    },
  });

  function updateElapsed() {
    const s = Math.floor((Date.now() - startedAt) / 1000);
    if (s < 60) elapsed = `${s}s`;
    else elapsed = `${Math.floor(s / 60)}m${s % 60}s`;
  }

  function createTerminal(): Terminal {
    return new Terminal({
      fontSize: $settings.fontSize,
      fontFamily: $settings.fontFamily,
      lineHeight: $settings.lineHeight,
      scrollback: $settings.scrollback,
      cursorStyle: $settings.cursorStyle as "block" | "underline" | "bar",
      cursorBlink: $settings.cursorBlink,
      theme: getXtermTheme($settings.theme),
    });
  }

  async function cleanupListeners() {
    for (const fn of unlisteners.splice(0)) fn();
  }

  async function attachToPty(ptyId: string) {
    await cleanupListeners();

    // Register exit listener FIRST to avoid missing exit in the attach gap
    const unlisten = await onSessionExit(ptyId, (payload: SessionExitPayload) => {
      exitCode = payload.code;
      status = payload.code === 0 ? "succeeded" : "failed";
      if (elapsedTimer) clearInterval(elapsedTimer);
      updateElapsed();
    });
    if (destroyed) { unlisten(); return; }
    unlisteners.push(unlisten);
  }

  async function attachOutput(ptyId: string) {
    if (!outputChannel) {
      outputChannel = createPtyOutputChannel((bytes) => {
        term?.write(bytes);
      });
    }
    await attachPtyOutput(ptyId, outputChannel);
    if (destroyed) return;
  }

  function attach() {
    if (!containerEl || !term) return;
    if (!term.element) {
      term.open(containerEl);
      fitAddon = new FitAddon();
      term.loadAddon(fitAddon);
      try { term.loadAddon(new WebglAddon()); } catch {}
      term.loadAddon(new WebLinksAddon());
    } else if (!containerEl.contains(term.element)) {
      containerEl.appendChild(term.element);
    }

    resizeScheduler.schedule({
      afterFit: () => {
        if (isFocused) term?.focus();
      },
    });
  }

  function detach() {
    if (term?.element && containerEl?.contains(term.element)) {
      containerEl.removeChild(term.element);
    }
  }

  async function rerun() {
    // Kill old PTY if still running
    if (status === "running") {
      await killSession(currentPtyId).catch(() => {});
    }
    await cleanupListeners();

    // Reset state
    const newPtyId = `${paneId}-${Date.now()}`;
    currentPtyId = newPtyId;
    status = "running";
    exitCode = null;
    startedAt = Date.now();
    elapsed = "0s";

    // Clear terminal
    term?.clear();
    term?.reset();

    // Attach listeners before spawning
    await attachToPty(newPtyId);

    // Start elapsed timer
    if (elapsedTimer) clearInterval(elapsedTimer);
    elapsedTimer = setInterval(updateElapsed, 1000);

    // Spawn new command
    await spawnTask(newPtyId, command, workingDir);
    await attachOutput(newPtyId);
  }


  onMount(async () => {
    currentPtyId = initialPtyId;
    registerCommandPane({
      paneId,
      command,
      getStatus: () => status,
      triggerRerun: () => void rerun(),
    });
    term = createTerminal();

    // Attach to the initial PTY
    await attachToPty(currentPtyId);
    await attachOutput(currentPtyId);

    // Start elapsed timer
    elapsedTimer = setInterval(updateElapsed, 1000);

    resizeObserver = new ResizeObserver(() => {
      if (active && visible && fitAddon) {
        resizeScheduler.schedule();
      }
    });
    resizeObserver.observe(containerEl);

    if (active && visible) attach();
  });

  onDestroy(() => {
    destroyed = true;
    unregisterCommandPane(paneId);
    resizeScheduler.cancel();
    resizeObserver?.disconnect();
    if (elapsedTimer) clearInterval(elapsedTimer);
    cleanupListeners();
    detach();
    term?.dispose();
  });

  $effect(() => {
    if (term) term.options.theme = getXtermTheme($settings.theme);
  });

  // Refit when session becomes active (container goes from display:none to visible)
  $effect(() => {
    if (active && visible && fitAddon) {
      resizeScheduler.schedule();
    }
  });

  $effect(() => {
    if (active && visible) {
      attach();
    } else {
      detach();
    }
  });

  // Focus terminal when this pane is focused, visible, or a focus request is made
  $effect(() => {
    void focusRequestVersion;
    if (isFocused && visible && term) {
      resizeScheduler.schedule({
        afterFit: () => {
          term?.focus();
        },
      });
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="relative flex h-full w-full flex-col bg-bg-deep">
  <!-- Command header bar -->
  <div class="flex h-9 shrink-0 select-none items-center gap-2 border-b border-hairline bg-bg-surface/30 px-3">
    <span class="font-mono text-[11px] text-text-secondary truncate flex-1">{command}</span>
    <span class="text-[10px] text-text-muted font-mono">{elapsed}</span>
    {#if status === "running"}
      <span class="h-2 w-2 shrink-0 rounded-full bg-accent animate-pulse"></span>
      <button
        class="bg-transparent px-1 font-mono text-[10px] text-text-muted border-none cursor-pointer hover:text-red"
        onclick={() => { void killSession(currentPtyId).catch(() => {}); }}
        title="Stop"
      >&#9632;</button>
    {:else}
      {#if status === "succeeded"}
        <span class="text-[10px] text-green font-mono">exit 0</span>
      {:else}
        <span class="text-[10px] text-red font-mono">exit {exitCode ?? "?"}</span>
      {/if}
      <button
        class="bg-transparent px-1 font-mono text-[10px] text-text-muted border-none cursor-pointer hover:text-accent"
        onclick={() => void rerun()}
        title="Rerun (r)"
      >&#8635;</button>
    {/if}
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div bind:this={containerEl} class="ui-terminal-frame min-h-0 flex-1" onclick={() => term?.focus()}></div>
</div>
