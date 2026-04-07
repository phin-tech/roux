<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { attachPtyOutput, createPtyOutputChannel, onSessionExit, writeToSession, resizeSession, type SessionExitPayload } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import { ensureShellTerminal } from "$lib/panes/terminalRegistry";
  import { createResizeScheduler } from "$lib/panes/resizeScheduler";
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
  const resizeScheduler = createResizeScheduler({
    getFitAddon: () => fitAddon,
    getPtyId: () => capturedPtyId || ptyId,
    onResize: (nextPtyId, cols, rows) => {
      void resizeSession(nextPtyId, cols, rows);
    },
  });

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
      outputChannel: null,
      generation: null,
    }));
  }

  function attach() {
    if (!containerEl) return;
    const instance = getOrCreateTerminal();
    terminal = instance.terminal;
    fitAddon = instance.fitAddon;

    if (!terminal.element) {
      terminal.open(containerEl);
      if (!fitAddon) {
        fitAddon = new FitAddon();
        instance.fitAddon = fitAddon;
      }
      terminal.loadAddon(fitAddon);
      try { terminal.loadAddon(new WebglAddon()); } catch {}
      terminal.loadAddon(new WebLinksAddon());

      // Track cwd via OSC 7 (emitted by modern shells on directory change)
      terminal.parser.registerOscHandler(7, (data) => {
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

      instance.disposables.push(terminal.onData((data) => writeToSession(capturedPtyId, data)));
    } else if (!containerEl.contains(terminal.element)) {
      containerEl.appendChild(terminal.element);
    }

    resizeScheduler.schedule({
      afterFit: () => {
        if (isFocused) terminal?.focus();
      },
    });
  }

  function detach() {
    if (terminal?.element && containerEl?.contains(terminal.element)) {
      containerEl.removeChild(terminal.element);
    }
  }

  onMount(async () => {
    log(`ShellTerminal mounting: pane=${paneId} pty=${capturedPtyId}`);
    const instance = getOrCreateTerminal();
    terminal = instance.terminal;
    fitAddon = instance.fitAddon;

    // Register exit listener FIRST to avoid missing exit in the attach gap
    if (instance.unlisteners.length === 0 && closeOnExit) {
      instance.unlisteners.push(await onSessionExit(capturedPtyId, (payload: SessionExitPayload) => {
        log(`Shell pane ${paneId} exited (code=${payload.code}, reason=${payload.reason})`);
        void onClose();
      }));
    }

    if (!instance.outputChannel) {
      instance.outputChannel = createPtyOutputChannel((bytes) => {
        instance.terminal.write(bytes);
      });
    }
    await attachPtyOutput(capturedPtyId, instance.outputChannel);

    resizeObserver = new ResizeObserver(() => {
      if (active && visible && fitAddon) {
        resizeScheduler.schedule();
      }
    });
    resizeObserver.observe(containerEl);

    if (active && visible) attach();
  });

  onDestroy(() => {
    resizeScheduler.cancel();
    resizeObserver?.disconnect();
    detach();
  });

  $effect(() => {
    terminal = getOrCreateTerminal().terminal;
    terminal.options.theme = getXtermTheme($settings.theme);
  });

  $effect(() => {
    if (active && visible) {
      attach();
    } else {
      detach();
    }
  });

  // Refit when session becomes active (container goes from display:none to visible)
  $effect(() => {
    if (active && visible && fitAddon) {
      resizeScheduler.schedule();
    }
  });

  // Focus terminal when this pane is focused, visible, or a focus request is made
  $effect(() => {
    void focusRequestVersion;
    if (isFocused && visible && terminal) {
      resizeScheduler.schedule({
        afterFit: () => {
          terminal?.focus();
        },
      });
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="relative h-full w-full p-2">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    bind:this={containerEl}
    class="ui-terminal-frame h-full w-full overflow-hidden rounded-[0.95rem]"
    onclick={() => terminal?.focus()}
  ></div>
</div>
