<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { spawnTask, killSession, onPtyOutput, onSessionExit, resizeSession } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import { getXtermTheme } from "$lib/themes";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    command: string;
    workingDir: string;
    paneId: string;
    initialPtyId: string;
    onClose: () => void | Promise<void>;
  }

  let { command, workingDir, paneId, initialPtyId, onClose }: Props = $props();

  let containerEl: HTMLDivElement;
  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;

  let currentPtyId = $state("");
  let status = $state<"running" | "succeeded" | "failed">("running");
  let exitCode = $state<number | null>(null);
  let startedAt = $state(Date.now());
  let hovering = $state(false);
  let elapsed = $state("0s");

  let unlisteners: UnlistenFn[] = [];
  let elapsedTimer: ReturnType<typeof setInterval> | null = null;

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

    unlisteners.push(await onPtyOutput(ptyId, (b64data) => {
      const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
      term?.write(bytes);
    }));

    unlisteners.push(await onSessionExit(ptyId, (code) => {
      exitCode = code;
      status = code === 0 ? "succeeded" : "failed";
      if (elapsedTimer) clearInterval(elapsedTimer);
      updateElapsed();
    }));
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
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (status !== "running" && e.key === "r" && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      void rerun();
    }
  }

  onMount(async () => {
    currentPtyId = initialPtyId;
    term = createTerminal();
    term.open(containerEl);

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    try { term.loadAddon(new WebglAddon()); } catch {}
    term.loadAddon(new WebLinksAddon());

    // Attach to the initial PTY
    await attachToPty(currentPtyId);

    // Start elapsed timer
    elapsedTimer = setInterval(updateElapsed, 1000);

    resizeObserver = new ResizeObserver(() => {
      if (fitAddon) {
        fitAddon.fit();
        const dims = fitAddon.proposeDimensions();
        if (dims) resizeSession(currentPtyId, dims.cols, dims.rows);
      }
    });
    resizeObserver.observe(containerEl);

    requestAnimationFrame(() => {
      fitAddon?.fit();
      const dims = fitAddon?.proposeDimensions();
      if (dims) resizeSession(currentPtyId, dims.cols, dims.rows);
    });
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    if (elapsedTimer) clearInterval(elapsedTimer);
    cleanupListeners();
    if (term?.element && containerEl?.contains(term.element)) {
      containerEl.removeChild(term.element);
    }
    term?.dispose();
  });

  $effect(() => {
    if (term) term.options.theme = getXtermTheme($settings.theme);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="relative w-full h-full bg-black flex flex-col"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
  onkeydown={handleKeyDown}
>
  <!-- Command header bar -->
  <div class="flex items-center gap-2 px-3 py-1.5 bg-[#111118] border-b border-white/6 shrink-0 select-none">
    <span class="font-mono text-[11px] text-text-secondary truncate flex-1">{command}</span>
    <span class="text-[10px] text-text-muted font-mono">{elapsed}</span>
    {#if status === "running"}
      <span class="w-2 h-2 rounded-full bg-blue-400 animate-pulse shrink-0"></span>
      <button
        class="text-[10px] text-text-muted hover:text-red-400 bg-transparent border-none cursor-pointer px-1 font-mono"
        onclick={() => { void killSession(currentPtyId).catch(() => {}); }}
        title="Stop"
      >&#9632;</button>
    {:else}
      {#if status === "succeeded"}
        <span class="text-[10px] text-green-400 font-mono">exit 0</span>
      {:else}
        <span class="text-[10px] text-red-400 font-mono">exit {exitCode ?? "?"}</span>
      {/if}
      <button
        class="text-[10px] text-text-muted hover:text-accent bg-transparent border-none cursor-pointer px-1 font-mono"
        onclick={() => void rerun()}
        title="Rerun (r)"
      >&#8635;</button>
    {/if}
  </div>

  <div bind:this={containerEl} class="flex-1 min-h-0"></div>

  {#if hovering}
    <button
      class="absolute right-2 top-10 z-10 flex h-7 w-7 items-center justify-center rounded-full border border-white/8 bg-slate-900/85 text-xs leading-none text-text-muted backdrop-blur-sm hover:bg-slate-800 hover:text-text-primary"
      onclick={() => void onClose()}
      title="Close pane"
    >
      &times;
    </button>
  {/if}
</div>
