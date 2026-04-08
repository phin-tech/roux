<script lang="ts">
  import SplitPane from "./SplitPane.svelte";
  import PaneShell from "./PaneShell.svelte";
  import { fullscreenPaneId } from "$lib/panes/focus";
  import { paneInstances } from "$lib/panes/instances";
  import { getDropSide } from "$lib/panes/dropTarget";
  import { movePane, containsPaneId, setActiveStackIndex, type DropSide, type LayoutNode } from "$lib/panes/layout";
  import { draggedPaneId, dropTarget, resetPaneDrag } from "$lib/stores/paneDrag";

  interface Props {
    node: LayoutNode;
    sessionId: string;
    visible?: boolean;
  }

  let { node, sessionId, visible = true }: Props = $props();

  function getStackDisplayLabel(node: LayoutNode): string {
    if (node.kind === "leaf") {
      const inst = $paneInstances.get(node.paneId);
      return inst?.name ?? inst?.type ?? node.paneId;
    }
    return node.children.map(getStackDisplayLabel).join(" | ");
  }

  function dropOverlayClass(side: DropSide): string {
    switch (side) {
      case "left":
        return "left-0 top-0 h-full w-[28%] border-l-2";
      case "right":
        return "right-0 top-0 h-full w-[28%] border-r-2";
      case "top":
        return "left-0 top-0 h-[28%] w-full border-t-2";
      case "bottom":
        return "bottom-0 left-0 h-[28%] w-full border-b-2";
    }
  }

  function handleLeafDragOver(event: DragEvent, paneId: string) {
    if (!visible || !$draggedPaneId || $draggedPaneId === paneId) return;
    if (!(event.currentTarget instanceof HTMLElement)) return;
    event.preventDefault();
    const side = getDropSide(
      event.currentTarget.getBoundingClientRect(),
      event.clientX,
      event.clientY
    );
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = "move";
    }
    dropTarget.set({ paneId, side });
  }

  function handleLeafDragLeave(event: DragEvent, paneId: string) {
    const currentTarget = event.currentTarget;
    const related = event.relatedTarget;
    if (
      currentTarget instanceof HTMLElement &&
      related instanceof Node &&
      currentTarget.contains(related)
    ) {
      return;
    }
    if ($dropTarget?.paneId === paneId) {
      dropTarget.set(null);
    }
  }

  function handleLeafDrop(event: DragEvent, paneId: string) {
    event.preventDefault();
    const dragged = $draggedPaneId;
    const target = $dropTarget;
    resetPaneDrag();
    if (!visible || !dragged || dragged === paneId || target?.paneId !== paneId) return;
    movePane(sessionId, dragged, paneId, target.side);
  }
</script>

{#if node.kind === "leaf"}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="relative flex flex-1 min-h-0 min-w-0"
    data-drop-pane-id={node.paneId}
    ondragover={(event) => handleLeafDragOver(event, node.paneId)}
    ondragleave={(event) => handleLeafDragLeave(event, node.paneId)}
    ondrop={(event) => handleLeafDrop(event, node.paneId)}
  >
    <PaneShell paneId={node.paneId} {sessionId} {visible} />
    {#if visible && $dropTarget?.paneId === node.paneId && $draggedPaneId !== node.paneId}
      <div
        class={`pointer-events-none absolute z-10 bg-accent/20 shadow-[inset_0_0_0_1px_var(--color-accent)] ${dropOverlayClass($dropTarget.side)}`}
        data-drop-side={$dropTarget.side}
      ></div>
    {/if}
  </div>
{:else if node.stacked}
  <!-- Stacked view: Zellij-style with collapsed tabs and expanded active pane -->
  <!-- All children stay mounted (hidden via CSS) so terminals keep their state -->
  <div class="flex flex-col flex-1 min-h-0 min-w-0">
    {#each node.children as child, i}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="flex items-center h-6 shrink-0 select-none border-b border-hairline px-2 gap-1.5 cursor-pointer transition-[background-color,box-shadow] duration-150 {i === (node.activeIndex ?? 0) ? 'bg-bg-deep shadow-[inset_0_2px_0_var(--color-accent-dim)]' : 'bg-transparent hover:bg-bg-surface/30'}"
        onclick={() => setActiveStackIndex(sessionId, i)}
      >
        <span class="text-[10px] text-text-muted/60 shrink-0">{i === (node.activeIndex ?? 0) ? '\u25BE' : '\u25B8'}</span>
        <span class="text-[11px] font-mono truncate {i === (node.activeIndex ?? 0) ? 'text-text-secondary' : 'text-text-muted'}">{getStackDisplayLabel(child)}</span>
      </div>
      <div class="min-h-0 min-w-0 flex flex-col {i === (node.activeIndex ?? 0) ? 'flex-1' : 'hidden'}">
        {#if child.kind === "leaf"}
          <PaneShell
            paneId={child.paneId}
            {sessionId}
            visible={visible && i === (node.activeIndex ?? 0)}
            suppressTitleAccent={i === (node.activeIndex ?? 0)}
          />
        {:else}
          <SplitPane node={child} {sessionId} visible={visible && i === (node.activeIndex ?? 0)} />
        {/if}
      </div>
    {/each}
  </div>
{:else}
  <div
    class="flex flex-1 min-h-0 min-w-0 gap-px bg-hairline"
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
