<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import "@xterm/xterm/css/xterm.css";
  import { paneInstances, attachToContainer, updateInstance } from "$lib/panes/instances";
  import { focusedPaneId, setLogicalFocus } from "$lib/panes/focus";
  import { closePane } from "$lib/panes/actions";
  import { createResizeScheduler } from "$lib/panes/resizeScheduler";
  import { resizeSession, killSession, spawnTask, attachPtyOutput, createPtyOutputChannel } from "$lib/tauri";
  import { sessionState } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import { getXtermTheme } from "$lib/themes";
  import { reconnectSession } from "$lib/sessions/reconnect";
  import { log, logError } from "$lib/logging";
  import SessionPicker from "./SessionPicker.svelte";
  import LazyMarkdownPane from "./LazyMarkdownPane.svelte";

  interface Props {
    paneId: string;
    sessionId: string;
    visible?: boolean;
    suppressTitleAccent?: boolean;
  }

  let { paneId, sessionId, visible = true, suppressTitleAccent = false }: Props = $props();

  let containerEl: HTMLDivElement | undefined = $state();
  let resizeObserver: ResizeObserver | null = null;
  let editingName = $state(false);
  let nameInput = $state("");

  // Command pane local state
  let elapsed = $state("0s");

  const instance = $derived($paneInstances.get(paneId));
  const isFocused = $derived($focusedPaneId === paneId);
  const session = $derived($sessionState.sessions.find((s) => s.id === sessionId));
  const isDisconnected = $derived(instance?.type === "claude" && session?.status === "disconnected");

  // Command pane status helpers
  const commandStatus = $derived(instance?.commandStatus ?? "idle");
  const commandExitCode = $derived(instance?.commandExitCode ?? null);

  const resizeScheduler = createResizeScheduler({
    getFitAddon: () => instance?.fitAddon ?? null,
    getPtyId: () => instance?.ptyId ?? "",
    onResize: (ptyId, cols, rows) => {
      resizeSession(ptyId, cols, rows).catch((e) => {
        log(`Resize failed for ${ptyId}: ${e}`);
      });
    },
  });

  function updateElapsed() {
    const startedAt = instance?.commandStartedAt;
    if (!startedAt) return;
    const s = Math.floor((Date.now() - startedAt) / 1000);
    if (s < 60) elapsed = `${s}s`;
    else elapsed = `${Math.floor(s / 60)}m${s % 60}s`;
  }

  function paneTypeLabel(type: string): string {
    switch (type) {
      case "claude": return "claude";
      case "shell": return "shell";
      case "markdown": return "doc";
      case "command": return "cmd";
      default: return type;
    }
  }

  function startRenaming(currentName: string) {
    nameInput = currentName;
    editingName = true;
  }

  function commitRename() {
    updateInstance(paneId, { name: nameInput.trim() || undefined });
    editingName = false;
  }

  function canClose(): boolean {
    if (!instance) return false;
    return !(instance.type === "claude" && paneId === `${sessionId}-main`);
  }

  function handleMouseDown() {
    setLogicalFocus(paneId);
  }

  // Reconnect handlers for claude pane SessionPicker
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
    } catch (e: any) {
      if (e?.message?.includes("already in progress")) {
        log(`Reconnect for ${sessionId} skipped — already in progress`);
        return;
      }
      logError("Failed to reconnect session", e);
    }
  }

  // Command pane rerun
  async function rerunCommand() {
    if (!instance) return;
    const command = instance.command;
    const workingDir = instance.workingDir;
    if (!command || !workingDir) return;

    // Kill old PTY if still running
    if (commandStatus === "running") {
      await killSession(instance.ptyId).catch(() => {});
    }

    // Clean up old listeners
    for (const unlisten of instance.unlisteners.splice(0)) {
      try { unlisten(); } catch {}
    }

    // Reset state
    const newPtyId = `${paneId}-${Date.now()}`;
    if (instance.elapsedTimer != null) {
      clearInterval(instance.elapsedTimer);
    }

    // Clear terminal
    instance.terminal?.clear();
    instance.terminal?.reset();

    updateInstance(paneId, {
      ptyId: newPtyId,
      commandStatus: "running",
      commandExitCode: null,
      commandStartedAt: Date.now(),
      elapsedTimer: setInterval(updateElapsed, 1000),
      outputChannel: null,
      unlisteners: [],
    });

    // Attach listeners before spawning
    const { attachPtyListeners } = await import("$lib/panes/terminals");
    await attachPtyListeners(paneId, (payload) => {
      const exitCode = payload.code;
      const status = exitCode === 0 ? "success" : "error";
      updateInstance(paneId, {
        commandStatus: status as "success" | "error",
        commandExitCode: exitCode,
      });
      const inst = $paneInstances.get(paneId);
      if (inst?.elapsedTimer != null) {
        clearInterval(inst.elapsedTimer);
        updateInstance(paneId, { elapsedTimer: null });
      }
      updateElapsed();
    });

    // Spawn new command
    await spawnTask(newPtyId, command, workingDir);
    // Attach output
    const inst = $paneInstances.get(paneId);
    if (inst && !inst.outputChannel) {
      const outputChannel = createPtyOutputChannel((bytes) => {
        inst.terminal?.write(bytes);
      });
      updateInstance(paneId, { outputChannel });
      await attachPtyOutput(newPtyId, outputChannel);
    } else if (inst?.outputChannel) {
      await attachPtyOutput(newPtyId, inst.outputChannel);
    }
  }

  function doAttach() {
    if (!containerEl || !instance?.terminal) {
      log(`PaneShell.doAttach(${paneId}): skipped (container=${!!containerEl}, terminal=${!!instance?.terminal}, type=${instance?.type})`);
      return;
    }
    if (!instance.terminal.element) {
      log(`PaneShell.doAttach(${paneId}): opening terminal in container`);
      attachToContainer(paneId, containerEl);
    } else if (!containerEl.contains(instance.terminal.element)) {
      log(`PaneShell.doAttach(${paneId}): re-parenting terminal element`);
      containerEl.appendChild(instance.terminal.element);
    }
    // Only schedule refit — setLogicalFocus handles DOM focus.
    resizeScheduler.schedule();
  }

  function doDetach() {
    if (!instance?.terminal?.element || !containerEl?.contains(instance.terminal.element)) return;
    containerEl.removeChild(instance.terminal.element);
  }

  onMount(() => {
    if (containerEl) {
      resizeObserver = new ResizeObserver(() => {
        if (visible && instance?.fitAddon) {
          resizeScheduler.schedule();
        }
      });
      resizeObserver.observe(containerEl);
    }

    if (visible && instance?.terminal) {
      doAttach();
    }

    // Start elapsed timer for running commands
    if (instance?.type === "command" && instance.commandStatus === "running") {
      updateElapsed();
    }
  });

  onDestroy(() => {
    resizeScheduler.cancel();
    resizeObserver?.disconnect();
    doDetach();
  });

  // Visibility effect: attach/detach terminal
  $effect(() => {
    if (visible && instance?.terminal) {
      doAttach();
    } else {
      doDetach();
    }
  });

  // Theme sync
  $effect(() => {
    if (instance?.terminal) {
      instance.terminal.options.theme = getXtermTheme($settings.theme);
    }
  });

  // Focus effect: refit when gaining logical focus (disableStdin just changed,
  // terminal may need resize).
  $effect(() => {
    if (isFocused && visible && instance?.terminal) {
      resizeScheduler.schedule();
    }
  });

  // Elapsed timer sync
  $effect(() => {
    if (instance?.type === "command" && instance.commandStatus === "running") {
      updateElapsed();
    }
  });
