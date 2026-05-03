<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { Tooltip } from "bits-ui";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import X from "@lucide/svelte/icons/x";

  import {
    MULTI_LINE_EDITOR_FOCUS_EVENT,
    multiLineEditor,
    closeMultiLineEditor,
    buildSubmitPayload,
    type MultiLineEditorFocusDetail,
  } from "$lib/stores/multiLineEditor";
  import { getTerminalController } from "$lib/panes/terminalRuntime";
  import { writeToSession } from "$lib/tauri";
  import { setLogicalFocus } from "$lib/panes/focus";
  import { getInstance, getAttachedPtyId } from "$lib/panes/instances";
  import { hasPrimaryModifier, formatShortcut, isMacPlatform } from "$lib/platform";
  import { settings } from "$lib/stores/settings";
  import { activeSession } from "$lib/stores/sessions";
  import { worktreeMetadataFor } from "$lib/stores/worktreeMetadata";
  import { paneInstances } from "$lib/panes/instances";
  import { profileRegistry } from "$lib/panes/profiles";
  import {
    clearBuffer,
    clearSelectedLines,
    copyAndClearCurrentLine,
    deleteToLineEnd,
    deleteToLineStart,
    deleteWordLeft,
    insertAtSelection,
    type TextEditState,
  } from "$lib/panes/textEditing";
  import { suggestCommandCorrection } from "$lib/panes/commandCorrections";
  import { buildMultiLineEditorContextChips, type MultiLineContextChipTone } from "$lib/panes/multiLineEditorContext";

  interface Props {
    paneId: string;
  }

  let { paneId: hostPaneId }: Props = $props();

  interface ShortcutEntry {
    shortcut: string;
    action: string;
  }

  // Everything a user can trigger while the editor has focus, shown on
  // hover of the keyboard hint in the header. ctrl+enter is mac-only:
  // on Windows/Linux ctrl IS the primary modifier, so cmd+enter renders
  // as "Ctrl+Enter" and listing ctrl+enter separately would conflict.
  const modalShortcuts: ShortcutEntry[] = [
    { shortcut: "cmd+enter", action: "Send to terminal" },
    { shortcut: "shift+enter", action: "Insert newline" },
    ...(isMacPlatform()
      ? [{ shortcut: "ctrl+enter", action: "Insert newline" }]
      : []),
    { shortcut: "alt+enter", action: "Insert newline" },
    { shortcut: "ctrl+c", action: "Clear editor when nothing is selected" },
    { shortcut: "ctrl+u", action: "Copy and clear current line" },
    { shortcut: "cmd+shift+k", action: "Clear selected lines" },
    { shortcut: "alt+backspace", action: "Delete word left" },
    { shortcut: "ctrl+w", action: "Delete word left" },
    { shortcut: "ctrl+k", action: "Delete to line end" },
    { shortcut: "cmd+backspace", action: "Delete to line start" },
    { shortcut: "cmd+delete", action: "Delete to line end" },
    { shortcut: "escape", action: "Cancel without writing" },
    { shortcut: "ctrl+g", action: "Cancel without writing" },
  ];

  let textareaEl: HTMLTextAreaElement | undefined = $state();
  let draftText = $state("");
  let activePaneId: string | null = null;

  // Track the pane that should regain logical focus when the editor closes,
  // even if the store state races with a pane change.
  let disabledPaneId: string | null = null;
  const isVisible = $derived($multiLineEditor.open && $multiLineEditor.paneId === hostPaneId);
  const commandCorrection = $derived(
    $multiLineEditor.target === "shell" ? suggestCommandCorrection(draftText) : null,
  );
  const editorPane = $derived($paneInstances.get(hostPaneId) ?? null);
  const sessionMetadata = $derived(
    $activeSession?.worktreePath ? worktreeMetadataFor($activeSession.worktreePath) : null,
  );
  const wtMeta = $derived(sessionMetadata ? $sessionMetadata : null);
  const profileName = $derived(profileLabel(editorPane));
  const contextChips = $derived(buildMultiLineEditorContextChips({
    pane: editorPane,
    session: $activeSession,
    target: $multiLineEditor.target,
    metadata: wtMeta,
    profileName,
  }));

  $effect(() => {
    const state = $multiLineEditor;
    if (isVisible && state.paneId && activePaneId !== state.paneId) {
      restoreTargetPaneInput();
      activePaneId = state.paneId;
      draftText = state.initialText;
      disableTargetPaneInput(state.paneId);
      focusEditor();
    } else if (!isVisible && activePaneId) {
      activePaneId = null;
      restoreTargetPaneInput();
    }
  });

  onDestroy(() => {
    activePaneId = null;
    restoreTargetPaneInput();
  });

  onMount(() => {
    function handleFocusRequest(event: Event): void {
      const detail = (event as CustomEvent<MultiLineEditorFocusDetail>).detail;
      if (!isVisible || detail?.paneId !== hostPaneId) return;
      focusEditor();
    }

    window.addEventListener(MULTI_LINE_EDITOR_FOCUS_EVENT, handleFocusRequest);
    return () => window.removeEventListener(MULTI_LINE_EDITOR_FOCUS_EVENT, handleFocusRequest);
  });

  function focusEditor(): void {
    requestAnimationFrame(() => textareaEl?.focus());
  }

  function profileLabel(pane: typeof editorPane): string | null {
    const ref = pane?.spawnProfileRef;
    if (!ref) return null;
    if (ref.kind === "inline") return ref.profile.name;
    return $profileRegistry.get(ref.id)?.name ?? ref.id;
  }

  function chipClass(tone: MultiLineContextChipTone): string {
    switch (tone) {
      case "accent":
        return "border-accent-dim/45 bg-accent-dim/12 text-text-primary";
      case "warn":
        return "border-yellow/35 bg-yellow/10 text-yellow";
      default:
        return "border-border-subtle/70 bg-bg-surface/35 text-text-muted";
    }
  }

  function disableTargetPaneInput(paneId: string | null): void {
    if (!paneId) return;
    // Keep xterm stdin enabled while the editor owns DOM focus. Disabling
    // xterm here made focus restoration fragile and also changed how programmatic
    // input/paste behaved for shell panes.
    disabledPaneId = paneId;
  }

  function restoreTargetPaneInput(): void {
    if (!disabledPaneId) return;
    const paneId = disabledPaneId;
    const controller = getTerminalController(paneId);
    setLogicalFocus(paneId);
    controller?.setInputEnabled(true);
    controller?.focus();
    requestAnimationFrame(() => {
      const nextController = getTerminalController(paneId);
      nextController?.setInputEnabled(true);
      nextController?.focus();
    });
    disabledPaneId = null;
  }

  function waitForTerminalInputTurn(): Promise<void> {
    return new Promise((resolve) => window.setTimeout(resolve, 16));
  }

  function currentText(): string {
    return draftText;
  }

  function currentEditorState(): TextEditState | null {
    if (!textareaEl) return null;
    return {
      value: draftText,
      selectionStart: textareaEl.selectionStart,
      selectionEnd: textareaEl.selectionEnd,
    };
  }

  async function applyTextEdit(next: TextEditState): Promise<void> {
    draftText = next.value;
    await tick();
    textareaEl?.focus();
    textareaEl?.setSelectionRange(next.selectionStart, next.selectionEnd);
  }

  function handleTextEdit(e: KeyboardEvent, next: TextEditState): void {
    e.preventDefault();
    e.stopPropagation();
    void applyTextEdit(next);
  }

  async function copyCurrentLineAndClear(): Promise<void> {
    const state = currentEditorState();
    if (!state) return;
    const next = copyAndClearCurrentLine(state);
    try {
      await navigator.clipboard.writeText(next.clipboardText);
      await applyTextEdit(next);
    } catch (err) {
      console.error("MultiLineEditor: clipboard write failed", err);
    }
  }

  async function applyCurrentCorrection(): Promise<void> {
    const correction = commandCorrection;
    if (!correction) return;
    const cursor = correction.replacement.length;
    await applyTextEdit({
      value: correction.replacement,
      selectionStart: cursor,
      selectionEnd: cursor,
    });
  }

  function normalizeSubmitText(text: string): string {
    return text.replace(/[\r\n]+$/g, "");
  }

  async function submitViaTerminalController(
    paneId: string,
    target: string,
    text: string,
  ): Promise<boolean> {
    const controller = getTerminalController(paneId);
    if (!controller) return false;
    // Non-shell targets (Claude TUI) require explicit bracketed paste
    // markers regardless of xterm's current bracketed-paste mode state.
    // Fall through to the buildSubmitPayload + writeToSession path which
    // emits those markers unconditionally.
    if (target !== "shell") return false;

    controller.clearSelection();
    controller.scrollToBottom();
    controller.setInputEnabled(true);

    const normalized = normalizeSubmitText(text);
    controller.input("\x05\x15");
    if (normalized.includes("\n")) {
      controller.paste(normalized);
    } else {
      controller.input(normalized);
    }

    await waitForTerminalInputTurn();
    controller.input("\r");
    controller.scrollToBottom();
    requestAnimationFrame(() => getTerminalController(paneId)?.scrollToBottom());
    focusEditor();
    return true;
  }

  async function submitInsert(): Promise<void> {
    const state = $multiLineEditor;
    const text = currentText();
    if (!state.open || !state.paneId) {
      return;
    }
    const paneId = state.paneId;
    const paneInst = getInstance(paneId);
    if (!paneInst) {
      return;
    }
    const ptyId = getAttachedPtyId(paneInst);
    if (!ptyId) {
      return;
    }

    try {
      if (await submitViaTerminalController(paneId, state.target, text)) return;

      const payload = buildSubmitPayload(text, state.target);
      await writeToSession(ptyId, payload);
      await waitForTerminalInputTurn();
      await writeToSession(ptyId, "\r");
      focusEditor();
    } catch (err) {
      // Keep the editor open so the user can retry or copy the text out —
      // silently closing on a failed PTY write would drop their edits.
      console.error("MultiLineEditor: submit failed", err);
    }
  }

  function handleKeyDown(e: KeyboardEvent): void {
    // In-flight composition (IME): do not intercept.
    if (e.isComposing) return;

    if (e.key === "Escape" || (e.ctrlKey && e.key === "g")) {
      e.preventDefault();
      e.stopPropagation();
      closeMultiLineEditor();
      return;
    }

    if (e.key === "Enter" && hasPrimaryModifier(e) && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      e.stopPropagation();
      void submitInsert();
      return;
    }

    const state = currentEditorState();
    if (!state) return;
    const key = e.key.toLowerCase();
    const hasSelection = state.selectionStart !== state.selectionEnd;

    if (e.key === "Enter" && !hasPrimaryModifier(e) && (e.shiftKey || e.ctrlKey || e.altKey)) {
      handleTextEdit(e, insertAtSelection(state, "\n"));
      return;
    }

    if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && key === "c" && !hasSelection) {
      handleTextEdit(e, clearBuffer(state));
      return;
    }

    if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && key === "u") {
      e.preventDefault();
      e.stopPropagation();
      void copyCurrentLineAndClear();
      return;
    }

    if (hasPrimaryModifier(e) && e.shiftKey && !e.altKey && key === "k") {
      handleTextEdit(e, clearSelectedLines(state));
      return;
    }

    if (e.altKey && !e.metaKey && !e.ctrlKey && !e.shiftKey && e.key === "Backspace") {
      handleTextEdit(e, deleteWordLeft(state));
      return;
    }

    if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && key === "w") {
      handleTextEdit(e, deleteWordLeft(state));
      return;
    }

    if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && key === "k") {
      handleTextEdit(e, deleteToLineEnd(state));
      return;
    }

    if (hasPrimaryModifier(e) && !e.shiftKey && !e.altKey && e.key === "Backspace") {
      handleTextEdit(e, deleteToLineStart(state));
      return;
    }

    if (hasPrimaryModifier(e) && !e.shiftKey && !e.altKey && e.key === "Delete") {
      handleTextEdit(e, deleteToLineEnd(state));
      return;
    }
  }
