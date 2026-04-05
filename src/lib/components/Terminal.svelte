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
  import { ensureClaudeTerminal } from "$lib/panes/terminalRegistry";
  import { getXtermTheme } from "$lib/themes";

  interface Props {
    sessionId: string;
    active: boolean;
  }

  let { sessionId, active }: Props = $props();

  let containerEl: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;

  function getOrCreateTerminal() {
    return ensureClaudeTerminal(sessionId, () => ({
      terminal: new Terminal({
        fontSize: $settings.fontSize,
        fontFamily: $settings.fontFamily,
        lineHeight: $settings.lineHeight,
        scrollback: $settings.scrollback,
        cursorStyle: $settings.cursorStyle as "block" | "underline" | "bar",
        cursorBlink: $settings.cursorBlink,
        theme: getXtermTheme($settings.theme),
      }),
      fitAddon: null,
      unlisteners: [],
      disposables: [],
    }));
  }

  async function attachListeners() {
    const entry = getOrCreateTerminal();
    if (entry.unlisteners.length > 0) return;

    const sid = sessionId; // capture by value for callbacks
    entry.unlisteners.push(await onPtyOutput(sid, (b64data) => {
      if (entry.terminal) {
        const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
        entry.terminal.write(bytes);
      }
    }));

    entry.unlisteners.push(await onSessionExit(sid, (_code) => {
      setSessionDisconnected(sid);
    }));
  }

  // Non-reactive copy of sessionId for use in xterm event callbacks.
  // Reading the reactive prop inside xterm's synchronous focus/blur handlers
  // can hit stale parent props and throw.
  let capturedSessionId = "";
  $effect.pre(() => { capturedSessionId = sessionId; });

  function attach() {
    if (!containerEl) return;

    const entry = getOrCreateTerminal();
    terminal = entry.terminal;
    fitAddon = entry.fitAddon;

    if (!terminal.element) {
      // First time — open into the container
      terminal.open(containerEl);

      fitAddon = new FitAddon();
      entry.fitAddon = fitAddon;
      terminal.loadAddon(fitAddon);

      try {
        terminal.loadAddon(new WebglAddon());
      } catch {
        // WebGL not available, fall back to canvas
      }

      terminal.loadAddon(new WebLinksAddon());

      entry.disposables.push(terminal.onData((data) => {
        writeToSession(capturedSessionId, data);
      }));
    } else {
      // Re-attach existing terminal element
      containerEl.appendChild(terminal.element);
    }

    requestAnimationFrame(() => {
      fitAddon?.fit();
      terminal?.focus();
      const dims = fitAddon?.proposeDimensions();
      if (dims) {
        resizeSession(capturedSessionId, dims.cols, dims.rows);
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
    resizeObserver?.disconnect();
    detach();
  });

  $effect(() => {
    if (active) {
      attach();
    } else {
      detach();
    }
  });

  $effect(() => {
    terminal = getOrCreateTerminal().terminal;
    terminal.options.theme = getXtermTheme($settings.theme);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="flex h-full w-full p-2"
  class:hidden={!active}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    bind:this={containerEl}
    class="h-full w-full overflow-hidden rounded-[0.95rem] bg-[#0a0a0a] shadow-[inset_0_0_0_1px_rgba(39,39,42,0.9),inset_0_18px_36px_rgba(255,255,255,0.02)]"
    onclick={() => terminal?.focus()}
  ></div>
</div>
