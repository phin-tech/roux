<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { onPtyOutput, writeToSession, resizeSession, killSession } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    ptyId: string;
    paneId: string;
    onClose: () => void;
  }

  let { ptyId, paneId, onClose }: Props = $props();

  let containerEl: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let unlisteners: UnlistenFn[] = [];
  let resizeObserver: ResizeObserver | null = null;
  let hovering = $state(false);

  onMount(async () => {
    terminal = new Terminal({
      fontSize: $settings.fontSize,
      fontFamily: $settings.fontFamily,
      lineHeight: $settings.lineHeight,
      scrollback: $settings.scrollback,
      cursorStyle: $settings.cursorStyle as "block" | "underline" | "bar",
      cursorBlink: $settings.cursorBlink,
      theme: {
        background: "#0a0a0c",
        foreground: "#c8cad8",
        cursor: "#7aa2f7",
        selectionBackground: "#282b40",
        black: "#0a0a0c",
        red: "#f7768e",
        green: "#9ece6a",
        yellow: "#e0af68",
        blue: "#7aa2f7",
        magenta: "#bb9af7",
        cyan: "#7dcfff",
        white: "#c8cad8",
        brightBlack: "#444b6a",
        brightRed: "#ff7a93",
        brightGreen: "#b9f27c",
        brightYellow: "#ff9e64",
        brightBlue: "#7da6ff",
        brightMagenta: "#c0a0ff",
        brightCyan: "#0db9d7",
        brightWhite: "#d5d6db",
      },
    });

    terminal.open(containerEl);

    fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);

    try {
      terminal.loadAddon(new WebglAddon());
    } catch {
      // WebGL not available
    }

    terminal.loadAddon(new WebLinksAddon());

    terminal.onData((data) => {
      writeToSession(ptyId, data);
    });

    const outputUnlisten = await onPtyOutput(ptyId, (b64data) => {
      const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
      terminal?.write(bytes);
    });
    unlisteners.push(outputUnlisten);

    resizeObserver = new ResizeObserver(() => {
      if (fitAddon) {
        fitAddon.fit();
        const dims = fitAddon.proposeDimensions();
        if (dims) {
          resizeSession(ptyId, dims.cols, dims.rows);
        }
      }
    });
    resizeObserver.observe(containerEl);

    requestAnimationFrame(() => {
      fitAddon?.fit();
      const dims = fitAddon?.proposeDimensions();
      if (dims) {
        resizeSession(ptyId, dims.cols, dims.rows);
      }
    });
  });

  onDestroy(() => {
    for (const unlisten of unlisteners) unlisten();
    resizeObserver?.disconnect();
    terminal?.dispose();
    killSession(ptyId).catch(() => {});
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="relative w-full h-full"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
>
  <div bind:this={containerEl} class="w-full h-full"></div>
  {#if hovering}
    <button
      class="absolute top-1 right-1 z-10 w-5 h-5 flex items-center justify-center rounded bg-bg-surface/80 text-text-muted hover:text-text-primary hover:bg-bg-surface text-xs leading-none"
      onclick={onClose}
      title="Close pane"
    >
      &times;
    </button>
  {/if}
</div>