</script>

{#if instance}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="pane-shell relative flex flex-col flex-1 min-h-0 min-w-0 overflow-hidden bg-bg-deep"
    data-focused={isFocused}
    onmousedown={handleMouseDown}
  >
    <!-- Mini title bar -->
    <div
      class="pane-shell__titlebar flex h-6 shrink-0 select-none items-center border-b border-hairline px-2 gap-1.5"
      class:shadow-[inset_0_2px_0_var(--color-accent-dim)]={isFocused && !suppressTitleAccent}
      ondblclick={() => startRenaming(instance.name ?? "")}
    >
      <span class="text-[10px] uppercase tracking-wider text-text-muted/60 shrink-0">{paneTypeLabel(instance.type)}</span>
      {#if editingName}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          class="min-w-0 flex-1 bg-transparent text-[11px] text-text-primary font-mono outline-none placeholder:text-text-muted/40"
          placeholder="name this pane..."
          bind:value={nameInput}
          autofocus
          onblur={() => commitRename()}
          onkeydown={(e) => {
            if (e.key === "Enter") commitRename();
            if (e.key === "Escape") { editingName = false; }
          }}
        />
      {:else if instance.name}
        <span class="min-w-0 flex-1 truncate text-[11px] text-text-secondary font-mono">{instance.name}</span>
      {:else}
        <span class="flex-1"></span>
      {/if}
      {#if canClose()}
        <button
          class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-[12px] leading-none text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
          onclick={(e) => {
            e.stopPropagation();
            void closePane(sessionId, paneId);
          }}
          title="Close pane"
        >
          &times;
        </button>
      {/if}
    </div>

    <div class="flex-1 min-h-0 min-w-0">
      {#if instance.type === "claude" && isDisconnected && session}
        <div class="ui-terminal-frame h-full w-full overflow-hidden">
          <SessionPicker
            cwd={session.worktreePath}
            onContinue={handleContinue}
            onResume={handleResume}
            onNew={handleNew}
          />
        </div>
      {:else if instance.type === "markdown"}
        <LazyMarkdownPane docPath={instance.docPath ?? ""} />
      {:else if instance.type === "command"}
        <!-- Command pane: header bar + terminal -->
        <div class="relative flex h-full w-full flex-col bg-bg-deep">
          <div class="flex h-9 shrink-0 select-none items-center gap-2 border-b border-hairline bg-bg-surface/30 px-3">
            <span class="font-mono text-[11px] text-text-secondary truncate flex-1">{instance.command ?? ""}</span>
            <span class="text-[10px] text-text-muted font-mono">{elapsed}</span>
            {#if commandStatus === "running"}
              <span class="h-2 w-2 shrink-0 rounded-full bg-accent animate-pulse"></span>
              <button
                class="bg-transparent px-1 font-mono text-[10px] text-text-muted border-none cursor-pointer hover:text-red"
                onclick={() => { void killSession(instance.ptyId).catch(() => {}); }}
                title="Stop"
              >&#9632;</button>
            {:else}
              {#if commandStatus === "success"}
                <span class="text-[10px] text-green font-mono">exit 0</span>
              {:else}
                <span class="text-[10px] text-red font-mono">exit {commandExitCode ?? "?"}</span>
              {/if}
              <button
                class="bg-transparent px-1 font-mono text-[10px] text-text-muted border-none cursor-pointer hover:text-accent"
                onclick={() => void rerunCommand()}
                title="Rerun (r)"
              >&#8635;</button>
            {/if}
          </div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            bind:this={containerEl}
            class="ui-terminal-frame min-h-0 flex-1"
            onclick={() => instance?.terminal?.focus()}
          ></div>
        </div>
      {:else}
        <!-- claude or shell: just a terminal container -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          bind:this={containerEl}
          class="ui-terminal-frame h-full w-full overflow-hidden"
          onclick={() => instance?.terminal?.focus()}
        ></div>
      {/if}
    </div>
  </div>
{/if}
