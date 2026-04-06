# Markdown Editor Pane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the read-only DocPane with a CodeMirror 6 markdown editor pane supporting file editing, scratchpad tabs, and optional vim keybindings.

**Architecture:** New `"markdown"` pane type replaces `"doc"`. A single `MarkdownPane.svelte` component manages a tab bar and a CodeMirror editor instance. A `write_file` Tauri command is added for saving. Vim mode uses `@replit/codemirror-vim`.

**Tech Stack:** Svelte 5, CodeMirror 6, @codemirror/lang-markdown, @replit/codemirror-vim, Tauri dialog plugin, Tauri invoke commands.

---

### File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/lib/components/MarkdownPane.svelte` | CodeMirror editor with tab bar, vim toggle, file ops |
| Modify | `src/lib/stores/panes.ts:7` | Change `"doc"` to `"markdown"` in Pane type union |
| Modify | `src/lib/components/SplitPane.svelte:2-6,53-59` | Replace DocPane import/rendering with MarkdownPane |
| Modify | `src/lib/commands/index.ts:136` | Change `type: "doc"` to `type: "markdown"` |
| Modify | `src/lib/tauri.ts:132-138` | Add `writeFile` function |
| Modify | `src-tauri/src/main.rs:372-374,459` | Add `write_file` Tauri command and register it |
| Modify | `src/lib/panes/__tests__/actions.test.ts:42` | Update test to use `type: "markdown"` |
| Modify | `src/lib/stores/__tests__/panes.test.ts:190` | Update test to use `type: "markdown"` |
| Delete | `src/lib/components/DocPane.svelte` | Replaced by MarkdownPane |

---

### Task 1: Install CodeMirror dependencies

**Files:**
- Modify: `package.json`

- [ ] **Step 1: Install packages**

```bash
npm install codemirror @codemirror/lang-markdown @codemirror/language-data @replit/codemirror-vim
```

- [ ] **Step 2: Verify installation**

```bash
npm ls codemirror @codemirror/lang-markdown @codemirror/language-data @replit/codemirror-vim
```

Expected: All four packages listed without errors.

- [ ] **Step 3: Commit**

```bash
git add package.json package-lock.json
git commit -m "chore: add codemirror dependencies for markdown editor pane"
```

---

### Task 2: Add `write_file` Tauri command

**Files:**
- Modify: `src-tauri/src/main.rs:372-374,459`
- Modify: `src/lib/tauri.ts:132-138`

- [ ] **Step 1: Add Rust `write_file` command**

In `src-tauri/src/main.rs`, add this function right after the existing `read_file` function (after line 374):

```rust
#[tauri::command]
fn write_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, &contents).map_err(|e| format!("Failed to write file: {}", e))
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/main.rs`, in the `invoke_handler` array (around line 459), add `write_file` after `read_file`:

```rust
            read_file,
            write_file,
```

- [ ] **Step 3: Add TypeScript wrapper**

In `src/lib/tauri.ts`, add after the `readFile` function (after line 134):

```typescript
export async function writeFile(path: string, contents: string): Promise<void> {
  return invoke("write_file", { path, contents });
}
```

- [ ] **Step 4: Verify the app compiles**

```bash
cd src-tauri && cargo check
```

Expected: No errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/main.rs src/lib/tauri.ts
git commit -m "feat: add write_file tauri command for saving editor content"
```

---

### Task 3: Update pane type from `"doc"` to `"markdown"`

**Files:**
- Modify: `src/lib/stores/panes.ts:7`
- Modify: `src/lib/commands/index.ts:136`
- Modify: `src/lib/panes/__tests__/actions.test.ts:42`
- Modify: `src/lib/stores/__tests__/panes.test.ts:190`

- [ ] **Step 1: Update the Pane type union**

In `src/lib/stores/panes.ts`, line 7, change:

```typescript
  type: "claude" | "shell" | "doc" | "command";
```

to:

```typescript
  type: "claude" | "shell" | "markdown" | "command";
```

- [ ] **Step 2: Update command palette**

In `src/lib/commands/index.ts`, line 136, change:

```typescript
              type: "doc",
```

to:

```typescript
              type: "markdown",
```

- [ ] **Step 3: Update actions test**

In `src/lib/panes/__tests__/actions.test.ts`, line 42, change:

```typescript
      type: "doc",
```

to:

```typescript
      type: "markdown",
```

Also update the test description on line 38 from `"removes document panes"` to `"removes markdown panes"` if applicable.

- [ ] **Step 4: Update panes store test**

In `src/lib/stores/__tests__/panes.test.ts`, line 190, change:

```typescript
      type: "doc",
```

to:

```typescript
      type: "markdown",
```

And line 199, change:

```typescript
        expect(docPane.pane.type).toBe("doc");
```

to:

```typescript
        expect(docPane.pane.type).toBe("markdown");
```

- [ ] **Step 5: Run tests**

```bash
npm run test
```

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib/stores/panes.ts src/lib/commands/index.ts src/lib/panes/__tests__/actions.test.ts src/lib/stores/__tests__/panes.test.ts
git commit -m "refactor: rename doc pane type to markdown"
```

---

