<script lang="ts">
  import { onDestroy } from "svelte";
  import SplitPane from "./SplitPane.svelte";
  import PaneShell from "./PaneShell.svelte";
  import { fullscreenPaneId } from "$lib/panes/focus";
  import { paneInstances } from "$lib/panes/instances";
  import {
    containsPaneId,
    resizeSplitDivider,
    setActiveStackIndex,
    type LayoutNode,
  } from "$lib/panes/layout";
  import type { Session } from "$lib/types";

  interface Props {
    node: LayoutNode;
    sessionId: string;
    session?: Session | null;
    visible?: boolean;
    path?: number[];
  }

  let { node, sessionId, session = null, visible = true, path = [] }: Props = $props();

  let splitEl: HTMLDivElement | null = $state(null);
  let activeDivider = $state<number | null>(null);
  let dragTeardown: (() => void) | null = null;

  function getStackDisplayLabel(node: LayoutNode): string {
    if (node.kind === "leaf") {
      const inst = $paneInstances.get(node.paneId);
      return inst?.name ?? inst?.type ?? node.paneId;
    }
    return node.children.map(getStackDisplayLabel).join(" | ");
  }

  function endDividerDrag(): void {
    dragTeardown?.();
    dragTeardown = null;
    activeDivider = null;
  }

  function installDividerDragHandlers(onMove: (ev: PointerEvent) => void): void {
    const onUp = () => endDividerDrag();
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") endDividerDrag();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    window.addEventListener("blur", endDividerDrag);
    window.addEventListener("keydown", onKey);
    dragTeardown = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      window.removeEventListener("blur", endDividerDrag);
      window.removeEventListener("keydown", onKey);
    };
  }

  function onDividerPointerDown(ev: PointerEvent, dividerIndex: number): void {
    if (node.kind !== "split" || node.stacked || !splitEl) return;

    ev.preventDefault();
    ev.stopPropagation();
    endDividerDrag();
    activeDivider = dividerIndex;

    let lastPosition = node.direction === "h" ? ev.clientX : ev.clientY;
    const rect = splitEl.getBoundingClientRect();
    const containerSize = node.direction === "h" ? rect.width : rect.height;
    const splitPath = [...path];

    installDividerDragHandlers((moveEv) => {
      const position = node.direction === "h" ? moveEv.clientX : moveEv.clientY;
      resizeSplitDivider(sessionId, splitPath, dividerIndex, position - lastPosition, containerSize);
      lastPosition = position;
    });
  }

  onDestroy(() => endDividerDrag());
</script>

{#if node.kind === "leaf"}
  <PaneShell paneId={node.paneId} {sessionId} {session} {visible} />
{:else if node.stacked}
  <!-- Stacked view: Zellij-style with collapsed tabs and expanded active pane -->
  <!-- All children stay mounted (hidden via CSS) so terminals keep their state -->
  <div class="flex flex-col flex-1 min-h-0 min-w-0">
    {#each node.children as child, i}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="flex items-center h-6 shrink-0 select-none border-b border-hairline px-2 gap-1.5 cursor-pointer transition-[background-color,box-shadow] duration-150 {i === (node.activeIndex ?? 0) ? 'bg-bg-deep shadow-[inset_0_2px_0_var(--color-accent-dim)]' : 'bg-transparent hover:bg-bg-surface/30'}"
        onclick={() => setActiveStackIndex(sessionId, i, path)}
      >
        <span class="text-[10px] text-text-muted/60 shrink-0">{i === (node.activeIndex ?? 0) ? '\u25BE' : '\u25B8'}</span>
        <span class="text-[11px] truncate {i === (node.activeIndex ?? 0) ? 'text-text-secondary' : 'text-text-muted'}">{getStackDisplayLabel(child)}</span>
      </div>
      <div class="min-h-0 min-w-0 flex flex-col {i === (node.activeIndex ?? 0) ? 'flex-1' : 'hidden'}">
        {#if child.kind === "leaf"}
          <PaneShell
            paneId={child.paneId}
            {sessionId}
            {session}
            visible={visible && i === (node.activeIndex ?? 0)}
            suppressTitleAccent={i === (node.activeIndex ?? 0)}
          />
        {:else}
          <SplitPane
            node={child}
            {sessionId}
            {session}
            visible={visible && i === (node.activeIndex ?? 0)}
            path={[...path, i]}
          />
        {/if}
      </div>
    {/each}
  </div>
{:else}
  <div
    bind:this={splitEl}
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
        <SplitPane
          node={child}
          {sessionId}
          {session}
          visible={visible && childVisible}
          path={[...path, i]}
        />
      </div>
      {#if childVisible && i < node.children.length - 1}
        {@const nextChild = node.children[i + 1]}
        {@const nextVisible = !isFullscreenActive || (fsId ? containsPaneId(nextChild, fsId) : false)}
        {#if nextVisible}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            data-testid={`pane-divider-${node.direction}-${i}`}
            role="separator"
            aria-orientation={node.direction === "h" ? "vertical" : "horizontal"}
            class="group relative z-10 shrink-0 select-none touch-none {node.direction === 'h' ? 'cursor-col-resize w-2 -mx-1' : 'cursor-row-resize h-2 -my-1'}"
            onpointerdown={(ev) => onDividerPointerDown(ev, i)}
          >
            <div
              class="pointer-events-none absolute bg-transparent transition-colors duration-150 group-hover:bg-accent-dim/40 {node.direction === 'h' ? 'inset-y-0 left-1/2 w-px -translate-x-1/2' : 'inset-x-0 top-1/2 h-px -translate-y-1/2'}"
              class:bg-accent-dim={activeDivider === i}
            ></div>
          </div>
        {/if}
      {/if}
    {/each}
  </div>
{/if}
