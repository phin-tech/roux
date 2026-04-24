<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { onDestroy } from "svelte";
  import { Tooltip } from "bits-ui";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView, keymap } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import {
    syntaxHighlighting,
    defaultHighlightStyle,
    StreamLanguage,
  } from "@codemirror/language";
  import { shell } from "@codemirror/legacy-modes/mode/shell";

  import {
    multiLineEditor,
    closeMultiLineEditor,
    buildSubmitPayload,
  } from "$lib/stores/multiLineEditor";
  import { getTerminalController } from "$lib/panes/terminalRuntime";
  import { writeToSession } from "$lib/tauri";
  import { setLogicalFocus } from "$lib/panes/focus";
  import { getInstance, getAttachedPtyId } from "$lib/panes/instances";
  import { hasPrimaryModifier, formatShortcut } from "$lib/platform";
  import { settings } from "$lib/stores/settings";
  import {
    joinLines,
    smartQuotesToStraight,
    stripCodeFence,
    stripPromptPrefix,
    trimDocument,
    unwrapContinuations,
  } from "$lib/panes/textTransforms";

  interface ToolbarAction {
    label: string;
    description: string;
    shortcut: string | null;
    transform: (text: string) => string;
  }

  const toolbarActions: ToolbarAction[] = [
    {
      label: "Join lines",
      description: "Collapse newlines + whitespace into one line",
      shortcut: "cmd+j",
      transform: joinLines,
    },
    {
      label: "Unwrap \\",
      description: "Remove trailing \\-newline continuations",
      shortcut: null,
      transform: unwrapContinuations,
    },
    {
      label: "Strip $ / ❯",
      description: "Strip leading $, ❯, #, > prompt markers",
      shortcut: null,
      transform: stripPromptPrefix,
    },
    {
      label: "Strip ```",
      description: "Remove leading / trailing markdown code fences",
      shortcut: null,
      transform: stripCodeFence,
    },
    {
      label: "Smart → straight",
      description: "Replace curly quotes with straight ASCII quotes",
      shortcut: null,
      transform: smartQuotesToStraight,
    },
    {
      label: "Trim",
      description: "Strip leading and trailing whitespace",
      shortcut: null,
      transform: trimDocument,
    },
  ];

  interface ShortcutEntry {
    shortcut: string;
    action: string;
  }

  // Everything a user can trigger while the modal has focus — shown on
  // hover of the keyboard hint in the header. Covers both our custom
  // keybindings and the CodeMirror defaults that users commonly rely on.
  const modalShortcuts: ShortcutEntry[] = [
    { shortcut: "cmd+enter", action: "Insert into terminal (no auto-execute)" },
    { shortcut: "escape", action: "Cancel — nothing written" },
    { shortcut: "ctrl+g", action: "Cancel — nothing written" },
    { shortcut: "cmd+j", action: "Join lines transform" },
    { shortcut: "cmd+z", action: "Undo transform / edit" },
    { shortcut: "cmd+shift+z", action: "Redo" },
  ];

  let editorContainer: HTMLElement | undefined = $state();
  let editorView: EditorView | null = null;

  // Track the pane id we disabled input on so we can re-enable the right one
  // even if the store state races with a pane change.
  let disabledPaneId: string | null = null;

  $effect(() => {
    const state = $multiLineEditor;
    if (state.open && editorContainer && !editorView) {
      mountEditor(state.initialText);
      disableTargetPaneInput(state.paneId);
    } else if (!state.open && editorView) {
      tearDownEditor();
      restoreTargetPaneInput();
    }
  });

  onDestroy(() => {
    tearDownEditor();
    restoreTargetPaneInput();
  });

  function mountEditor(initialText: string): void {
    if (!editorContainer) return;
    const extensions: Extension[] = [
      EditorView.lineWrapping,
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      syntaxHighlighting(defaultHighlightStyle),
      EditorView.theme({
        "&": {
          height: "100%",
          fontSize: `${$settings.fontSize}px`,
          fontFamily: $settings.fontFamily,
        },
        ".cm-content": {
          caretColor: "var(--color-text-primary)",
          color: "var(--color-text-primary)",
          padding: "0.75rem 1rem",
        },
        ".cm-cursor": {
          borderLeftColor: "var(--color-text-primary)",
        },
        "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
          backgroundColor: "var(--color-bg-active) !important",
        },
        ".cm-scroller": {
          overflow: "auto",
        },
        "&.cm-focused": {
          outline: "none",
        },
      }),
    ];

    if ($multiLineEditor.target === "shell") {
      extensions.push(StreamLanguage.define(shell));
    }

    editorView = new EditorView({
      state: EditorState.create({ doc: initialText, extensions }),
      parent: editorContainer,
    });
    requestAnimationFrame(() => editorView?.focus());
  }

  function tearDownEditor(): void {
    editorView?.destroy();
    editorView = null;
  }

  function disableTargetPaneInput(paneId: string | null): void {
    if (!paneId) return;
    const controller = getTerminalController(paneId);
    if (!controller) return;
    controller.setInputEnabled(false);
    disabledPaneId = paneId;
  }

  function restoreTargetPaneInput(): void {
    if (!disabledPaneId) return;
    const controller = getTerminalController(disabledPaneId);
    if (controller) {
      controller.setInputEnabled(true);
      controller.focus();
    }
    setLogicalFocus(disabledPaneId);
    disabledPaneId = null;
  }

  function currentText(): string {
    return editorView?.state.doc.toString() ?? "";
  }

  function applyTransform(fn: (text: string) => string): void {
    if (!editorView) return;
    const current = currentText();
    const next = fn(current);
    if (next === current) return;
    editorView.dispatch({
      changes: { from: 0, to: editorView.state.doc.length, insert: next },
    });
    editorView.focus();
  }

  async function submitInsert(): Promise<void> {
    const state = $multiLineEditor;
    if (!state.open || !state.paneId) return;
    const paneInst = getInstance(state.paneId);
    if (!paneInst) return;
    const ptyId = getAttachedPtyId(paneInst);
    if (!ptyId) return;
    const payload = buildSubmitPayload(currentText(), state.target);

    try {
      await writeToSession(ptyId, payload);
      closeMultiLineEditor();
    } catch (err) {
      // Keep the modal open so the user can retry or copy the text out —
      // silently closing on a failed PTY write would drop their edits.
      // eslint-disable-next-line no-console
      console.error("MultiLineEditor: writeToSession failed", err);
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

    if (e.key === "j" && hasPrimaryModifier(e) && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      e.stopPropagation();
      applyTransform(joinLines);
      return;
    }
  }

  // Track whether the *mouse-down* started on the backdrop. Using `click`
  // alone closes the modal even when the user drags from inside the editor
  // out to the backdrop to make a text selection — the browser fires
  // `click` on the common ancestor (the backdrop). By gating on where the
  // pointer went DOWN, a selection drag no longer trips closure.
  let mouseDownOnBackdrop = false;

  function onBackdropPointerDown(e: PointerEvent): void {
    mouseDownOnBackdrop = e.target === e.currentTarget;
  }

  function onBackdropClick(e: MouseEvent): void {
    if (e.target === e.currentTarget && mouseDownOnBackdrop) {
      closeMultiLineEditor();
    }
    mouseDownOnBackdrop = false;
  }
</script>

{#if $multiLineEditor.open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-start justify-center bg-black/65 pt-[14vh] backdrop-blur-md"
    onpointerdown={onBackdropPointerDown}
    onclick={onBackdropClick}
    onkeydown={handleKeyDown}
    transition:fade={{ duration: 120 }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <Tooltip.Provider delayDuration={350} skipDelayDuration={150}>
    <div
      class="ui-dialog flex h-[480px] w-[680px] flex-col overflow-hidden rounded-[1.4rem] border-l-2 border-l-accent"
      transition:scale={{ duration: 120, start: 0.985 }}
    >
      <!-- Header -->
      <div class="flex items-center justify-between px-5 pt-4 pb-3 text-[11px] uppercase tracking-[0.22em] text-text-muted">
        <span>
          Editing prompt for
          <span class="text-text-primary normal-case tracking-normal ml-1">
            {$multiLineEditor.paneLabel ?? "pane"}
          </span>
        </span>
        <div class="flex items-center gap-3">
          {#if $multiLineEditor.seeded}
            <span class="text-[10px] normal-case tracking-normal text-text-muted">
              Seeded from prompt
            </span>
          {/if}
          <Tooltip.Root>
            <Tooltip.Trigger class="mle-icon-btn" aria-label="Show keyboard shortcuts">
              <Keyboard class="w-3.5 h-3.5" />
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
        </div>
      </div>

      <!-- Toolbar -->
      <div class="flex flex-wrap items-center gap-1.5 border-b border-border-subtle bg-bg-surface/55 px-4 py-2.5">
        {#each toolbarActions as action (action.label)}
          <Tooltip.Root>
            <Tooltip.Trigger
              class="mle-btn"
              onclick={() => applyTransform(action.transform)}
            >
              {action.label}
            </Tooltip.Trigger>
            <Tooltip.Portal>
              <Tooltip.Content sideOffset={6} class="mle-tooltip">
                <span>{action.description}</span>
                {#if action.shortcut}
                  <kbd class="mle-tooltip-kbd">{formatShortcut(action.shortcut)}</kbd>
                {/if}
              </Tooltip.Content>
            </Tooltip.Portal>
          </Tooltip.Root>
        {/each}
      </div>

      <!-- Editor -->
      <div bind:this={editorContainer} class="flex-1 overflow-hidden"></div>

      <!-- Footer -->
      <div class="flex items-center justify-between border-t border-border-subtle px-4 py-2.5 text-[11px] text-text-muted">
        <div class="flex items-center gap-2">
          <button class="mle-footer-btn mle-footer-btn-primary" onclick={() => void submitInsert()}>
            <kbd class="mle-kbd">{formatShortcut("cmd+enter")}</kbd>
            <span>Insert</span>
          </button>
          <button class="mle-footer-btn" onclick={closeMultiLineEditor}>
            <kbd class="mle-kbd">Esc</kbd>
            <span>Cancel</span>
          </button>
        </div>
        <span class="text-[10px]">
          target · <span class="text-text-primary">{$multiLineEditor.paneLabel ?? "pane"}</span>
        </span>
      </div>
    </div>
    </Tooltip.Provider>
  </div>
{/if}

<style>
  :global(.mle-btn) {
    padding: 4px 10px;
    border-radius: 8px;
    background: transparent;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border-subtle);
    font-size: 11px;
    cursor: pointer;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  :global(.mle-btn:hover) {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
    border-color: var(--color-border);
  }
  :global(.mle-kbd) {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--color-bg-elevated);
    border: 1px solid var(--color-border-subtle);
    color: var(--color-text-muted);
  }
  :global(.border-l-accent) {
    border-left-color: var(--color-border);
  }
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
  :global(.mle-icon-btn) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    background: transparent;
    color: var(--color-text-muted);
    border: 1px solid transparent;
    cursor: pointer;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  :global(.mle-icon-btn:hover) {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
    border-color: var(--color-border-subtle);
  }
  :global(.mle-footer-btn) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 4px 6px;
    border-radius: 8px;
    background: transparent;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border-subtle);
    font-size: 11px;
    cursor: pointer;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  :global(.mle-footer-btn:hover) {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
    border-color: var(--color-border);
  }
  :global(.mle-footer-btn-primary) {
    background: var(--color-bg-active);
    color: var(--color-text-primary);
    border-color: var(--color-border);
  }
  :global(.mle-footer-btn-primary:hover) {
    background: var(--color-bg-active);
    filter: brightness(1.12);
  }
</style>
