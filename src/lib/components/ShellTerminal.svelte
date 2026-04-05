<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { onPtyOutput, onSessionExit, writeToSession, resizeSession } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import { ensureShellTerminal } from "$lib/panes/terminalRegistry";
  import { getXtermTheme } from "$lib/themes";

  interface Props {
    ptyId: string;
    paneId: string;
    closeOnExit?: boolean;
    onClose: () => void | Promise<void>;
  }

  let { ptyId, paneId, closeOnExit = true, onClose }: Props = $props();

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
        theme: getXtermTheme($settings.theme),
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

      if (closeOnExit) {
        instance.unlisteners.push(await onSessionExit(ptyId, () => {
          void onClose();
        }));
      }
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

  $effect(() => {
    terminal = getOrCreateTerminal().terminal;
    terminal.options.theme = getXtermTheme($settings.theme);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="relative w-full h-full bg-black"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
>
  <div bind:this={containerEl} class="w-full h-full"></div>
  {#if hovering}
    <button
      class="absolute right-2 top-2 z-10 flex h-7 w-7 items-center justify-center rounded-full border border-white/8 bg-slate-900/85 text-xs leading-none text-text-muted backdrop-blur-sm hover:bg-slate-800 hover:text-text-primary"
      onclick={handleClose}
      title="Close pane"
    >
      &times;
    </button>
  {/if}
</div>
