<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { onPtyOutput, onSessionExit, writeToSession, resizeSession } from "$lib/tauri";
  import { setSessionDisconnected } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    sessionId: string;
    active: boolean;
  }

  let { sessionId, active }: Props = $props();

  let containerEl: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let unlisteners: UnlistenFn[] = [];
  let resizeObserver: ResizeObserver | null = null;

  const terminalInstances = new Map<string, Terminal>();

  function getOrCreateTerminal(): Terminal {
    if (terminalInstances.has(sessionId)) {
      return terminalInstances.get(sessionId)!;
    }

    const term = new Terminal({
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

    terminalInstances.set(sessionId, term);
    return term;
  }

  async function attachListeners() {
    const outputUnlisten = await onPtyOutput(sessionId, (b64data) => {
      const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
      terminal?.write(bytes);
    });
    unlisteners.push(outputUnlisten);

    // Status is now handled globally in App.svelte via hooks-based detection

    const exitUnlisten = await onSessionExit(sessionId, (_code) => {
      setSessionDisconnected(sessionId);
    });
    unlisteners.push(exitUnlisten);
  }

  function attach() {
    if (!containerEl) return;

    terminal = getOrCreateTerminal();

    if (!terminal.element) {
      // First time — open into the container
      terminal.open(containerEl);

      fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);

      try {
        terminal.loadAddon(new WebglAddon());
      } catch {
        // WebGL not available, fall back to canvas
      }

      terminal.loadAddon(new WebLinksAddon());

      terminal.onData((data) => {
        writeToSession(sessionId, data);
      });
    } else {
      // Re-attach existing terminal element
      containerEl.appendChild(terminal.element);
    }

    requestAnimationFrame(() => {
      fitAddon?.fit();
      const dims = fitAddon?.proposeDimensions();
      if (dims) {
        resizeSession(sessionId, dims.cols, dims.rows);
      }
    });
  }

  function detach() {
    if (terminal?.element && containerEl?.contains(terminal.element)) {
      containerEl.removeChild(terminal.element);
    }
  }

  onMount(async () => {
    await attachListeners();

    resizeObserver = new ResizeObserver(() => {
      if (active && fitAddon) {
        fitAddon.fit();
        const dims = fitAddon.proposeDimensions();
        if (dims) {
          resizeSession(sessionId, dims.cols, dims.rows);
        }
      }
    });
    resizeObserver.observe(containerEl);

    if (active) attach();
  });

  onDestroy(() => {
    for (const unlisten of unlisteners) unlisten();
    resizeObserver?.disconnect();
    detach();
    // Dispose the terminal to free WebGL contexts and scrollback buffers
    if (terminal) {
      terminal.dispose();
      terminalInstances.delete(sessionId);
    }
  });

  $effect(() => {
    if (active) {
      attach();
    } else {
      detach();
    }
  });
</script>

<div
  bind:this={containerEl}
  class="flex-1 w-full h-full"
  class:hidden={!active}
></div>
