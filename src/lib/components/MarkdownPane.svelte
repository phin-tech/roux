<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView, keymap } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
  import { languages } from "@codemirror/language-data";
  import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
  import { vim } from "@replit/codemirror-vim";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { readFile, writeFile } from "$lib/tauri";
  import { hasPrimaryModifier } from "$lib/platform";
  import { settings } from "$lib/stores/settings";
  import CloseButton from "./CloseButton.svelte";

  interface Props {
    docPath?: string;
  }

  let { docPath = "" }: Props = $props();

  interface EditorTab {
    id: string;
    filePath: string | null;
    content: string;
    dirty: boolean;
  }

  let tabs = $state<EditorTab[]>([]);
  let activeTabId = $state<string>("");
  let editorContainer: HTMLElement | undefined = $state();
  let editorView: EditorView | null = null;
  let vimEnabled = $state(false);

  function createTab(filePath: string | null, content: string): EditorTab {
    return {
      id: crypto.randomUUID(),
      filePath,
      content,
      dirty: false,
    };
  }

  function tabName(tab: EditorTab): string {
    if (!tab.filePath) return "Untitled";
    const parts = tab.filePath.split("/");
    return parts[parts.length - 1];
  }

  function switchTab(tabId: string) {
    // Save current editor content to current tab before switching
    if (editorView && activeTabId) {
      const current = tabs.find((t) => t.id === activeTabId);
      if (current) {
        current.content = editorView.state.doc.toString();
      }
    }
    activeTabId = tabId;
    const tab = tabs.find((t) => t.id === tabId);
    if (tab && editorView) {
      editorView.dispatch({
        changes: {
          from: 0,
          to: editorView.state.doc.length,
          insert: tab.content,
        },
      });
    }
  }

  function addScratchpad() {
    const tab = createTab(null, "");
    tabs = [...tabs, tab];
    switchTab(tab.id);
  }

  function closeTab(tabId: string) {
    const idx = tabs.findIndex((t) => t.id === tabId);
    if (idx === -1) return;
    tabs = tabs.filter((t) => t.id !== tabId);
    if (tabs.length === 0) {
      addScratchpad();
    } else if (activeTabId === tabId) {
      const newIdx = Math.min(idx, tabs.length - 1);
      switchTab(tabs[newIdx].id);
    }
  }

  async function openFile() {
    const selected = await open({
      title: "Open Markdown File",
      filters: [{ name: "Markdown", extensions: ["md", "mdx", "markdown", "txt"] }],
    });
    if (!selected) return;
    const path = typeof selected === "string" ? selected : selected;
    // Check if already open
    const existing = tabs.find((t) => t.filePath === path);
    if (existing) {
      switchTab(existing.id);
      return;
    }
    const content = await readFile(path);
    const tab = createTab(path, content);
    tabs = [...tabs, tab];
    switchTab(tab.id);
  }

  async function saveCurrentTab() {
    const tab = tabs.find((t) => t.id === activeTabId);
    if (!tab) return;
    // Sync editor content
    if (editorView) {
      tab.content = editorView.state.doc.toString();
    }
    let path = tab.filePath;
    if (!path) {
      const selected = await save({
        title: "Save Markdown File",
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (!selected) return;
      path = selected;
      tab.filePath = path;
    }
    await writeFile(path, tab.content);
    tab.dirty = false;
    tabs = [...tabs]; // trigger reactivity
  }

  function buildExtensions(): Extension[] {
    const exts: Extension[] = [
      EditorView.lineWrapping,
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(defaultHighlightStyle),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          const tab = tabs.find((t) => t.id === activeTabId);
          if (tab && !tab.dirty) {
            tab.dirty = true;
            tabs = [...tabs];
          }
        }
      }),
      EditorView.theme({
        "&": {
          height: "100%",
          fontSize: `${$settings.fontSize}px`,
          fontFamily: $settings.fontFamily,
        },
        ".cm-content": {
          caretColor: "var(--color-text-primary)",
          color: "var(--color-text-primary)",
          padding: "1rem",
        },
        ".cm-gutters": {
          backgroundColor: "var(--color-bg-deep)",
          color: "var(--color-text-muted)",
          border: "none",
        },
        ".cm-activeLine": {
          backgroundColor: "var(--color-bg-surface)",
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
      }),
    ];
    if (vimEnabled) {
      exts.unshift(vim());
    }
    return exts;
  }

  function createEditorView(content: string) {
    if (!editorContainer) return;
    editorView?.destroy();
    editorView = new EditorView({
      state: EditorState.create({
        doc: content,
        extensions: buildExtensions(),
      }),
      parent: editorContainer,
    });
  }

  function toggleVim() {
    vimEnabled = !vimEnabled;
    // Recreate editor with new extensions
    const tab = tabs.find((t) => t.id === activeTabId);
    if (tab && editorView) {
      tab.content = editorView.state.doc.toString();
      createEditorView(tab.content);
    }
  }

  // Handle Cmd+S
  function handleKeydown(e: KeyboardEvent) {
    if (hasPrimaryModifier(e) && e.key === "s") {
      e.preventDefault();
      saveCurrentTab();
    }
  }

  onMount(async () => {
    if (docPath) {
      try {
        const content = await readFile(docPath);
        const tab = createTab(docPath, content);
        tabs = [tab];
        activeTabId = tab.id;
      } catch {
        const tab = createTab(null, "");
        tabs = [tab];
        activeTabId = tab.id;
      }
    } else {
      const tab = createTab(null, "");
      tabs = [tab];
      activeTabId = tab.id;
    }
    createEditorView(tabs[0].content);
  });

  onDestroy(() => {
    editorView?.destroy();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="relative flex h-full w-full flex-col bg-bg-deep">
  <!-- Tab bar -->
  <div class="flex h-9 shrink-0 items-center gap-0 border-b border-hairline bg-bg-surface/30 px-1">
    <div class="flex min-w-0 flex-1 items-center gap-0 overflow-x-auto">
      {#each tabs as tab (tab.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="group flex shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1 text-[11px] transition-colors
            {activeTabId === tab.id
              ? 'bg-bg-active text-text-primary'
              : 'text-text-muted hover:bg-bg-hover hover:text-text-secondary'}"
          onclick={() => switchTab(tab.id)}
        >
          <span class="max-w-[120px] truncate">
            {#if tab.dirty}<span class="text-accent">&#8226; </span>{/if}{tabName(tab)}
          </span>
          <CloseButton
            class="ml-0.5 p-0 opacity-0 transition-opacity hover:border-transparent group-hover:opacity-100"
            onclick={(e: MouseEvent) => { e.stopPropagation(); closeTab(tab.id); }}
            label="Close tab"
            title="Close tab"
            size={11}
            tabindex={-1}
          />
        </div>
      {/each}
    </div>
    <div class="flex shrink-0 items-center gap-1 px-1">
      <button
        class="cursor-pointer rounded-lg p-1 text-[11px] text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary"
        onclick={addScratchpad}
        title="New scratchpad"
      >+</button>
      <button
        class="cursor-pointer rounded-lg p-1 text-[11px] text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary"
        onclick={openFile}
        title="Open file"
      >&#128194;</button>
      <button
        class="cursor-pointer rounded-lg px-1.5 py-0.5 font-mono text-[10px] transition-colors
          {vimEnabled
            ? 'bg-accent-dim/20 text-accent'
            : 'text-text-muted hover:bg-bg-hover hover:text-text-primary'}"
        onclick={toggleVim}
        title={vimEnabled ? "Disable vim mode" : "Enable vim mode"}
      >vim</button>
    </div>
  </div>

  <!-- Editor -->
  <div class="flex-1 overflow-hidden" bind:this={editorContainer}></div>
</div>
