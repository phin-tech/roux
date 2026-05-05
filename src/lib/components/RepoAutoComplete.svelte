<script lang="ts">
  import { Command } from "bits-ui";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    pickerShellClass,
    pickerInputRowClass,
    pickerInputClass,
    pickerListClass,
    pickerItemClass,
    pickerSideButtonClass,
    focusLeavingElement,
  } from "./pickerStyles";
  import { findQuickPickMatch, type RepoQuickPickOption } from "$lib/repos/quickPick";

  interface Props {
    /** Two-way bound input text. Caller decides whether this is the chosen
     *  path (NewSessionDialog) or a transient draft (NewProjectDialog). */
    value: string;
    options: RepoQuickPickOption[];
    /** DOM id for the input — lets a parent `<label for="...">` target it. */
    id?: string;
    placeholder?: string;
    /** Sets `disabled` on the Refresh button while options are reloading. */
    loading?: boolean;
    /** Suppresses the dropdown when the user has no repo roots configured.
     *  Same gate NewSessionDialog uses. */
    hasConfiguredRoots?: boolean;
    showRefresh?: boolean;
    showBrowse?: boolean;
    refreshLabel?: string;
    browseLabel?: string;
    emptyText?: string;
    /** Fired when the user picks an item from the dropdown OR Enter matches one. */
    onselect: (path: string, label: string) => void;
    /** Fired on Enter when the typed input has no quick-pick match. Caller
     *  decides whether to commit the raw text. */
    onenter?: (text: string) => void;
    onrefresh?: () => void;
    /** Override the default Tauri directory picker. */
    onbrowse?: () => void | Promise<void>;
  }

  let {
    value = $bindable(""),
    options,
    id,
    placeholder = "Type path or search configured repo roots",
    loading = false,
    hasConfiguredRoots = true,
    showRefresh = false,
    showBrowse = false,
    refreshLabel = "Refresh",
    browseLabel = "Browse",
    emptyText = "No matching repositories",
    onselect,
    onenter,
    onrefresh,
    onbrowse,
  }: Props = $props();

  let pickerOpen = $state(true);
  let closeT: ReturnType<typeof setTimeout> | null = null;
  // One-shot guard: swallows the next focus event after a selection so a
  // programmatic refocus by the caller doesn't immediately re-pop the dropdown.
  let suppressNextFocusOpen = $state(false);

  function cancelDeferredClose() {
    if (closeT != null) {
      clearTimeout(closeT);
      closeT = null;
    }
  }
  function armDeferredClose() {
    cancelDeferredClose();
    closeT = setTimeout(() => {
      closeT = null;
      pickerOpen = false;
    }, 150);
  }

  function handleSelect(path: string, label: string) {
    onselect(path, label);
    pickerOpen = false;
    suppressNextFocusOpen = true;
  }

  function handleEnter() {
    const match = findQuickPickMatch(value, options);
    if (match) {
      handleSelect(match.path, match.label);
      return;
    }
    pickerOpen = false;
    suppressNextFocusOpen = true;
    onenter?.(value);
  }

  async function defaultBrowse() {
    if (onbrowse) {
      await onbrowse();
      return;
    }
    const selected = await open({ directory: true, title: "Select Directory" });
    if (typeof selected === "string") {
      value = selected;
      onenter?.(selected);
    }
  }
</script>

<div
  class={pickerShellClass}
  onfocusin={cancelDeferredClose}
  onfocusout={(e) => {
    const shell = e.currentTarget as HTMLElement;
    if (!focusLeavingElement(shell, e.relatedTarget)) return;
    armDeferredClose();
  }}
>
  <Command.Root shouldFilter={true} loop={true} vimBindings={true}>
    <div class={pickerInputRowClass}>
      <Command.Input
        {id}
        bind:value
        {placeholder}
        class={pickerInputClass}
        onfocus={() => {
          if (suppressNextFocusOpen) {
            suppressNextFocusOpen = false;
            return;
          }
          pickerOpen = true;
        }}
        oninput={() => {
          suppressNextFocusOpen = false;
          pickerOpen = true;
        }}
        onkeydown={(e) => {
          if (e.key !== "Enter") return;
          e.preventDefault();
          handleEnter();
        }}
      />
      {#if showRefresh}
        <button
          class={pickerSideButtonClass}
          onclick={onrefresh}
          disabled={loading}
        >
          {loading ? "..." : refreshLabel}
        </button>
      {/if}
      {#if showBrowse}
        <button
          class={pickerSideButtonClass}
          onclick={defaultBrowse}
        >
          {browseLabel}
        </button>
      {/if}
    </div>
    {#if pickerOpen && hasConfiguredRoots}
      <Command.List class={`${pickerListClass} max-h-36`}>
        <Command.Empty class="px-3 py-2 text-[11px] text-text-muted">
          {emptyText}
        </Command.Empty>
        <Command.Group>
          <Command.GroupItems>
            {#each options as opt (opt.path)}
              <Command.Item
                value={opt.label}
                keywords={[opt.path]}
                onSelect={() => handleSelect(opt.path, opt.label)}
                class={`${pickerItemClass} justify-between py-1.5 data-[selected]:bg-bg-active`}
              >
                <span class="truncate text-[12px] text-text-primary">{opt.label}</span>
                <span class="ml-2 max-w-40 truncate font-mono text-[10px] text-text-muted">{opt.path}</span>
              </Command.Item>
            {/each}
          </Command.GroupItems>
        </Command.Group>
      </Command.List>
    {/if}
  </Command.Root>
</div>
