<script lang="ts">
  import SplitPane from "./SplitPane.svelte";
  import PaneShell from "./PaneShell.svelte";
  import { fullscreenPaneId } from "$lib/panes/focus";
  import { containsPaneId, getStackLabel, setActiveStackIndex, type LayoutNode } from "$lib/panes/layout";

  interface Props {
    node: LayoutNode;
    sessionId: string;
    visible?: boolean;
  }

  let { node, sessionId, visible = true }: Props = $props();
</script>

{#if node.kind === "leaf"}
  <PaneShell paneId={node.paneId} {sessionId} {visible} />
{:else if node.stacked}
  <!-- Stacked view: Zellij-style with collapsed tabs and expanded active pane -->
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
      <div class="min-h-0 min-w-0 flex flex-col {i === (node.activeIndex ?? 0) ? 'flex-1' : 'hidden'}">
        <SplitPane node={child} {sessionId} visible={visible && i === (node.activeIndex ?? 0)} />
      </div>
    {/each}
  </div>
{:else}
  <div
    class="flex flex-1 min-h-0 min-w-0 gap-1"
    class:flex-row={node.direction === "h"}
    class:flex-col={node.direction === "v"}
  >
    {#each node.children as child, i}
      {@const fsId = $fullscreenPaneId}
      {@const isFullscreenActive = !!fsId}
      {@const childHasFullscreen = fsId ? containsPaneId(child, fsId) : false}
      {@const childVisible = !isFullscreenActive || childHasFullscreen}
      {@const size = node.sizes?.[i]}
      <div
        class="flex flex-col min-h-0 min-w-0 {childVisible ? '' : 'hidden'}"
        style={childVisible ? `flex: ${size ?? 1}` : ''}
      >
        <SplitPane node={child} {sessionId} visible={visible && childVisible} />
      </div>
    {/each}
  </div>
{/if}
