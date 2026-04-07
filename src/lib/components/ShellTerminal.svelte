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
  import { updatePaneWorkingDir } from "$lib/stores/panes";
  import { getXtermTheme } from "$lib/themes";
  import { log } from "$lib/logging";

  interface Props {
    sessionId: string;
    ptyId: string;
    paneId: string;
    active?: boolean;
    isFocused?: boolean;
    visible?: boolean;
    focusRequestVersion?: number;
    closeOnExit?: boolean;
    onClose: () => void | Promise<void>;
  }

  let { sessionId, ptyId, paneId, active = true, isFocused = false, visible = true, focusRequestVersion = 0, closeOnExit = true, onClose }: Props = $props();

  // Non-reactive copy of ptyId for use in xterm event callbacks.
  // Reading the reactive prop inside xterm's synchronous handlers
  // can hit stale parent props and throw.
  let capturedPtyId = "";
  $effect.pre(() => { capturedPtyId = ptyId; });

  let containerEl: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let hovering = $state(false);

  function getOrCreateTerminal() {
    return ensureShellTerminal(paneId, () => ({
      ptyId: capturedPtyId,
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
    log(`ShellTerminal mounting: pane=${paneId} pty=${capturedPtyId}`);
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

      // Track cwd via OSC 7 (emitted by modern shells on directory change)
      term.parser.registerOscHandler(7, (data) => {
        try {
          const url = new URL(data);
          updatePaneWorkingDir(sessionId, paneId, decodeURIComponent(url.pathname));
        } catch {
          if (data.startsWith("/")) {
            updatePaneWorkingDir(sessionId, paneId, data);
          }
        }
        return false;
      });

      instance.disposables.push(term.onData((data) => writeToSession(capturedPtyId, data)));

      instance.unlisteners.push(await onPtyOutput(capturedPtyId, (b64data) => {
        const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
        term.write(bytes);
      }));

      if (closeOnExit) {
        instance.unlisteners.push(await onSessionExit(capturedPtyId, () => {
          log(`Shell pane ${paneId} exited`);
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
        if (dims) resizeSession(capturedPtyId, dims.cols, dims.rows);
      }
    });
    resizeObserver.observe(containerEl);

    requestAnimationFrame(() => {
      fitAddon?.fit();
      const dims = fitAddon?.proposeDimensions();
      if (dims) resizeSession(capturedPtyId, dims.cols, dims.rows);
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

  // Refit when session becomes active (container goes from display:none to visible)
  $effect(() => {
    if (active && fitAddon) {
      requestAnimationFrame(() => {
        fitAddon?.fit();
        const dims = fitAddon?.proposeDimensions();
        if (dims) resizeSession(capturedPtyId, dims.cols, dims.rows);
      });
    }
  });

  // Focus terminal when this pane is focused, visible, or a focus request is made
  $effect(() => {
    const _version = focusRequestVersion;
    if (isFocused && visible && terminal) {
      requestAnimationFrame(() => {
        fitAddon?.fit();
        terminal?.focus();
      });
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="relative h-full w-full p-2"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    bind:this={containerEl}
    class="ui-terminal-frame h-full w-full overflow-hidden rounded-[0.95rem]"
    onclick={() => terminal?.focus()}
  ></div>
  {#if hovering}
    <button
      class="absolute right-4 top-4 z-10 flex h-7 w-7 items-center justify-center rounded-full border border-border-subtle bg-bg-surface/85 text-xs leading-none text-text-muted backdrop-blur-sm hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
      onclick={handleClose}
      title="Close pane"
    >
      &times;
    </button>
  {/if}
</div>
