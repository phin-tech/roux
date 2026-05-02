<script lang="ts">
  import {
    notifications,
    markAllNotificationsRead,
    clearNotifications,
    removeNotification,
  } from "$lib/stores/notifications";
  import { sessionList } from "$lib/stores/sessions";
  import { dispatchNotificationAction } from "$lib/notifications/dispatchAction";
  import type { Notification, NotificationAction, NotificationLevel, NotificationSource } from "$lib/types";

  import PinButton from "./PinButton.svelte";
  import CloseButton from "./CloseButton.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();

  interface Group {
    key: string;
    label: string;
    notifications: Notification[];
  }

  let grouped = $derived.by((): Group[] => {
    if (!visible) return [];
    const groups: Group[] = [];
    const globals = $notifications.filter((n) => n.sessionId == null);
    if (globals.length > 0) {
      groups.push({ key: "__global__", label: "Global", notifications: globals });
    }
    const sessionMap = new Map<string, Notification[]>();
    for (const n of $notifications) {
      if (n.sessionId == null) continue;
      const list = sessionMap.get(n.sessionId) ?? [];
      list.push(n);
      sessionMap.set(n.sessionId, list);
    }
    for (const [sid, ns] of sessionMap) {
      const session = $sessionList.find((s) => s.id === sid);
      const label = session ? session.name : `Session ${sid.slice(0, 8)}`;
      groups.push({ key: `s-${sid}`, label, notifications: ns });
    }
    return groups;
  });

  function levelColor(level: NotificationLevel): string {
    switch (level) {
      case "success": return "bg-green";
      case "error": return "bg-red";
      case "warning": return "bg-amber";
      case "attention": return "bg-amber";
      default: return "bg-text-muted";
    }
  }

  function sourceLabel(source: NotificationSource): string {
    switch (source.type) {
      case "hook": return source.provider;
      case "watch": return "watch";
      case "task": return "task";
      case "cli": return "cli";
      case "osc": return source.code === 9 ? "terminal" : `osc ${source.code}`;
      case "internal": return "roux";
    }
  }

  function formatRelative(ts: number): string {
    const secs = Math.floor((Date.now() - ts) / 1000);
    if (secs < 10) return "just now";
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
    return `${Math.floor(secs / 86400)}d ago`;
  }

  function primaryAction(n: Notification): NotificationAction | undefined {
    return n.actions.find((a) => a.primary) ?? n.actions[0];
  }

  async function handleRowClick(n: Notification) {
    const action = primaryAction(n);
    if (action) {
      await dispatchNotificationAction(n.id, action);
    }
  }

  async function handleAction(e: Event, n: Notification, action: NotificationAction) {
    e.stopPropagation();
    await dispatchNotificationAction(n.id, action);
  }

  async function handleDismiss(e: Event, n: Notification) {
    e.stopPropagation();
    await removeNotification(n.id);
  }

  async function handleMarkAllRead() {
    await markAllNotificationsRead();
  }

  async function handleClearAll() {
    await clearNotifications();
  }
</script>

<div
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <SidebarPanelHeader title="Notifications">
    {#snippet actions()}
      {#if $notifications.length > 0}
        <button
          class="cursor-pointer rounded border border-transparent bg-transparent px-2 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
          onclick={handleMarkAllRead}
          title="Mark all read"
        >mark all</button>
        <button
          class="cursor-pointer rounded border border-transparent bg-transparent px-2 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
          onclick={handleClearAll}
          title="Clear all"
        >clear</button>
      {/if}
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <CollapseSidebarButton
        onclick={onclose}
        label="Collapse notifications sidebar"
        title="Collapse notifications sidebar"
      />
    {/snippet}
  </SidebarPanelHeader>

  <div class="flex-1 overflow-y-auto p-2">
    {#if $notifications.length === 0}
      <div class="flex h-full items-center justify-center text-sm text-text-muted">
        No notifications
      </div>
    {:else}
      {#each grouped as group (group.key)}
        <div class="mb-3">
          <div class="mb-1 flex items-center gap-1 px-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">
            <span>{group.label}</span>
            <span class="text-text-muted/60">· {group.notifications.length}</span>
          </div>
          {#each group.notifications as n (n.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="mb-1 flex cursor-pointer flex-col gap-1 rounded-lg border border-transparent bg-transparent px-2 py-1.5 text-left text-sm hover:border-border-subtle hover:bg-bg-hover {!n.read ? 'bg-bg-surface/30' : ''}"
              onclick={() => handleRowClick(n)}
            >
              <div class="flex items-center gap-2">
                <span
                  class="inline-block h-2 w-2 shrink-0 rounded-full {levelColor(n.level)}"
                  class:opacity-40={n.read}
                ></span>
                <span class="min-w-0 flex-1 truncate {n.read ? 'text-text-muted' : 'text-text-primary'}">{n.title}</span>
                <span class="shrink-0 text-[9px] uppercase tracking-wider text-text-muted/60">{sourceLabel(n.source)}</span>
                <span class="shrink-0 text-[10px] text-text-muted">{formatRelative(n.createdAt)}</span>
                <CloseButton
                  class="shrink-0 p-0.5 hover:border-transparent hover:text-red"
                  onclick={(e) => handleDismiss(e, n)}
                  label="Dismiss notification"
                  title="Dismiss"
                  size={12}
                />
              </div>

              {#if n.subtitle}
                <div class="ml-4 text-[11px] text-text-secondary truncate">{n.subtitle}</div>
              {/if}
              {#if n.body}
                <div class="ml-4 text-[11px] text-text-muted truncate">{n.body}</div>
              {/if}

              {#if n.actions.length > 0}
                <div class="ml-4 mt-1 flex flex-wrap gap-1">
                  {#each n.actions as action (action.id)}
                    {#if action.kind.type !== "dismiss"}
                      <button
                        class="cursor-pointer rounded border border-border-subtle bg-transparent px-1.5 py-0.5 text-[10px] text-text-secondary hover:border-accent hover:bg-bg-hover hover:text-text-primary {action.primary ? 'border-accent-dim text-text-primary' : ''}"
                        onclick={(e) => handleAction(e, n, action)}
                      >{action.label}</button>
                    {/if}
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>
