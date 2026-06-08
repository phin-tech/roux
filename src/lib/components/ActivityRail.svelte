<script lang="ts">
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import Eye from "@lucide/svelte/icons/eye";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import Library from "@lucide/svelte/icons/library";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Bell from "@lucide/svelte/icons/bell";
  import Inbox from "@lucide/svelte/icons/inbox";
  import TerminalSquare from "@lucide/svelte/icons/square-terminal";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import Trees from "@lucide/svelte/icons/trees";
  import Kanban from "@lucide/svelte/icons/kanban";
  import Pin from "@lucide/svelte/icons/pin";
  import { worktrunkDetection } from "$lib/stores/worktrunkDetection";
  import type { Component } from "svelte";
  import {
    activeSidebar,
    closeSidebar,
    isPinned,
    openSidebar,
    pinnedSidebar,
    PINNABLE_SIDEBARS,
    pinSidebar,
    unpinSidebar,
    type SidebarId,
  } from "$lib/stores/ui";
  import { sidebarLayout } from "$lib/stores/sidebarLayout";
  import {
    mainViewRoute,
    openMainView,
    closeMainView,
  } from "$lib/stores/mainView";
  import { unreadTotal } from "$lib/stores/notifications";
  import { meUnread } from "$lib/stores/mailbox";

  interface DockItem {
    id: SidebarId;
    label: string;
    icon: Component<{ size?: number; class?: string }>;
  }

  interface RailActionItem {
    label: string;
    icon: Component<{ size?: number; class?: string }>;
  }

  // Static rail items. The Worktrunk icon is appended dynamically below
  // only when its binary is detected.
  const baseDockItems: DockItem[] = [
    { id: "sessions", label: "Sessions", icon: FolderTree },
    { id: "notes", label: "Notes", icon: StickyNote },
    { id: "watches", label: "Watches", icon: Eye },
    { id: "library", label: "Library", icon: Library },
    { id: "tasks", label: "Tasks", icon: ListTodo },
    { id: "board", label: "Board", icon: Kanban },
    { id: "ptys", label: "PTYs", icon: TerminalSquare },
    { id: "docs", label: "Docs", icon: BookOpen },
    { id: "notifications", label: "Notifications", icon: Bell },
    { id: "mailbox", label: "Mailbox", icon: Inbox },
  ];
  const worktrunkItem: DockItem = {
    id: "worktrunk",
    label: "Worktrunk",
    icon: Trees,
  };

  let dockItems = $derived.by<DockItem[]>(() => {
    const items = [...baseDockItems];
    if ($worktrunkDetection.binaryPath) items.push(worktrunkItem);
    return items;
  });

  const settingsItem: RailActionItem = {
    label: "Settings",
    icon: SettingsIcon,
  };

  function handleClick(event: MouseEvent, id: SidebarId): void {
    event.preventDefault();
    // When the dock is collapsed to icons, any rail click should bring it
    // back — there's no visible panel to dismiss, and the pinned-icon
    // branch below would otherwise make plain-clicks dead. openSidebar()
    // calls showSidebar() internally and is a no-op on the active slot
    // when id is already pinned, so the pinned panel re-appears correctly.
    if ($sidebarLayout.hidden) {
      openSidebar(id);
      return;
    }
    if ($pinnedSidebar === id) {
      if (event.shiftKey) unpinSidebar();
      return;
    }
    if ($activeSidebar === id) {
      closeSidebar();
      return;
    }
    openSidebar(id);
  }

  function handleContextMenu(event: MouseEvent, id: SidebarId): void {
    event.preventDefault();
    if (!PINNABLE_SIDEBARS.has(id)) return;
    if (isPinned(id)) {
      unpinSidebar();
    } else {
      pinSidebar(id);
    }
  }

  function handleSettingsClick(event: MouseEvent): void {
    event.preventDefault();
    if ($mainViewRoute?.kind === "preferences") {
      closeMainView();
      return;
    }
    openMainView({ kind: "preferences", category: "general" });
  }

  function buttonTitle(item: DockItem): string {
    return item.label;
  }

  const tooltipBaseClass =
    "pointer-events-none absolute top-1/2 z-50 -translate-y-1/2 whitespace-nowrap rounded border border-border-subtle bg-bg-elevated px-2 py-1 text-[11px] font-medium text-text-primary opacity-0 shadow-lg shadow-black/30 transition-opacity duration-75 group-hover:opacity-100 group-focus-visible:opacity-100";

  function tooltipClass(): string {
    return $sidebarLayout.railSide === "right"
      ? `${tooltipBaseClass} right-full mr-1`
      : `${tooltipBaseClass} left-full ml-1`;
  }
</script>

<div class="flex h-full flex-col items-center p-1">
  {#each dockItems as item (item.id)}
    {@const active = $activeSidebar === item.id}
    {@const pinned = $pinnedSidebar === item.id}
    {@const badgeCount =
      item.id === "notifications"
        ? $unreadTotal
        : item.id === "mailbox"
          ? $meUnread
          : 0}
    {@const showBadge = badgeCount > 0}
    <button
      type="button"
      aria-label={item.label}
      aria-pressed={active || pinned}
      class="group relative flex h-7 w-7 shrink-0 items-center justify-center rounded text-text-secondary transition-colors hover:bg-white/5 hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 {active
        ? 'bg-white/10 text-text-primary'
        : ''} {pinned ? 'text-accent' : ''}"
      onclick={(e) => handleClick(e, item.id)}
      oncontextmenu={(e) => handleContextMenu(e, item.id)}
    >
      <item.icon size={16} />
      <span class={tooltipClass()} aria-hidden="true">{buttonTitle(item)}</span>
      {#if pinned}
        <span
          class="absolute -top-0.5 -right-0.5 flex h-3 w-3 items-center justify-center rounded-full bg-accent text-[8px] text-bg-deep"
        >
          <Pin size={8} />
        </span>
      {/if}
      {#if showBadge}
        <span
          class="absolute -top-1 -right-1 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-red-500 px-1 text-[9px] font-semibold leading-none text-white"
        >
          {badgeCount > 9 ? "9+" : badgeCount}
        </span>
      {/if}
    </button>
  {/each}

  <div class="flex-1"></div>

  <button
    type="button"
    aria-label={settingsItem.label}
    aria-pressed={$mainViewRoute?.kind === "preferences"}
    class="group relative flex h-7 w-7 shrink-0 items-center justify-center rounded text-text-secondary transition-colors hover:bg-white/5 hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 {$mainViewRoute?.kind ===
    'preferences'
      ? 'bg-white/10 text-text-primary'
      : ''}"
    onclick={handleSettingsClick}
  >
    <settingsItem.icon size={16} />
    <span class={tooltipClass()} aria-hidden="true">{settingsItem.label}</span>
  </button>
</div>
