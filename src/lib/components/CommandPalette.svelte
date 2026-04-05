<script lang="ts">
  import { CommandRoot, CommandInput, CommandList, CommandItem, CommandGroup, CommandEmpty } from "cmdk-sv";
  import { registry, type Command, type CommandItem as CmdItem } from "$lib/commands/registry";
  import { tick } from "svelte";

  interface Props {
    open: boolean;
    onclose: () => void;
    onNewSession: () => void;
    onSettings: () => void;
  }

  let { open, onclose, onNewSession, onSettings }: Props = $props();

  // Step stack for multi-step drill-in
  let stepStack = $state<{ label: string; items: CmdItem[] }[]>([]);
  let inputEl = $state<HTMLInputElement | undefined>(undefined);
  let search = $state("");

  // Are we in a drill-in step?
  let inDrillStep = $derived(stepStack.length > 0);
  let currentStep = $derived(inDrillStep ? stepStack[stepStack.length - 1] : null);

  // Group commands by category for root view
  let availableCommands = $derived.by(() => {
    if (inDrillStep) return [];
    const cmds = registry.getAvailable();
    const groups = new Map<string, Command[]>();
    for (const cmd of cmds) {
      if (!groups.has(cmd.category)) groups.set(cmd.category, []);
      groups.get(cmd.category)!.push(cmd);
    }
    return [...groups.entries()];
  });

  // Focus input when opened
  $effect(() => {
    if (open) {
      search = "";
      stepStack = [];
      tick().then(() => inputEl?.focus());
    }
  });

  function formatShortcut(shortcut: string): string {
    return shortcut
      .split("+")
      .map((part) => {
        switch (part) {
          case "cmd": return "\u2318";
          case "shift": return "\u21e7";
          case "alt": return "\u2325";
          case "ctrl": return "\u2303";
          default: return part.toUpperCase();
        }
      })
      .join("");
  }

  async function handleCommandSelect(cmd: Command) {
    if (cmd.getItems) {
      const items = await cmd.getItems();
      stepStack = [...stepStack, { label: cmd.label, items }];
      search = "";
      await tick();
      inputEl?.focus();
      return;
    }

    // Handle special commands that need external callbacks
    if (cmd.id === "session.new") {
      onclose();
      onNewSession();
      return;
    }
    if (cmd.id === "app.settings") {
      onclose();
      onSettings();
      return;
    }
    if (cmd.id === "app.command-palette") {
      // Already open — just ignore
      return;
    }

    if (cmd.execute) {
      onclose();
      await cmd.execute();
    }
  }

  async function handleItemSelect(item: CmdItem) {
    if (item.substeps) {
      const subItems = await item.substeps();
      stepStack = [...stepStack, { label: item.label, items: subItems }];
      search = "";
      await tick();
      inputEl?.focus();
      return;
    }

    if (item.action) {
      onclose();
      await item.action();
    }
  }

  function goBack() {
    if (stepStack.length > 0) {
      stepStack = stepStack.slice(0, -1);
      search = "";
      tick().then(() => inputEl?.focus());
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (inDrillStep) {
        e.preventDefault();
        e.stopPropagation();
        goBack();
      } else {
        onclose();
      }
      return;
    }
    if (e.key === "Backspace" && search === "" && inDrillStep) {
      e.preventDefault();
      goBack();
    }
  }

  function handleOverlayClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onclose();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-start justify-center pt-[20vh]"
    onclick={handleOverlayClick}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-[540px] max-h-[420px] bg-bg-surface border border-border rounded-xl shadow-2xl flex flex-col overflow-hidden"
      onkeydown={handleKeyDown}
    >
      <!-- Breadcrumbs -->
      {#if inDrillStep}
        <div class="px-4 pt-3 pb-0 flex items-center gap-1.5 text-[11px]">
          <button
            class="text-text-muted hover:text-text-primary bg-transparent border-none cursor-pointer p-0 font-sans"
            onclick={goBack}
          >Commands</button>
          {#each stepStack as step, i}
            <span class="text-text-muted">/</span>
            {#if i < stepStack.length - 1}
              <button
                class="text-text-muted hover:text-text-primary bg-transparent border-none cursor-pointer p-0 font-sans"
                onclick={() => { stepStack = stepStack.slice(0, i + 1); search = ""; tick().then(() => inputEl?.focus()); }}
              >{step.label}</button>
            {:else}
              <span class="text-text-primary">{step.label}</span>
            {/if}
          {/each}
        </div>
      {/if}

      <CommandRoot
        label="Command Palette"
        shouldFilter={!inDrillStep}
      >
        <div class="px-4 py-3 border-b border-border-subtle flex items-center gap-2">
          {#if inDrillStep}
            <button
              class="text-text-muted hover:text-text-primary bg-transparent border-none cursor-pointer p-0 text-sm"
              onclick={goBack}
              title="Back"
            >&#8592;</button>
          {/if}
          <CommandInput
            bind:el={inputEl}
            bind:value={search}
            placeholder={inDrillStep ? `Search ${currentStep?.label}...` : "Type a command..."}
            class="flex-1 bg-transparent border-none outline-none text-sm text-text-primary placeholder-text-muted font-sans"
          />
        </div>

        <CommandList class="flex-1 overflow-y-auto px-2 py-2 scrollbar-thin max-h-[320px]">
          <CommandEmpty class="px-4 py-8 text-center text-text-muted text-sm">
            No results found
          </CommandEmpty>

          {#if inDrillStep && currentStep}
            <!-- Drill-in items -->
            {#each currentStep.items as item (item.id)}
              {@const matchesSearch = !search || item.label.toLowerCase().includes(search.toLowerCase()) || (item.description ?? "").toLowerCase().includes(search.toLowerCase())}
              {#if matchesSearch}
                <CommandItem
                  value={item.label}
                  onSelect={() => handleItemSelect(item)}
                  class="px-3 py-2 rounded-lg flex items-center gap-3 cursor-pointer text-sm transition-colors data-[selected]:bg-bg-active hover:bg-bg-hover group"
                >
                  <div class="flex-1 min-w-0">
                    <div class="text-text-primary text-sm">{item.label}</div>
                    {#if item.description}
                      <div class="text-text-muted text-xs truncate mt-0.5">{item.description}</div>
                    {/if}
                  </div>
                  {#if item.substeps}
                    <span class="text-text-muted text-xs font-mono shrink-0">&#8594;</span>
                  {/if}
                </CommandItem>
              {/if}
            {/each}
          {:else}
            <!-- Root: commands grouped by category -->
            {#each availableCommands as [category, commands] (category)}
              <CommandGroup heading={category} class="mb-2">
                <div class="px-2 py-1.5 text-[10px] uppercase tracking-wider text-text-muted font-semibold">{category}</div>
                {#each commands as cmd (cmd.id)}
                  <CommandItem
                    value={cmd.label}
                    onSelect={() => handleCommandSelect(cmd)}
                    class="px-3 py-2 rounded-lg flex items-center gap-3 cursor-pointer text-sm transition-colors data-[selected]:bg-bg-active hover:bg-bg-hover group"
                  >
                    <div class="flex-1 min-w-0">
                      <span class="text-text-primary">{cmd.label}</span>
                    </div>
                    {#if cmd.getItems}
                      <span class="text-text-muted text-xs font-mono shrink-0">&#8594;</span>
                    {/if}
                    {#if cmd.shortcut}
                      <kbd class="text-[11px] font-mono text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded border border-border-subtle shrink-0">
                        {formatShortcut(cmd.shortcut)}
                      </kbd>
                    {/if}
                  </CommandItem>
                {/each}
              </CommandGroup>
            {/each}
          {/if}
        </CommandList>
      </CommandRoot>
    </div>
  </div>
{/if}

<style>
  :global([data-cmdk-root]) {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  :global([data-cmdk-list]) {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  :global([data-cmdk-input]) {
    width: 100%;
  }
  :global([data-cmdk-item]) {
    padding: 8px 12px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    font-size: 14px;
    transition: background 0.1s;
  }
  :global([data-cmdk-item]:hover) {
    background: var(--color-bg-hover);
  }
  :global([data-cmdk-item][data-selected="true"]) {
    background: var(--color-bg-active) !important;
    outline: 1px solid var(--color-border);
  }
  :global([data-cmdk-group-heading]) {
    display: none; /* We render our own headings */
  }
</style>
