<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { onPtyOutput, onSessionExit, writeToSession, resizeSession } from "$lib/tauri";
  import { sessionState, setSessionDisconnected } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import { ensureClaudeTerminal } from "$lib/panes/terminalRegistry";
  import { getXtermTheme } from "$lib/themes";
  import { log, logError } from "$lib/logging";
  import { reconnectSession } from "$lib/sessions/reconnect";
  import SessionPicker from "./SessionPicker.svelte";

  interface Props {
    sessionId: string;
    active: boolean;
    isFocused?: boolean;
    visible?: boolean;
    focusRequestVersion?: number;
  }

  let { sessionId, active, isFocused = false, visible = true, focusRequestVersion = 0 }: Props = $props();

  let containerEl: HTMLDivElement | undefined = $state();
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;

  const session = $derived($sessionState.sessions.find((s) => s.id === sessionId));
  const isDisconnected = $derived(session?.status === "disconnected");

  async function handleContinue() {
    if (!session) return;
    log(`Continuing last session for ${sessionId}`);
    await reconnect(["--continue"]);
  }

  async function handleResume(claudeSessionId: string) {
    if (!session) return;
    log(`Resuming claude session ${claudeSessionId} for ${sessionId}`);
    await reconnect(["--resume", claudeSessionId]);
  }

  async function handleNew() {
    if (!session) return;
    log(`Starting new claude session for ${sessionId}`);
    await reconnect();
  }

  async function reconnect(extraFlags?: string[]) {
    if (!session) return;
    try {
      await reconnectSession(session, extraFlags);
      // Re-attach listeners for the new PTY
      await attachListeners();
    } catch (e) {
      logError("Failed to reconnect session", e);
    }
  }

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
      log(`Session ${sid} exited (code=${_code})`);
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
    if (containerEl) {
      resizeObserver.observe(containerEl);
    }

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

{#if isDisconnected && session}
  <div class="ui-terminal-frame h-full w-full overflow-hidden rounded-[0.95rem]">
    <SessionPicker
      cwd={session.worktreePath}
      onContinue={handleContinue}
      onResume={handleResume}
      onNew={handleNew}
    />
  </div>
{:else}
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
      class="ui-terminal-frame h-full w-full overflow-hidden rounded-[0.95rem]"
      onclick={() => terminal?.focus()}
    ></div>
  </div>
{/if}
