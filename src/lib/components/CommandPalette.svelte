<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { Command } from "bits-ui";
  import { registry, type Command as Cmd, type CommandItem as CmdItem } from "$lib/commands/registry";
  import { formatShortcut } from "$lib/platform";
  import { shortcutFor, keymapState } from "$lib/keymap/store";

  // Resolve the shortcut label for a command from the current keymap. The
  // `_state` parameter is passed in the template so `$keymapState` drives
  // reactivity when the keymap reloads.
  function resolveShortcut(cmd: Cmd, _state: unknown): string | null {
    return shortcutFor(cmd.id);
  }

  interface Props {
    open: boolean;
    onclose: () => void;
    onNewSession: () => void;
    onSettings: () => void;
    onCheckForUpdates: () => void;
  }

  let {
    open,
    onclose,
    onNewSession,
    onSettings,
    onCheckForUpdates,
  }: Props = $props();

  let stepStack = $state<{ label: string; items: CmdItem[]; sourceCmd?: Cmd }[]>([]);
  let inputValue = $state("");

  let inDrillStep = $derived(stepStack.length > 0);
  let currentStep = $derived(inDrillStep ? stepStack[stepStack.length - 1] : null);

  let availableCommands = $derived.by(() => {
    if (inDrillStep) return [];
    const cmds = registry.getAvailable();
    const groups = new Map<string, Cmd[]>();
    for (const cmd of cmds) {
      if (!groups.has(cmd.category)) groups.set(cmd.category, []);
      groups.get(cmd.category)!.push(cmd);
    }
    return [...groups.entries()];
  });

  let dialogEl: HTMLDivElement | undefined = $state();

  $effect(() => {
    if (open) {
      inputValue = "";
      stepStack = [];
      // Focus the input after render — find it in the DOM
      requestAnimationFrame(() => {
        const input = dialogEl?.querySelector("input");
        input?.focus();
      });
    }
  });

  async function handleCommandSelect(cmd: Cmd) {
    if (cmd.getItems) {
      const items = await cmd.getItems();
      stepStack = [...stepStack, { label: cmd.label, items, sourceCmd: cmd }];
      inputValue = "";
      return;
    }

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
    if (cmd.id === "app.check-updates") {
      onclose();
      onCheckForUpdates();
      return;
    }
    if (cmd.id === "app.command-palette") return;

    if (cmd.execute) {
      stepStack = [];
      inputValue = "";
      onclose();
      await new Promise(r => setTimeout(r, 50));
      await cmd.execute();
    }
  }

  async function handleItemSelect(item: CmdItem) {
    if (item.drillCommand) {
      const cmd = registry.get(item.drillCommand);
      if (cmd) {
        await handleCommandSelect(cmd);
        return;
      }
    }

    if (item.substeps) {
      const subItems = await item.substeps();
      stepStack = [...stepStack, { label: item.label, items: subItems }];
      inputValue = "";
      return;
    }

    if (item.action) {
      // Close first, then execute
      stepStack = [];
      inputValue = "";
      onclose();
      // Small delay to ensure UI closes before action runs
      await new Promise(r => setTimeout(r, 50));
      await item.action();
    }
  }

  function goBack() {
    if (stepStack.length > 0) {
      stepStack = stepStack.slice(0, -1);
      inputValue = "";
    }
  }

  async function handleInputSubmit() {
    if (!inDrillStep || !inputValue.trim()) return;
    const cmd = currentStep?.sourceCmd;
    if (!cmd?.onInput) return;
    const text = inputValue.trim();
    stepStack = [];
    inputValue = "";
    onclose();
    await new Promise(r => setTimeout(r, 50));
    await cmd.onInput(text);
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (inDrillStep) {
        goBack();
      } else {
        stepStack = [];
        inputValue = "";
        onclose();
      }
      return;
    }
    if (e.key === "Enter" && inDrillStep && inputValue.trim() && currentStep?.sourceCmd?.onInput) {
      e.preventDefault();
      e.stopPropagation();
      void handleInputSubmit();
      return;
    }
    if (e.key === "Backspace" && inputValue === "" && inDrillStep) {
      e.preventDefault();
      goBack();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-start justify-center bg-black/65 pt-[18vh] backdrop-blur-md"
    onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
    transition:fade={{ duration: 120 }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      bind:this={dialogEl}
      class="ui-dialog flex max-h-[440px] w-[560px] flex-col overflow-hidden rounded-[1.4rem]"
      onkeydown={handleKeyDown}
      transition:scale={{ duration: 120, start: 0.985 }}
    >
      {#if inDrillStep}
        <div class="flex items-center gap-1.5 px-5 pt-4 pb-0 text-[11px]">
          <button
            class="text-text-muted hover:text-text-primary bg-transparent border-none cursor-pointer p-0 font-sans"
            onclick={goBack}
          >Commands</button>
          {#each stepStack as step, i}
            <span class="text-text-muted">/</span>
            {#if i < stepStack.length - 1}
              <button
                class="text-text-muted hover:text-text-primary bg-transparent border-none cursor-pointer p-0 font-sans"
                onclick={() => { stepStack = stepStack.slice(0, i + 1); inputValue = ""; }}
              >{step.label}</button>
            {:else}
              <span class="text-text-primary">{step.label}</span>
            {/if}
          {/each}
        </div>
      {/if}

      <Command.Root
        shouldFilter={!inDrillStep}
        loop={true}
        vimBindings={true}
      >
        <div
          class="flex items-center gap-2 border-b border-border-subtle bg-bg-surface/55 px-5 py-4"
        >
          {#if inDrillStep}
            <button
              class="text-text-muted hover:text-text-primary bg-transparent border-none cursor-pointer p-0 text-sm"
              onclick={goBack}
              title="Back"
            >&#8592;</button>
          {/if}
          <Command.Input
            bind:value={inputValue}
            onkeydown={handleKeyDown}
            placeholder={inDrillStep ? (currentStep?.sourceCmd?.inputPlaceholder ?? `Search ${currentStep?.label}...`) : "Type a command..."}
            class="flex-1 border-none bg-transparent text-sm text-text-primary outline-none placeholder:text-text-muted"
          />
        </div>

        <Command.List
          class="app-scrollbar max-h-[340px] flex-1 overflow-y-auto px-3 py-3"
        >
          <Command.Empty class="px-4 py-8 text-center text-sm text-text-muted">
            {#if inDrillStep && currentStep?.sourceCmd?.onInput}
              Type and press Enter to submit
            {:else}
              No results found
            {/if}
          </Command.Empty>

          {#if inDrillStep && currentStep}
            {#each currentStep.items as item (item.id)}
              {@const matches = !inputValue || item.label.toLowerCase().includes(inputValue.toLowerCase()) || (item.description ?? "").toLowerCase().includes(inputValue.toLowerCase())}
              {#if matches}
              <Command.Item
                value={item.label}
                keywords={item.description ? [item.description] : []}
                onSelect={() => handleItemSelect(item)}
                class="cmd-item"
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
                </Command.Item>
              {/if}
            {/each}
          {:else}
            {#each availableCommands as [category, commands] (category)}
              <Command.Group>
                <Command.GroupHeading class="px-3 py-2 text-[10px] font-medium uppercase tracking-[0.22em] text-text-muted">
                  {category}
                </Command.GroupHeading>
                <Command.GroupItems>
                  {#each commands as cmd (cmd.id)}
                    <Command.Item
                      value={cmd.label}
                      onSelect={() => handleCommandSelect(cmd)}
                      class="cmd-item"
                    >
                      <div class="flex-1 min-w-0">
                        <span class="text-text-primary">{cmd.label}</span>
                      </div>
                      {#if cmd.getItems}
                        <span class="text-text-muted text-xs font-mono shrink-0">&#8594;</span>
                      {/if}
                      {@const shortcut = resolveShortcut(cmd, $keymapState)}
                      {#if shortcut}
                        <kbd class="text-[11px] font-mono text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded border border-border-subtle shrink-0">
                          {formatShortcut(shortcut)}
                        </kbd>
                      {/if}
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            {/each}
          {/if}
        </Command.List>
      </Command.Root>
    </div>
  </div>
{/if}

<style>
  :global(.cmd-item) {
    padding: 12px 14px;
    border-radius: 14px;
    border: 1px solid transparent;
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    font-size: 14px;
    transition: background 0.1s, border-color 0.1s, transform 0.1s;
  }
  :global(.cmd-item:hover) {
    background: color-mix(in srgb, var(--color-bg-hover) 82%, transparent);
    border-color: var(--color-border-subtle);
  }
  :global(.cmd-item[data-selected]) {
    background: var(--color-bg-active);
    border-color: var(--color-border);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
    transform: translateY(-1px);
  }
</style>