</script>

{#if isVisible}
  <Tooltip.Provider delayDuration={350} skipDelayDuration={150}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      data-multiline-editor-root
      class="ui-dialog z-20 flex h-[clamp(88px,14vh,148px)] shrink-0 flex-col overflow-hidden rounded-none border-x-0 border-b-0"
      onkeydown={handleKeyDown}
    >
      <!-- Header -->
      <div class="flex h-7 select-none items-center justify-between gap-2 border-b border-border-subtle bg-bg-surface/25 px-2">
        <div class="flex min-w-0 items-center gap-1.5">
          <button
            type="button"
            onclick={closeMultiLineEditor}
            class="flex h-5 w-5 cursor-pointer items-center justify-center rounded border border-transparent bg-transparent text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
            aria-label="Close editor"
          >
            <X class="h-3 w-3" />
          </button>
          <span
            id={`mle-title-${hostPaneId}`}
            class="min-w-0 truncate text-[10px] font-medium uppercase tracking-wider text-text-muted"
          >
            {$multiLineEditor.paneLabel ?? "pane"}
            {#if $multiLineEditor.seeded}
              <span class="text-text-muted/60"> seeded</span>
            {/if}
          </span>
          <div class="flex min-w-0 items-center gap-1 overflow-hidden">
            {#each contextChips as chip (chip.kind)}
              <span
                class={`inline-flex h-5 max-w-[160px] shrink min-w-0 items-center truncate rounded border px-1.5 text-[10px] leading-none ${chipClass(chip.tone)}`}
                title={chip.title}
                data-context-chip={chip.kind}
              >
                <span class="min-w-0 truncate">{chip.label}</span>
              </span>
            {/each}
          </div>
          {#if commandCorrection}
            <button
              type="button"
              class="flex h-5 max-w-[220px] cursor-pointer items-center gap-1 overflow-hidden rounded border border-accent-dim/40 bg-accent-dim/10 px-1.5 text-[10px] text-text-secondary transition-colors hover:border-accent-dim hover:bg-accent-dim/20 hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              title={commandCorrection.description}
              aria-label={commandCorrection.description}
              onclick={() => void applyCurrentCorrection()}
            >
              <span class="shrink-0 text-text-muted">Fix</span>
              <span class="min-w-0 truncate font-mono">{commandCorrection.label}</span>
            </button>
          {/if}
        </div>
        <div class="flex shrink-0 items-center gap-1">
          <Tooltip.Root>
            <Tooltip.Trigger
              class="flex h-5 w-5 cursor-pointer items-center justify-center rounded border border-transparent bg-transparent text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              aria-label="Show keyboard shortcuts"
            >
              <Keyboard class="h-3 w-3" />
            </Tooltip.Trigger>
            <Tooltip.Portal>
              <Tooltip.Content sideOffset={6} class="mle-tooltip mle-tooltip-grid">
                <div class="mle-tooltip-title">Shortcuts</div>
                {#each modalShortcuts as entry (entry.shortcut)}
                  <kbd class="mle-tooltip-kbd">{formatShortcut(entry.shortcut)}</kbd>
                  <span>{entry.action}</span>
                {/each}
              </Tooltip.Content>
            </Tooltip.Portal>
          </Tooltip.Root>
          <button
            class="inline-flex h-5 cursor-pointer items-center gap-1 rounded border border-accent bg-accent px-2 text-[10px] font-medium text-bg-deep transition-colors hover:bg-accent hover:opacity-90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent/50"
            onclick={() => void submitInsert()}
          >
            <span>Send</span>
          </button>
        </div>
      </div>

      <!-- Editor -->
      <textarea
        bind:this={textareaEl}
        bind:value={draftText}
        aria-labelledby={`mle-title-${hostPaneId}`}
        class="min-h-0 flex-1 resize-none border-0 bg-transparent px-2.5 py-1.5 text-text-primary outline-none placeholder:text-text-muted/60"
        style="font-size: {$settings.fontSize}px; font-family: {$settings.fontFamily};"
        spellcheck="false"
        onkeydown={handleKeyDown}
      ></textarea>
    </div>
  </Tooltip.Provider>
{/if}

<style>
  /* bits-ui Tooltip Content only accepts a `class` prop (no slot for inline
     class names from the parent), so the tooltip styles have to live as
     :global rules. Everything else is utility-driven. */
  :global(.mle-tooltip) {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 8px;
    background: var(--color-bg-elevated, rgba(20, 20, 20, 0.98));
    color: var(--color-text-primary);
    border: 1px solid var(--color-border-subtle);
    box-shadow: 0 8px 24px -6px rgba(0, 0, 0, 0.35);
    font-size: 11px;
    z-index: 70;
    pointer-events: none;
  }
  :global(.mle-tooltip-kbd) {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--color-bg-surface, rgba(255, 255, 255, 0.06));
    border: 1px solid var(--color-border-subtle);
    color: var(--color-text-muted);
    justify-self: start;
  }
  :global(.mle-tooltip-grid) {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 12px;
    align-items: center;
    padding: 10px 12px;
  }
  :global(.mle-tooltip-title) {
    grid-column: 1 / -1;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.22em;
    color: var(--color-text-muted);
    margin-bottom: 4px;
  }
</style>
