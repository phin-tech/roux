<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    activeSidebar,
    pinnedSidebar,
    closeSidebar,
    closePinned,
    unpinSidebar,
    pinSidebar,
    PINNABLE_SIDEBARS,
    notesOverrideSessionId,
    type SidebarId,
  } from "$lib/stores/ui";
  import {
    sidebarLayout,
    setDockWidth,
    setDockSplit,
    MIN_DOCK_WIDTH,
    MAX_DOCK_WIDTH,
  } from "$lib/stores/sidebarLayout";
  import { activeSession, sessionList } from "$lib/stores/sessions";
  import { archivedSessionsState } from "$lib/stores/archivedSessions";
  import { projects } from "$lib/stores/projects";
  import NotesPanel from "./NotesPanel.svelte";
  import WorktrunkPanel from "./WorktrunkPanel.svelte";
  import WatchesPane from "./WatchesPane.svelte";
  import NotificationsPane from "./NotificationsPane.svelte";
  import DocPanel from "./DocPanel.svelte";
  import LibraryPanel from "./LibraryPanel.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import SessionTabs from "./SessionTabs.svelte";

  interface Props {
    onNewSession: () => void;
  }

  let {
    onNewSession,
  }: Props = $props();

  type Slot = "hidden" | "solo" | "pinned-half" | "active-half";

  let dockWidth = $derived($sidebarLayout.width);
  let splitRatio = $derived($sidebarLayout.splitRatio);
  let railSide = $derived($sidebarLayout.railSide);
  let widthDragging = $state(false);
  let splitDragging = $state(false);

  function clampWidth(w: number): number {
    return Math.max(MIN_DOCK_WIDTH, Math.min(MAX_DOCK_WIDTH, w));
  }

  let dockEl = $state<HTMLDivElement | null>(null);

  // Active drag teardown. Keeping a ref lets us guarantee cleanup on
  // onDestroy / Escape / window blur — covers cases where mouseup never fires
  // (drag outside window, browser focus loss, component unmount mid-drag).
  let dragTeardown: (() => void) | null = null;

  function endDrag(): void {
    dragTeardown?.();
    dragTeardown = null;
    widthDragging = false;
    splitDragging = false;
  }

  function installDragHandlers(
    onMove: (ev: MouseEvent) => void,
  ): void {
    const onUp = () => endDrag();
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") endDrag();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("blur", endDrag);
    window.addEventListener("keydown", onKey);
    dragTeardown = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("blur", endDrag);
      window.removeEventListener("keydown", onKey);
    };
  }

  function onWidthDragStart(e: MouseEvent) {
    e.preventDefault();
    endDrag();
    widthDragging = true;
    const startX = e.clientX;
    const startW = dockWidth;
    // Dock sits on the same side as the rail, so its resize handle faces main.
    // railSide=left → handle on right edge → drag right grows.
    // railSide=right → handle on left edge → drag left grows.
    const sign = railSide === "left" ? 1 : -1;
    installDragHandlers((ev) => {
      setDockWidth(clampWidth(startW + sign * (ev.clientX - startX)));
    });
  }

  function onSplitDragStart(e: MouseEvent) {
    e.preventDefault();
    if (!dockEl) return;
    endDrag();
    splitDragging = true;
    installDragHandlers((ev) => {
      // Recompute rect each move — dock height may change (split itself, window
      // resize, status bar toggle) during a drag.
      if (!dockEl) return;
      const rect = dockEl.getBoundingClientRect();
      if (rect.height <= 0) return;
      setDockSplit((ev.clientY - rect.top) / rect.height);
    });
  }

  onDestroy(() => endDrag());

  // Settings renders as a modal (own component); everything else lives in the dock.
  const DOCK_PANEL_IDS: readonly SidebarId[] = [
    "sessions",
    "notes",
    "watches",
    "library",
    "tasks",
    "notifications",
    "docs",
    "worktrunk",
  ];
  const DOCK_SET = new Set<SidebarId>(DOCK_PANEL_IDS);

  let pinned = $derived(
    $pinnedSidebar && DOCK_SET.has($pinnedSidebar) ? $pinnedSidebar : null,
  );
  let active = $derived(
    $activeSidebar && DOCK_SET.has($activeSidebar) ? $activeSidebar : null,
  );

  // Docs takes over the whole dock when open, regardless of pin state.
  let takeover = $derived<SidebarId | null>(active === "docs" ? "docs" : null);

  let anyVisible = $derived(pinned !== null || active !== null);

  let splitMode = $derived(
    takeover === null &&
      pinned !== null &&
      active !== null &&
      pinned !== active,
  );

  function slotFor(id: SidebarId): Slot {
    if (takeover !== null) {
      return id === takeover ? "solo" : "hidden";
    }
    if (splitMode) {
      if (id === pinned) return "pinned-half";
      if (id === active) return "active-half";
      return "hidden";
    }
    // Solo: prefer active, fall back to pinned
    const solo = active ?? pinned;
    return id === solo ? "solo" : "hidden";
  }

  function slotStyle(slot: Slot, ratio: number): string {
    if (slot === "hidden") return "display: none;";
    if (slot === "solo") return "position: absolute; inset: 0;";
    if (slot === "pinned-half") {
      return `position: absolute; left: 0; right: 0; top: 0; height: calc(${ratio * 100}% - 2px);`;
    }
    // active-half
    return `position: absolute; left: 0; right: 0; top: calc(${ratio * 100}% + 2px); bottom: 0;`;
  }

  function onCloseFor(id: SidebarId): () => void {
    // Close button (×) on a panel = "dismiss THIS panel" — clear whichever
    // slot(s) hold it. Do NOT trigger the anchor-promotion that unpinSidebar
    // does (that's reserved for the explicit PinButton / unpin actions).
    return () => {
      if ($pinnedSidebar === id) closePinned();
      if ($activeSidebar === id) closeSidebar();
    };
  }

  function onTogglePinFor(id: SidebarId): (() => void) | undefined {
    if (!PINNABLE_SIDEBARS.has(id)) return undefined;
    return () => {
      if ($pinnedSidebar === id) {
        unpinSidebar();
      } else {
        pinSidebar(id);
      }
    };
  }

  // Contexts needed by panels
  let activeSessionData = $derived($activeSession);
  let notesSessionData = $derived(
    $notesOverrideSessionId
      ? ($sessionList.find((s) => s.id === $notesOverrideSessionId)
        ?? $archivedSessionsState.sessions.find((s) => s.id === $notesOverrideSessionId))
      : activeSessionData,
  );