### Task 4: Create MarkdownPane component

**Files:**
- Create: `src/lib/components/MarkdownPane.svelte`

- [ ] **Step 1: Create the component**

Create `src/lib/components/MarkdownPane.svelte`:

```svelte
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView, keymap, lineWrapping } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
  import { languages } from "@codemirror/language-data";
  import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
  import { vim } from "@replit/codemirror-vim";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { readFile, writeFile } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";

  interface Props {
    docPath?: string;
    onClose: () => void;
  }

  let { docPath = "", onClose }: Props = $props();

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
  let hovering = $state(false);

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
      lineWrapping,
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
    if ((e.metaKey || e.ctrlKey) && e.key === "s") {
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
<div
  class="relative flex h-full w-full flex-col bg-bg-deep"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
>
  <!-- Tab bar -->
  <div class="flex h-9 shrink-0 items-center gap-0 border-b border-hairline bg-bg-surface/30 px-1">
    <div class="flex min-w-0 flex-1 items-center gap-0 overflow-x-auto">
      {#each tabs as tab (tab.id)}
        <button
          class="group flex shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1 text-[11px] transition-colors
            {activeTabId === tab.id
              ? 'bg-bg-active text-text-primary'
              : 'text-text-muted hover:bg-bg-hover hover:text-text-secondary'}"
          onclick={() => switchTab(tab.id)}
        >
          <span class="max-w-[120px] truncate">
            {#if tab.dirty}<span class="text-accent">&#8226; </span>{/if}{tabName(tab)}
          </span>
          <span
            class="ml-0.5 rounded text-[10px] leading-none opacity-0 transition-opacity hover:text-text-primary group-hover:opacity-100"
            onclick|stopPropagation={() => closeTab(tab.id)}
            role="button"
            tabindex="-1"
          >&times;</span>
        </button>
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

  <!-- Close button on hover -->
  {#if hovering}
    <button
      class="absolute right-2 top-2 z-10 flex h-7 w-7 items-center justify-center rounded-full border border-border-subtle bg-bg-surface/85 text-xs leading-none text-text-muted backdrop-blur-sm hover:bg-bg-hover hover:text-text-primary"
      onclick={onClose}
      title="Close pane"
    >
      &times;
    </button>
  {/if}
</div>
```

- [ ] **Step 2: Verify no syntax errors**

```bash
npx svelte-check --fail-on-warnings 2>&1 | head -30
```

Expected: No errors in MarkdownPane.svelte (warnings from other files are OK).

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/MarkdownPane.svelte
git commit -m "feat: create MarkdownPane component with CodeMirror editor, tabs, and vim mode"
```

---

### Task 5: Wire MarkdownPane into SplitPane and remove DocPane

**Files:**
- Modify: `src/lib/components/SplitPane.svelte:2-6,53-59`
- Delete: `src/lib/components/DocPane.svelte`

- [ ] **Step 1: Update SplitPane imports**

In `src/lib/components/SplitPane.svelte`, replace line 6:

```typescript
  import DocPane from "./DocPane.svelte";
```

with:

```typescript
  import MarkdownPane from "./MarkdownPane.svelte";
```

- [ ] **Step 2: Update SplitPane rendering**

In `src/lib/components/SplitPane.svelte`, replace lines 53-59:

```svelte
      {:else if node.pane.type === "doc"}
        <DocPane
          docPath={node.pane.docPath ?? ""}
          onClose={async () => {
            await closePane(sessionId, node.pane.id);
          }}
        />
```

with:

```svelte
      {:else if node.pane.type === "markdown"}
        <MarkdownPane
          docPath={node.pane.docPath ?? ""}
          onClose={async () => {
            await closePane(sessionId, node.pane.id);
          }}
        />
```

- [ ] **Step 3: Delete DocPane.svelte**

```bash
rm src/lib/components/DocPane.svelte
```

- [ ] **Step 4: Check for remaining DocPane references**

```bash
grep -r "DocPane" src/ --include="*.svelte" --include="*.ts"
```

Expected: No results.

- [ ] **Step 5: Run tests**

```bash
npm run test
```

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/SplitPane.svelte src/lib/components/MarkdownPane.svelte
git rm src/lib/components/DocPane.svelte
git commit -m "feat: replace DocPane with MarkdownPane in split pane rendering"
```

---

### Task 6: Manual verification

- [ ] **Step 1: Build and launch**

```bash
npm run tauri dev
```

- [ ] **Step 2: Test markdown pane creation**

Open the command palette (Cmd+K), search for a doc — it should open in the new markdown editor pane.

- [ ] **Step 3: Test scratchpad**

Click the `+` button in the tab bar. Type some markdown. Verify syntax highlighting works.

- [ ] **Step 4: Test file operations**

Open a file via the folder icon. Edit it. Press Cmd+S to save. Reopen and verify content persisted.

- [ ] **Step 5: Test vim mode**

Click the `vim` button. Verify vim keybindings work (hjkl navigation, i to enter insert mode, Esc to exit, :w to save). Click again to disable.

- [ ] **Step 6: Test tab management**

Open multiple tabs. Switch between them. Close tabs. Verify dirty indicators show on edited tabs.
