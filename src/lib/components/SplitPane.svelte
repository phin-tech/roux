<script lang="ts">
  import SplitPane from "./SplitPane.svelte";
  import Terminal from "./Terminal.svelte";
  import ShellTerminal from "./ShellTerminal.svelte";
  import CommandPane from "./CommandPane.svelte";
  import MarkdownPane from "./MarkdownPane.svelte";
  import { focusedPaneId, renamePane, setActiveStackIndex, getStackLabel, type SplitNode } from "$lib/stores/panes";
  import { closePane } from "$lib/panes/actions";

  let editingName = $state(false);
  let nameInput = $state("");

  function startRenaming(currentName: string) {
    nameInput = currentName;
    editingName = true;
  }

  function commitRename(paneId: string) {
    renamePane(sessionId, paneId, nameInput.trim());
    editingName = false;
  }

  function paneTypeLabel(type: string): string {
    switch (type) {
      case "claude": return "claude";
      case "shell": return "shell";
      case "markdown": return "doc";
      case "command": return "cmd";
      default: return type;
    }
  }

  function handlePaneMouseDown(e: MouseEvent, paneId: string) {
    focusedPaneId.set(paneId);
    const container = e.currentTarget as HTMLElement;
    focusTerminal(container);
  }

  function focusTerminal(container: HTMLElement) {
    setTimeout(() => {
      container.dispatchEvent(new CustomEvent("pane-focus", { bubbles: true }));
    }, 0);
  }

  interface Props {
    node: SplitNode;
    sessionId: string;
    sessionActive: boolean;
  }

  let { node, sessionId, sessionActive }: Props = $props();
  let paneContainer: HTMLElement | undefined = $state();

  // Focus terminal when this pane becomes focused via keyboard navigation
  $effect(() => {
    if (node.kind === "pane" && $focusedPaneId === node.pane.id && paneContainer) {
      focusTerminal(paneContainer);
    }
  });
</script>

{#if node.kind === "pane"}
  {#key node.pane.id}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      bind:this={paneContainer}
      class="relative flex flex-col flex-1 min-h-0 min-w-0 overflow-hidden rounded-lg transition-colors {$focusedPaneId === node.pane.id ? 'bg-bg-surface/60 shadow-[inset_0_1px_0_rgba(255,255,255,0.05)]' : 'bg-bg-deep shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]'}"
      onmousedown={(e) => handlePaneMouseDown(e, node.pane.id)}
    >
      <!-- Mini title bar -->
      <div
        class="flex h-7 shrink-0 select-none items-center border-b border-hairline/50 px-2.5 gap-2"
        ondblclick={() => startRenaming(node.pane.name ?? "")}
      >
        <span class="text-[10px] uppercase tracking-wider text-text-muted/60 shrink-0">{paneTypeLabel(node.pane.type)}</span>
        {#if editingName}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            type="text"
            class="flex-1 min-w-0 bg-transparent text-[11px] text-text-primary font-mono outline-none placeholder:text-text-muted/40"
            placeholder="name this pane..."
            bind:value={nameInput}
            autofocus
            onblur={() => commitRename(node.pane.id)}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename(node.pane.id);
              if (e.key === "Escape") { editingName = false; }
            }}
          />
        {:else if node.pane.name}
          <span class="text-[11px] text-text-secondary font-mono truncate">{node.pane.name}</span>
        {/if}
      </div>

      <div class="flex-1 min-h-0 min-w-0">
        {#if node.pane.type === "claude"}
          <Terminal sessionId={node.pane.ptyId} active={sessionActive} />
        {:else if node.pane.type === "markdown"}
          <MarkdownPane
            docPath={node.pane.docPath ?? ""}
            onClose={async () => {
              await closePane(sessionId, node.pane.id);
            }}
          />
        {:else if node.pane.type === "command"}
          <CommandPane
            command={node.pane.command ?? ""}
            workingDir={node.pane.workingDir ?? ""}
            paneId={node.pane.id}
            initialPtyId={node.pane.ptyId}
            active={sessionActive}
            onClose={async () => {
              await closePane(sessionId, node.pane.id);
            }}
          />
        {:else}
          <ShellTerminal
            ptyId={node.pane.ptyId}
            paneId={node.pane.id}
            active={sessionActive}
            closeOnExit={!node.pane.id.startsWith("task-")}
            onClose={async () => {
              await closePane(sessionId, node.pane.id);
            }}
          />
        {/if}
      </div>
    </div>
  {/key}
{:else if node.stacked}
  <!-- Stacked view: collapsed title bars + one expanded child -->
  <!-- All children stay mounted (hidden via CSS) so terminals keep their state -->
  <div class="flex flex-col flex-1 min-h-0 min-w-0">
    {#each node.children as child, i}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="flex items-center h-7 shrink-0 select-none border-b border-hairline/50 px-2.5 gap-2 cursor-pointer {i === (node.activeIndex ?? 0) ? 'bg-bg-surface/60' : 'hover:bg-bg-surface/30 transition-colors'}"
        onclick={() => setActiveStackIndex(sessionId, i)}
      >
        <span class="text-[10px] text-text-muted/60 shrink-0">{i === (node.activeIndex ?? 0) ? '\u25BE' : '\u25B8'}</span>
        <span class="text-[11px] font-mono truncate {i === (node.activeIndex ?? 0) ? 'text-text-secondary' : 'text-text-muted'}">{getStackLabel(child)}</span>
      </div>
      <div class="min-h-0 min-w-0 {i === (node.activeIndex ?? 0) ? 'flex-1' : 'hidden'}">
        <SplitPane node={child} {sessionId} {sessionActive} />
      </div>
    {/each}
  </div>
{:else}
  <div
    class="flex flex-1 min-h-0 min-w-0 gap-1"
    class:flex-row={node.direction === "horizontal"}
    class:flex-col={node.direction === "vertical"}
  >
    {#each node.children as child}
      <SplitPane node={child} {sessionId} {sessionActive} />
    {/each}
  </div>
{/if}
