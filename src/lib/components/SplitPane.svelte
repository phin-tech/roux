<script lang="ts">
  import SplitPane from "./SplitPane.svelte";
  import Terminal from "./Terminal.svelte";
  import ShellTerminal from "./ShellTerminal.svelte";
  import DocPane from "./DocPane.svelte";
  import { focusedPaneId, removePane, type SplitNode } from "$lib/stores/panes";

  interface Props {
    node: SplitNode;
    sessionId: string;
    sessionActive: boolean;
  }

  let { node, sessionId, sessionActive }: Props = $props();
</script>

{#if node.kind === "pane"}
  {#key node.pane.id}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="flex-1 min-h-0 min-w-0 relative"
      class:ring-1={$focusedPaneId === node.pane.id}
      class:ring-accent-dim={$focusedPaneId === node.pane.id}
      onclick={() => focusedPaneId.set(node.pane.id)}
    >
      {#if node.pane.type === "claude"}
        <Terminal sessionId={node.pane.ptyId} active={sessionActive} />
      {:else if node.pane.type === "doc"}
        <DocPane
          docPath={node.pane.docPath ?? ""}
          onClose={() => removePane(sessionId, node.pane.id)}
        />
      {:else}
        <ShellTerminal
          ptyId={node.pane.ptyId}
          paneId={node.pane.id}
          onClose={() => removePane(sessionId, node.pane.id)}
        />
      {/if}
    </div>
  {/key}
{:else}
  <div
    class="flex flex-1 min-h-0 min-w-0"
    class:flex-row={node.direction === "horizontal"}
    class:flex-col={node.direction === "vertical"}
  >
    {#each node.children as child, i}
      {#if i > 0}
        <div
          class:w-px={node.direction === "horizontal"}
          class:h-px={node.direction === "vertical"}
          class="bg-border-subtle shrink-0"
        ></div>
      {/if}
      <SplitPane node={child} {sessionId} {sessionActive} />
    {/each}
  </div>
{/if}
