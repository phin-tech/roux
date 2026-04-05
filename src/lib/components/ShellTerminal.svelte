<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { onPtyOutput, writeToSession, resizeSession } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import { ensureShellTerminal } from "$lib/panes/terminalRegistry";

  interface Props {
    ptyId: string;
    paneId: string;
    onClose: () => void | Promise<void>;
  }

  let { ptyId, paneId, onClose }: Props = $props();

  let containerEl: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let hovering = $state(false);

  function getOrCreateTerminal() {
    return ensureShellTerminal(paneId, () => ({
      ptyId,
      terminal: new Terminal({
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
      }),
      fitAddon: null,
      unlisteners: [],
      disposables: [],
    }));
  }

  function detach() {
    if (terminal?.element && containerEl?.contains(terminal.element)) {
      containerEl.removeChild(terminal.element);
    }
  }

  onMount(async () => {
    const instance = getOrCreateTerminal();
    const term = instance.terminal;
    terminal = term;
    fitAddon = instance.fitAddon;

    if (!term.element) {
      // First time — open into container
      term.open(containerEl);
      if (!fitAddon) {
        fitAddon = new FitAddon();
        instance.fitAddon = fitAddon;
      }
      term.loadAddon(fitAddon);
      try { term.loadAddon(new WebglAddon()); } catch {}
      term.loadAddon(new WebLinksAddon());

      instance.disposables.push(term.onData((data) => writeToSession(ptyId, data)));

      instance.unlisteners.push(await onPtyOutput(ptyId, (b64data) => {
        const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
        term.write(bytes);
      }));
    } else {
      // Re-mount: move the existing terminal element into the new container
      containerEl.appendChild(term.element);
    }

    resizeObserver = new ResizeObserver(() => {
      if (fitAddon) {
        fitAddon.fit();
        const dims = fitAddon.proposeDimensions();
        if (dims) resizeSession(ptyId, dims.cols, dims.rows);
      }
    });
    resizeObserver.observe(containerEl);

    requestAnimationFrame(() => {
      fitAddon?.fit();
      const dims = fitAddon?.proposeDimensions();
      if (dims) resizeSession(ptyId, dims.cols, dims.rows);
    });
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    detach();
  });

  function handleClose() {
    void onClose();
  }
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
      onclick={handleClose}
      title="Close pane"
    >
      &times;
    </button>
  {/if}
</div>
