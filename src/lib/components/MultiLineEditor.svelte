<script lang="ts">
  import { scale } from "svelte/transition";
  import { onDestroy } from "svelte";
  import { Tooltip } from "bits-ui";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import X from "@lucide/svelte/icons/x";
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

  // Panel width — kept in sync with the inline `w-[680px]` class below so
  // drag clamping uses the right horizontal dimension. (No height constant
  // needed: the y-clamp pins on the header, not the bottom edge.)
  const PANEL_WIDTH = 680;
  // Pixels of the panel header that must remain inside the viewport during
  // a drag, so the user can always grab it back.
  const MIN_VISIBLE = 80;
  const POSITION_STORAGE_KEY = "roux:multiLineEditor:position";

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

  // Everything a user can trigger while the editor has focus — shown on
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

  // null → fall back to defaultPosition() (centered horizontally, top at 14vh).
  let position = $state<{ x: number; y: number } | null>(null);
  let dragging = $state(false);
  let dragOffset = { x: 0, y: 0 };

  // Track the pane id we disabled input on so we can re-enable the right one
  // even if the store state races with a pane change.
  let disabledPaneId: string | null = null;

  $effect(() => {
    const state = $multiLineEditor;
    if (state.open && editorContainer && !editorView) {
      mountEditor(state.initialText);
      disableTargetPaneInput(state.paneId);
      position = loadPosition() ?? defaultPosition();
      window.addEventListener("resize", onWindowResize);
    } else if (!state.open && editorView) {
      tearDownEditor();
      restoreTargetPaneInput();
      window.removeEventListener("resize", onWindowResize);
    }
  });

  onDestroy(() => {
    tearDownEditor();
    restoreTargetPaneInput();
    window.removeEventListener("resize", onWindowResize);
  });

  function defaultPosition(): { x: number; y: number } {
    const x = Math.max(0, Math.round((window.innerWidth - PANEL_WIDTH) / 2));
    const y = Math.max(0, Math.round(window.innerHeight * 0.14));
    return { x, y };
  }

  function clampPosition(p: { x: number; y: number }): { x: number; y: number } {
    const maxX = window.innerWidth - MIN_VISIBLE;
    const maxY = window.innerHeight - MIN_VISIBLE;
    const minX = MIN_VISIBLE - PANEL_WIDTH;
    const minY = 0;
    return {
      x: Math.min(maxX, Math.max(minX, p.x)),
      y: Math.min(maxY, Math.max(minY, p.y)),
    };
  }

  function loadPosition(): { x: number; y: number } | null {
    try {
      const raw = localStorage.getItem(POSITION_STORAGE_KEY);
      if (!raw) return null;
      const parsed = JSON.parse(raw);
      if (typeof parsed?.x !== "number" || typeof parsed?.y !== "number") return null;
      return clampPosition(parsed);
    } catch {
      return null;
    }
  }

  function savePosition(p: { x: number; y: number }): void {
    try {
      localStorage.setItem(POSITION_STORAGE_KEY, JSON.stringify(p));
    } catch {
      // localStorage can be unavailable (private mode, quota); silently ignore.
    }
  }

  function onWindowResize(): void {
    if (position) position = clampPosition(position);
  }

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
      // Keep the editor open so the user can retry or copy the text out —
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

  // Skip drag init when the pointer landed on an interactive control inside
  // the header (e.g. the keyboard-shortcut tooltip trigger) — otherwise
  // clicking those would start a drag instead of activating them.
  function isInteractiveTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    return target.closest("button, [role='button']") !== null;
  }

  function onHeaderPointerDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    if (isInteractiveTarget(e.target)) return;
    if (!position) position = defaultPosition();
    dragOffset = { x: e.clientX - position.x, y: e.clientY - position.y };
    dragging = true;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onHeaderPointerMove(e: PointerEvent): void {
    if (!dragging) return;
    position = clampPosition({
      x: e.clientX - dragOffset.x,
      y: e.clientY - dragOffset.y,
    });
  }

  function onHeaderPointerUp(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as Element).releasePointerCapture(e.pointerId);
    if (position) savePosition(position);
  }
</script>

{#if $multiLineEditor.open}
  <Tooltip.Provider delayDuration={350} skipDelayDuration={150}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="ui-dialog fixed z-50 flex h-[480px] w-[680px] flex-col overflow-hidden rounded-2xl"
      style="top: {position?.y ?? 0}px; left: {position?.x ?? 0}px;"
      onkeydown={handleKeyDown}
      transition:scale={{ duration: 120, start: 0.985 }}
    >
      <!-- Header (drag handle) -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="flex items-center justify-between gap-3 px-4 pt-3 pb-3 select-none {dragging
          ? 'cursor-grabbing'
          : 'cursor-grab'}"
        onpointerdown={onHeaderPointerDown}
        onpointermove={onHeaderPointerMove}
        onpointerup={onHeaderPointerUp}
      >
        <div class="flex items-center gap-2.5">
          <button
            type="button"
            onclick={closeMultiLineEditor}
            class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
            aria-label="Close editor"
          >
            <X class="h-3.5 w-3.5" />
          </button>
          <div class="flex flex-col gap-0.5">
            <span class="text-[12px] font-medium text-text-primary">Edit prompt</span>
            <span class="text-[11px] text-text-muted">
              {$multiLineEditor.paneLabel ?? "pane"}
              {#if $multiLineEditor.seeded}
                <span class="text-text-muted/70"> · seeded from prompt</span>
              {/if}
            </span>
          </div>
        </div>
        <Tooltip.Root>
          <Tooltip.Trigger
            class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
            aria-label="Show keyboard shortcuts"
          >
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

      <!-- Toolbar -->
      <div class="flex flex-wrap items-center gap-1 border-b border-border-subtle bg-bg-surface/55 px-4 py-2.5">
        {#each toolbarActions as action (action.label)}
          <Tooltip.Root>
            <Tooltip.Trigger
              class="cursor-pointer rounded-md border border-transparent px-2.5 py-1 text-[11px] text-text-secondary transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
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
          <button
            class="inline-flex cursor-pointer items-center gap-2 rounded-xl border border-accent-dim/20 bg-accent-dim/15 px-3 py-1.5 text-[12px] font-medium text-accent transition-colors hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
            onclick={() => void submitInsert()}
          >
            <kbd
              class="rounded border border-accent-dim/30 bg-accent-dim/20 px-1.5 py-0.5 font-mono text-[10px] text-accent"
              >{formatShortcut("cmd+enter")}</kbd
            >
            <span>Insert</span>
          </button>
          <button
            class="inline-flex cursor-pointer items-center gap-2 rounded-xl border border-border-subtle bg-bg-surface px-3 py-1.5 text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
            onclick={closeMultiLineEditor}
          >
            <kbd
              class="rounded border border-border-subtle bg-bg-elevated px-1.5 py-0.5 font-mono text-[10px] text-text-muted"
              >Esc</kbd
            >
            <span>Cancel</span>
          </button>
        </div>
        <span class="text-[10px]">
          target · <span class="text-text-primary">{$multiLineEditor.paneLabel ?? "pane"}</span>
        </span>
      </div>
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
