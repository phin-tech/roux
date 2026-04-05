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
      class="relative flex-1 min-h-0 min-w-0 overflow-hidden rounded-[1.15rem] shadow-[0_22px_48px_rgba(2,6,23,0.42)] transition-colors ring-1 ring-inset {$focusedPaneId === node.pane.id ? 'bg-zinc-900/80 ring-zinc-700/60' : 'bg-zinc-950/92 ring-zinc-800/50'} focus-within:ring-sky-500/50"
      onmousedown={(e) => handlePaneMouseDown(e, node.pane.id)}
    >
      {#if $focusedPaneId === node.pane.id}
        <div class="absolute left-0 top-3 bottom-3 z-10 w-[2px] rounded-full bg-sky-400 shadow-[0_0_12px_rgba(56,189,248,0.45)]"></div>
      {/if}
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
          onClose={async () => {
            await closePane(sessionId, node.pane.id);
          }}
        />
      {:else}
        <ShellTerminal
          ptyId={node.pane.ptyId}
          paneId={node.pane.id}
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
    class="flex flex-1 min-h-0 min-w-0 gap-3"
    class:flex-row={node.direction === "horizontal"}
    class:flex-col={node.direction === "vertical"}
  >
    {#each node.children as child}
      <SplitPane node={child} {sessionId} {sessionActive} />
    {/each}
  </div>
{/if}
