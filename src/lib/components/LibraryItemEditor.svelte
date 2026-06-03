<script lang="ts">
  import { onDestroy } from "svelte";
  import { EditorState, type Extension } from "@codemirror/state";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
  import { languages } from "@codemirror/language-data";
  import {
    defaultHighlightStyle,
    syntaxHighlighting,
  } from "@codemirror/language";
  import { EditorView, keymap } from "@codemirror/view";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import {
    type LibraryItemType,
    type LibraryRead,
    type LibrarySource,
    type LibraryVariable,
    type LibraryVariableType,
    type SaveLibraryItemRequest,
    type SaveLibraryTarget,
  } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";

  interface Props {
    item: LibraryRead | null;
    itemType: LibraryItemType;
    sources: LibrarySource[];
    activeRepo: string | null;
    onsave: (request: SaveLibraryItemRequest) => void | Promise<void>;
    oncancel: () => void;
    ondirtychange?: (dirty: boolean) => void;
  }

  let {
    item,
    itemType,
    sources,
    activeRepo,
    onsave,
    oncancel,
    ondirtychange,
  }: Props = $props();

  let editorContainer: HTMLElement | undefined = $state();
  let editorView: EditorView | null = null;
  let originalPath = $state<string | null>(null);
  let originalId = $state<string | null>(null);
  let title = $state("");
  let itemId = $state("");
  let description = $state("");
  let tags = $state("");
  let provider = $state("");
  let variables = $state<LibraryVariable[]>([]);
  let targetValue = $state("global");
  let body = $state("");
  let initialSnapshot = $state("");
  let dirty = $state(false);
  let saving = $state(false);
  let editorFontSize = $state<number | null>(null);
  let editorFontFamily = $state<string | null>(null);
  let initializedKey = $state<string | null>(null);

  function initFromItem(next: LibraryRead | null, nextTargetValue: string) {
    const nextBody = next?.body ?? "";
    const nextOriginalPath = next?.item.sourcePath ?? null;
    const nextOriginalId = next?.item.id ?? null;
    const nextTitle = next?.item.title ?? "";
    const nextItemId = next?.item.id ?? "";
    const nextDescription = next?.item.description ?? "";
    const nextTags = next?.item.tags.join(", ") ?? "";
    const nextProvider = next?.item.provider ?? "";
    const nextVariables =
      next?.item.variables.map((variable) => ({ ...variable })) ?? [];
    originalPath = nextOriginalPath;
    originalId = nextOriginalId;
    title = nextTitle;
    itemId = nextItemId;
    description = nextDescription;
    tags = nextTags;
    provider = nextProvider;
    variables = nextVariables;
    body = nextBody;
    targetValue = nextTargetValue;
    setEditorDoc(nextBody);
    initialSnapshot = snapshotFromValues({
      title: nextTitle,
      itemId: nextItemId,
      description: nextDescription,
      tags: nextTags,
      provider: nextProvider,
      variables: nextVariables,
      targetValue: nextTargetValue,
      body: nextBody,
    });
    dirty = false;
    ondirtychange?.(false);
  }

  function defaultTargetValue(): string {
    return activeRepo ? "activeRepo" : "global";
  }

  function targetValueForItem(next: LibraryRead): string {
    if (next.item.sourceLayer === "activeRepo") return "activeRepo";
    if (next.item.sourceId) return `source:${next.item.sourceId}`;
    return "global";
  }

  function buildExtensions(): Extension[] {
    return [
      EditorView.lineWrapping,
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(defaultHighlightStyle),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) body = update.state.doc.toString();
      }),
      EditorView.theme({
        "&": {
          height: "100%",
          fontSize: `${$settings.fontSize}px`,
          fontFamily: $settings.fontFamily,
          backgroundColor: "var(--color-bg-deep)",
          color: "var(--color-text-primary)",
        },
        ".cm-content": {
          caretColor: "var(--color-text-primary)",
          color: "var(--color-text-primary)",
          padding: "0.75rem",
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
  }

  function createEditor() {
    if (!editorContainer || editorView) return;
    editorFontSize = $settings.fontSize;
    editorFontFamily = $settings.fontFamily;
    editorView = new EditorView({
      state: EditorState.create({ doc: body, extensions: buildExtensions() }),
      parent: editorContainer,
    });
  }

  function setEditorDoc(next: string) {
    body = next;
    if (!editorView) return;
    if (editorView.state.doc.toString() === next) return;
    editorView.dispatch({
      changes: { from: 0, to: editorView.state.doc.length, insert: next },
    });
  }

  function parseTarget(): SaveLibraryTarget {
    if (targetValue === "activeRepo") return { type: "activeRepo" };
    if (targetValue.startsWith("source:"))
      return { type: "source", id: targetValue.slice("source:".length) };
    return { type: "global" };
  }

  function addVariable() {
    variables = [
      ...variables,
      {
        name: "",
        label: null,
        default: null,
        required: true,
        valueType: "string",
        options: [],
      },
    ];
  }

  function updateVariable(index: number, patch: Partial<LibraryVariable>) {
    variables = variables.map((variable, i) =>
      i === index ? { ...variable, ...patch } : variable,
    );
  }

  function updateVariableOptions(index: number, value: string) {
    updateVariable(index, {
      options: value
        .split(",")
        .map((option) => option.trim())
        .filter(Boolean),
    });
  }

  function removeVariable(index: number) {
    variables = variables.filter((_, i) => i !== index);
  }

  function currentSnapshot(): string {
    return snapshotFromValues({
      title,
      itemId,
      description,
      tags,
      provider,
      variables,
      targetValue,
      body,
    });
  }

  function snapshotFromValues(values: {
    title: string;
    itemId: string;
    description: string;
    tags: string;
    provider: string;
    variables: LibraryVariable[];
    targetValue: string;
    body: string;
  }): string {
    return JSON.stringify(values);
  }

  async function save() {
    if (saving) return;
    saving = true;
    try {
      body = editorView?.state.doc.toString() ?? body;
      await onsave({
        originalId,
        itemId,
        itemType,
        title,
        description,
        tags: tags
          .split(",")
          .map((tag) => tag.trim())
          .filter(Boolean),
        provider,
        variables,
        body,
        target: parseTarget(),
        expectedSourcePath: originalPath,
      });
    } finally {
      saving = false;
    }
  }

  $effect(() => {
    void editorContainer;
    createEditor();
  });

  $effect(() => {
    const nextKey = item
      ? `item:${item.item.id}:${item.item.sourcePath}`
      : `new:${itemType}`;
    if (nextKey === initializedKey) return;
    initializedKey = nextKey;
    initFromItem(item, item ? targetValueForItem(item) : defaultTargetValue());
  });

  $effect(() => {
    const nextDefault = defaultTargetValue();
    if (item !== null || dirty || targetValue === nextDefault) return;
    targetValue = nextDefault;
    initialSnapshot = currentSnapshot();
  });

  $effect(() => {
    const nextDirty =
      initialSnapshot !== "" && currentSnapshot() !== initialSnapshot;
    if (nextDirty !== dirty) {
      dirty = nextDirty;
      ondirtychange?.(nextDirty);
    }
  });

  $effect(() => {
    const fontSize = $settings.fontSize;
    const fontFamily = $settings.fontFamily;
    if (!editorView || !editorContainer) return;
    if (editorFontSize === fontSize && editorFontFamily === fontFamily) return;
    const currentBody = editorView.state.doc.toString();
    editorView.destroy();
    editorView = null;
    body = currentBody;
    createEditor();
  });

  onDestroy(() => {
    editorView?.destroy();
  });
</script>

<div
  class="flex min-h-[620px] flex-col rounded-xl border border-border-subtle bg-bg-surface/20"
>
  <div
    class="flex items-center justify-between gap-3 border-b border-hairline px-3 py-2.5"
  >
    <div class="min-w-0">
      <div class="flex items-center gap-2">
        <div class="text-sm font-semibold text-text-primary">
          {originalId ? "Edit" : "New"}
          {itemType}
        </div>
        {#if dirty}
          <span
            class="rounded border border-yellow/30 bg-yellow/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-yellow"
            >Unsaved</span
          >
        {/if}
      </div>
      <div class="mt-0.5 truncate text-[11px] text-text-muted">
        Structured metadata, markdown body, plain files on disk
      </div>
    </div>
    <div class="flex gap-1">
      <button
        type="button"
        class="rounded-lg border border-border-subtle bg-bg-surface px-2 py-1 text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary"
        onclick={oncancel}>Cancel</button
      >
      <button
        type="button"
        class="rounded-lg border border-accent-dim/40 bg-accent-dim/15 px-3 py-1 text-xs font-semibold text-accent hover:bg-accent-dim/25 disabled:opacity-50"
        onclick={save}
        disabled={saving}
      >
        {saving ? "Saving" : "Save"}
      </button>
    </div>
  </div>

  <div class="grid gap-2 border-b border-hairline p-3">
    <div class="grid gap-2 lg:grid-cols-2">
      <label class="block">
        <span class="mb-1 block text-[11px] text-text-secondary">Title</span>
        <input
          class="w-full border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
          bind:value={title}
        />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] text-text-secondary">ID</span>
        <input
          class="w-full border border-border-subtle bg-bg-deep px-2 py-1.5 font-mono text-xs text-text-primary outline-none focus:border-border"
          bind:value={itemId}
        />
      </label>
    </div>

    <label class="block">
      <span class="mb-1 block text-[11px] text-text-secondary">Description</span
      >
      <input
        class="w-full border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
        bind:value={description}
      />
    </label>

    <div class="grid gap-2 lg:grid-cols-2">
      <label class="block">
        <span class="mb-1 block text-[11px] text-text-secondary">Tags</span>
        <input
          class="w-full border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
          placeholder="review, git"
          bind:value={tags}
        />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] text-text-secondary">Source</span>
        <select
          class="w-full border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
          bind:value={targetValue}
        >
          <option value="global">Global Library</option>
          {#if activeRepo}
            <option value="activeRepo">Active Repo Library</option>
          {/if}
          {#each sources as source (source.id)}
            <option value={`source:${source.id}`}
              >{source.name || source.path || source.url}</option
            >
          {/each}
        </select>
      </label>
    </div>

    <label class="block">
      <span class="mb-1 block text-[11px] text-text-secondary">Provider</span>
      <input
        class="w-full border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
        bind:value={provider}
      />
    </label>
  </div>

  {#if itemType === "prompt"}
    <div class="border-b border-hairline p-3">
      <div class="mb-2 flex items-center justify-between">
        <div
          class="text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary"
        >
          Variables
        </div>
        <button
          type="button"
          class="flex items-center gap-1 text-xs text-text-secondary hover:text-text-primary"
          onclick={addVariable}
        >
          <Plus size={13} /> Add
        </button>
      </div>
      <div class="space-y-1">
        {#each variables as variable, index}
          <div
            class="grid gap-1 lg:grid-cols-[1fr_1fr_0.7fr_1fr_1fr_auto_auto]"
          >
            <input
              class="min-w-0 border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
              placeholder="name"
              value={variable.name}
              oninput={(e) =>
                updateVariable(index, { name: e.currentTarget.value })}
            />
            <input
              class="min-w-0 border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
              placeholder="label"
              value={variable.label ?? ""}
              oninput={(e) =>
                updateVariable(index, { label: e.currentTarget.value })}
            />
            <select
              class="min-w-0 border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
              value={variable.valueType ?? "string"}
              onchange={(e) =>
                updateVariable(index, {
                  valueType: e.currentTarget.value as LibraryVariableType,
                })}
            >
              <option value="string">string</option>
              <option value="int">int</option>
              <option value="float">float</option>
              <option value="select">pick list</option>
            </select>
            <input
              class="min-w-0 border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border"
              placeholder="default"
              value={variable.default ?? ""}
              oninput={(e) =>
                updateVariable(index, { default: e.currentTarget.value })}
            />
            <input
              class="min-w-0 border border-border-subtle bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-border disabled:opacity-40"
              placeholder="options"
              value={(variable.options ?? []).join(", ")}
              disabled={(variable.valueType ?? "string") !== "select"}
              oninput={(e) =>
                updateVariableOptions(index, e.currentTarget.value)}
            />
            <label
              class="flex items-center gap-1 px-1 text-[11px] text-text-secondary"
            >
              <input
                type="checkbox"
                checked={variable.required}
                onchange={(e) =>
                  updateVariable(index, { required: e.currentTarget.checked })}
              />
              Required
            </label>
            <button
              type="button"
              class="px-1 text-text-muted hover:text-red"
              title="Remove variable"
              onclick={() => removeVariable(index)}
            >
              <Trash2 size={14} />
            </button>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <div
    class="border-t border-hairline bg-bg-surface/35 px-3 py-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary"
  >
    Body
  </div>
  <div
    class="min-h-[320px] flex-1 border-t border-hairline"
    bind:this={editorContainer}
  ></div>
</div>
