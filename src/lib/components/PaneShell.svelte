<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import "@xterm/xterm/css/xterm.css";
  import { paneInstances, attachToContainer, updateInstance } from "$lib/panes/instances";
  import { focusedPaneId, setLogicalFocus } from "$lib/panes/focus";
  import { closePane } from "$lib/panes/actions";
  import { resolveProfileRef } from "$lib/panes/profiles";
  import { runProfileInPane } from "$lib/panes/profileRunner";
  import { createResizeScheduler } from "$lib/panes/resizeScheduler";
  import {
    resizeSession,
    killPty,
    spawnTask,
    attachPtyOutput,
    createPtyOutputChannel,
    notificationsPush,
  } from "$lib/tauri";
  import { sessionState } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import { showPaneHints, paneSlotById } from "$lib/stores/ui";
  import { getXtermTheme } from "$lib/themes";
  import { reconnectSessionShell, retryShellPane } from "$lib/sessions/reconnect";
  import { log, logError } from "$lib/logging";
  import SessionPicker from "./SessionPicker.svelte";
  import LazyMarkdownPane from "./LazyMarkdownPane.svelte";
  import DeadPaneView from "./DeadPaneView.svelte";

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
  const paneSlot = $derived($paneSlotById.get(paneId) ?? null);
  const paneSlotLabel = $derived(
    paneSlot == null ? null : paneSlot === 10 ? "0" : String(paneSlot),
  );
  // A pane is "disconnected" (showing the resume picker) when it hosts the
  // session-owned PTY (ptyId === sessionId) and the session itself is in a
  // disconnected state. "disconnected" is the one session-level status the
  // backend owns exclusively — per-pane AgentState has no equivalent, since
  // dead-PTY is a session fact, not an agent observation — so we read
  // `session.status` directly here rather than going through
  // `computeEffectiveSessionStatus`. For every *other* UI decision that
  // needs a single indicator, consumers go through that helper instead.
  const isSessionPrimary = $derived(!!instance && instance.ptyId === sessionId);
  const isDisconnected = $derived(isSessionPrimary && session?.status === "disconnected");

  // Command pane status helpers
  const commandStatus = $derived(instance?.commandStatus ?? "idle");
  const commandExitCode = $derived(instance?.commandExitCode ?? null);

  // Resolved profile for the "Re-run profile" button. Built-in / user
  // refs resolve against the live registry; inline refs carry the profile
  // on the pane itself. Null when the pane has no profile attached or the
  // registered profile was deleted out from under it.
  const activeProfile = $derived(resolveProfileRef(instance?.spawnProfileRef));
  const canReRunProfile = $derived(
    !!activeProfile && (!!activeProfile.setupCommand || !!activeProfile.startupCommand),
  );

  // Dispatch for the disconnected reconnect UI: Claude built-in takes the
  // legacy SessionPicker (Continue/Resume/New via `claude --continue` etc.)
  // because the backend spawns the claude binary directly. Every other
  // profile — Codex, Plain shell, user-defined, inline — takes the
  // generic shell-reconnect path, which respawns a plain shell and
  // replays the profile's commands.
  const isClaudeBuiltinPrimary = $derived(
    isSessionPrimary &&
      activeProfile?.id === "claude" &&
      activeProfile?.source === "builtin",
  );

  async function reRunProfile() {
    if (!instance || !activeProfile) return;
    log(`Re-running profile "${activeProfile.id}" in pane ${paneId}`);
    try {
      await runProfileInPane(instance.ptyId, activeProfile);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      logError(`Re-run profile "${activeProfile.id}" failed`, e);
      // Surface the failure as a notification so the user isn't left
      // wondering why the button had no effect. The shell stays alive;
      // the user can hand-type whatever didn't get seeded.
      void notificationsPush({
        level: "warning",
        source: { type: "internal" },
        title: `Re-run failed: ${activeProfile.name}`,
        subtitle: null,
        body: msg,
        sessionId,
        actions: [
          {
            id: "dismiss",
            label: "Dismiss",
            kind: { type: "dismiss" },
            primary: true,
          },
        ],
      }).catch((pushErr) =>
        logError("re-run profile: notificationsPush failed", pushErr),
      );
    }
  }

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
    return !!instance;
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
      await reconnectSessionShell(session, extraFlags);
    } catch (e: any) {
      if (e?.message?.includes("already in progress")) {
        log(`Reconnect for ${sessionId} skipped — already in progress`);
        return;
      }
      logError("Failed to reconnect session", e);
    }
  }

  async function reconnectShell() {
    if (!session) return;
    try {
      await reconnectSessionShell(session);
    } catch (e: any) {
      if (e?.message?.includes("already in progress")) {
        log(`Reconnect for ${sessionId} skipped — already in progress`);
        return;
      }
      logError("Failed to reconnect shell session", e);
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
      await killPty(instance.ptyId).catch(() => {});
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
    await spawnTask(newPtyId, command, workingDir, sessionId, paneId);
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
      {#if canReRunProfile}
        <button
          class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-[11px] leading-none text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
          onclick={(e) => {
            e.stopPropagation();
            void reRunProfile();
          }}
          title={activeProfile ? `Re-run profile: ${activeProfile.name}` : "Re-run profile"}
        >
          &#8635;
        </button>
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
      {#if instance.restoreError}
        <DeadPaneView
          error={instance.restoreError}
          workingDir={instance.workingDir}
          onRetry={() => void retryShellPane(paneId, sessionId)}
          onClose={() => void closePane(sessionId, paneId)}
        />
      {:else if isDisconnected && session && isClaudeBuiltinPrimary}
        <!-- Claude built-in profile: Continue / Resume / New picker -->
        <div class="ui-terminal-frame h-full w-full overflow-hidden">
          <SessionPicker
            cwd={session.worktreePath}
            onContinue={handleContinue}
            onResume={handleResume}
            onNew={handleNew}
          />
        </div>
      {:else if isDisconnected && session}
        <!-- Any other profile: plain reconnect button that respawns a
             shell and replays the profile's commands. -->
        <div class="ui-terminal-frame flex h-full w-full flex-col items-center justify-center gap-3 bg-bg-deep p-6 text-center">
          <span class="text-[11px] uppercase tracking-wider text-text-muted">
            Session disconnected
          </span>
          <span class="max-w-xs text-[13px] text-text-secondary">
            {#if activeProfile}
              Reconnect will respawn a shell and re-run the <span class="font-mono text-text-primary">{activeProfile.name}</span> profile.
            {:else}
              Reconnect will respawn a plain shell in this pane.
            {/if}
          </span>
          <button
            class="cursor-pointer rounded-xl border border-accent-dim/20 bg-accent-dim/15 px-5 py-2 text-[13px] font-medium text-accent hover:bg-accent-dim/24"
            onclick={() => void reconnectShell()}
          >
            Reconnect
          </button>
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
                onclick={() => { void killPty(instance.ptyId).catch(() => {}); }}
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
        <!-- shell: just a terminal container -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          bind:this={containerEl}
          class="ui-terminal-frame h-full w-full overflow-hidden"
          onclick={() => instance?.terminal?.focus()}
        ></div>
      {/if}
    </div>

    {#if paneSlotLabel}
      <div
        class="pane-slot-hint pointer-events-none absolute inset-0 flex items-center justify-center bg-bg-deep/70 backdrop-blur-[1px]"
        class:pane-slot-hint--visible={$showPaneHints}
        aria-hidden="true"
      >
        <span class="font-mono text-[64px] font-bold leading-none text-text-primary drop-shadow-[0_2px_8px_rgba(0,0,0,0.7)]">
          &#8997;{paneSlotLabel}
        </span>
      </div>
    {/if}
  </div>
{/if}

<style>
  .pane-slot-hint {
    opacity: 0;
    transition: opacity 120ms ease-out;
  }
  .pane-slot-hint--visible {
    opacity: 1;
  }
</style>
