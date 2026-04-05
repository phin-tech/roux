<script lang="ts">
  import SplitPane from "./SplitPane.svelte";
  import Terminal from "./Terminal.svelte";
  import ShellTerminal from "./ShellTerminal.svelte";
  import CommandPane from "./CommandPane.svelte";
  import DocPane from "./DocPane.svelte";
  import { focusedPaneId, type SplitNode } from "$lib/stores/panes";
  import { closePane } from "$lib/panes/actions";

  function handlePaneMouseDown(e: MouseEvent, paneId: string) {
    focusedPaneId.set(paneId);
    const container = e.currentTarget as HTMLElement;
    // Defer focus to after xterm's own mousedown handling completes
    setTimeout(() => {
      const textarea = container?.querySelector<HTMLTextAreaElement>(".xterm-helper-textarea");
      textarea?.focus();
    }, 0);
  }

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
      class="relative flex-1 min-h-0 min-w-0 overflow-hidden rounded-xl transition-colors {$focusedPaneId === node.pane.id ? 'bg-bg-surface/60 shadow-[inset_0_1px_0_rgba(255,255,255,0.05)]' : 'bg-bg-deep shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]'}"
      onmousedown={(e) => handlePaneMouseDown(e, node.pane.id)}
    >
      {#if node.pane.type === "claude"}
        <Terminal sessionId={node.pane.ptyId} active={sessionActive} />
      {:else if node.pane.type === "doc"}
        <DocPane
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
  {/key}
{:else}
  <div
    class="flex flex-1 min-h-0 min-w-0 gap-2"
    class:flex-row={node.direction === "horizontal"}
    class:flex-col={node.direction === "vertical"}
  >
    {#each node.children as child}
      <SplitPane node={child} {sessionId} {sessionActive} />
    {/each}
  </div>
{/if}
