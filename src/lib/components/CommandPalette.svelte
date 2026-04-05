<script lang="ts">
  import { Command } from "bits-ui";
  import { registry, type Command as Cmd, type CommandItem as CmdItem } from "$lib/commands/registry";
  import { tick } from "svelte";

  interface Props {
    open: boolean;
    onclose: () => void;
    onNewSession: () => void;
    onSettings: () => void;
  }

  let { open, onclose, onNewSession, onSettings }: Props = $props();

  let stepStack = $state<{ label: string; items: CmdItem[] }[]>([]);
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

  async function handleCommandSelect(cmd: Cmd) {
    if (cmd.getItems) {
      const items = await cmd.getItems();
      stepStack = [...stepStack, { label: cmd.label, items }];
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
    class="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-start justify-center pt-[20vh]"
    onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      bind:this={dialogEl}
      class="w-[540px] max-h-[420px] bg-bg-surface border border-border rounded-xl shadow-2xl flex flex-col overflow-hidden"
      onkeydown={handleKeyDown}
    >
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
        <div class="px-4 py-3 border-b border-border-subtle flex items-center gap-2">
          {#if inDrillStep}
            <button
              class="text-text-muted hover:text-text-primary bg-transparent border-none cursor-pointer p-0 text-sm"
              onclick={goBack}
              title="Back"
            >&#8592;</button>
          {/if}
          <Command.Input
            bind:value={inputValue}
            placeholder={inDrillStep ? `Search ${currentStep?.label}...` : "Type a command..."}
            class="flex-1 bg-transparent border-none outline-none text-sm text-text-primary placeholder-text-muted font-sans"
          />
        </div>

        <Command.List
          class="flex-1 overflow-y-auto px-2 py-2 max-h-[320px]"
        >
          <Command.Empty class="px-4 py-8 text-center text-text-muted text-sm">
            No results found
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
                <Command.GroupHeading class="px-3 py-1.5 text-[10px] uppercase tracking-wider text-text-muted font-semibold">
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
                      {#if cmd.shortcut}
                        <kbd class="text-[11px] font-mono text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded border border-border-subtle shrink-0">
                          {formatShortcut(cmd.shortcut)}
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
    padding: 8px 12px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    font-size: 14px;
    transition: background 0.1s;
  }
  :global(.cmd-item:hover) {
    background: var(--color-bg-hover);
  }
  :global(.cmd-item[data-selected]) {
    background: var(--color-bg-active);
    outline: 1px solid var(--color-border);
  }
</style>