</script>

{#snippet widthHandle()}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="group relative flex w-1 shrink-0 cursor-col-resize self-stretch flex-col items-center"
    onmousedown={onWidthDragStart}
  >
    <div
      class="min-h-0 max-w-[0.5px] min-w-[0.5px] flex-1 transition-all duration-150 {widthDragging ? 'bg-white/30' : 'bg-white/20 group-hover:bg-white/40'}"
    ></div>
  </div>
{/snippet}

{#if anyVisible}
  <div class="relative flex shrink-0">
    {#if railSide === "right"}
      {@render widthHandle()}
    {/if}
    <div
      bind:this={dockEl}
      class="relative h-full shrink-0 bg-bg-deep"
      style="width: {dockWidth}px"
    >
      {#each DOCK_PANEL_IDS as id (id)}
        {@const slot = slotFor(id)}
        {@const visible = slot !== "hidden"}
        <div style={slotStyle(slot, splitRatio)}>
          {#if id === "sessions"}
            <SessionTabs
              {onNewSession}
              onclose={onCloseFor(id)}
              pinned={$pinnedSidebar === id}
              onTogglePin={onTogglePinFor(id)}
            />
          {:else if id === "notes"}
            <NotesPanel
              {visible}
              sessionId={notesSessionData?.id ?? null}
              projectId={notesSessionData?.projectId ?? null}
              projectName={$projects.find((p) => p.id === notesSessionData?.projectId)?.name ?? null}
              repoRoot={notesSessionData?.repoRoot ?? null}
              onclose={onCloseFor(id)}
              pinned={$pinnedSidebar === id}
              onTogglePin={onTogglePinFor(id)}
            />
          {:else if id === "watches"}
            <WatchesPane
              {visible}
              onclose={onCloseFor(id)}
              pinned={$pinnedSidebar === id}
              onTogglePin={onTogglePinFor(id)}
            />
          {:else if id === "library"}
            <LibraryPanel
              {visible}
              onclose={onCloseFor(id)}
              pinned={$pinnedSidebar === id}
              onTogglePin={onTogglePinFor(id)}
            />
          {:else if id === "tasks"}
            <TaskPanel
              {visible}
              onCollapse={onCloseFor(id)}
              pinned={$pinnedSidebar === id}
              onTogglePin={onTogglePinFor(id)}
            />
          {:else if id === "notifications"}
            <NotificationsPane
              {visible}
              onclose={onCloseFor(id)}
              pinned={$pinnedSidebar === id}
              onTogglePin={onTogglePinFor(id)}
            />
          {:else if id === "docs"}
            <DocPanel {visible} onclose={onCloseFor(id)} />
          {:else if id === "worktrunk"}
            <WorktrunkPanel
              {visible}
              onclose={onCloseFor(id)}
              pinned={$pinnedSidebar === id}
              onTogglePin={onTogglePinFor(id)}
            />
          {/if}
        </div>
      {/each}

      {#if splitMode}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="absolute left-0 right-0 h-1 cursor-row-resize"
          style="top: calc({splitRatio * 100}% - 2px);"
          onmousedown={onSplitDragStart}
        >
          <div
            class="h-px w-full translate-y-[2px] transition-all duration-150 {splitDragging ? 'bg-white/30' : 'bg-white/20 hover:bg-white/40'}"
          ></div>
        </div>
      {/if}
    </div>

    {#if railSide === "left"}
      {@render widthHandle()}
    {/if}
  </div>
{/if}
