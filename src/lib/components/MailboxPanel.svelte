<script lang="ts">
  import { onMount } from "svelte";
  import {
    aliases,
    ackEvent,
    clearReadFor,
    events,
    hydrateMailbox,
    markRead,
    postMailboxMessage,
    refreshUnreadCount,
    unreadByAlias,
  } from "$lib/stores/mailbox";
  import { mailboxDeliverToPane } from "$lib/tauri";
  import type { AgentAlias, EventKind } from "$lib/tauri";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import PinButton from "./PinButton.svelte";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();

  // ── Local UI state ──────────────────────────────────────────────────────────
  let selectedAlias = $state<string>("me");
  let composeOpen = $state(false);
  let composeTo = $state("");
  let composeSubject = $state("");
  let composeBody = $state("");
  let composeKind = $state<EventKind>("task");
  let posting = $state(false);
  let postError = $state<string | null>(null);
  let view = $state<"mailbox" | "firehose">("mailbox");

  // Hydrate the first time the panel is shown. (`hydrateMailbox` is
  // idempotent, but a guard keeps it from spamming on re-mount.)
  let hydrated = $state(false);
  $effect(() => {
    if (visible && !hydrated) {
      hydrated = true;
      void hydrateMailbox();
    }
  });

  // Pre-fill `to` once an alias is selected so the compose form is one
  // less click for "reply to whoever I'm looking at."
  $effect(() => {
    if (composeTo === "") {
      composeTo = selectedAlias === "me" ? "" : selectedAlias;
    }
  });

  // ── Derived views ───────────────────────────────────────────────────────────
  let recipientEvents = $derived(
    visible
      ? $events
          .filter((e) => e.to === selectedAlias)
          .slice()
          .reverse() // oldest first for drain semantics
      : [],
  );

  let firehoseEvents = $derived(visible ? $events : []);

  let sortedAliases = $derived.by((): AgentAlias[] => {
    const list = $aliases.slice();
    list.sort((a, b) => {
      // `me` always first.
      if (a.alias === "me") return -1;
      if (b.alias === "me") return 1;
      const ua = $unreadByAlias.get(a.alias) ?? 0;
      const ub = $unreadByAlias.get(b.alias) ?? 0;
      if (ub !== ua) return ub - ua;
      return a.alias.localeCompare(b.alias);
    });
    return list;
  });

  // ── Handlers ────────────────────────────────────────────────────────────────
  function unreadFor(a: string): number {
    return $unreadByAlias.get(a) ?? 0;
  }

  function formatRelative(ts: number): string {
    const secs = Math.floor((Date.now() - ts) / 1000);
    if (secs < 10) return "just now";
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
    return `${Math.floor(secs / 86400)}d ago`;
  }

  function kindColor(kind: EventKind): string {
    switch (kind) {
      case "task":
        return "bg-accent";
      case "question":
        return "bg-amber";
      case "result":
        return "bg-green";
      case "fyi":
        return "bg-text-muted";
      case "signal":
        return "bg-blue-400";
    }
  }

  async function handlePost() {
    if (!composeBody.trim() || !composeTo.trim()) return;
    posting = true;
    postError = null;
    try {
      await postMailboxMessage({
        to: composeTo,
        body: composeBody,
        subject: composeSubject || null,
        kind: composeKind,
      });
      composeBody = "";
      composeSubject = "";
      composeOpen = false;
      // The recipient may not be one of `selectedAlias`'s set, so refresh
      // its count too — covers the cross-repo "post to reviewer" case.
      void refreshUnreadCount(composeTo);
    } catch (err) {
      postError = String(err);
    } finally {
      posting = false;
    }
  }

  async function handleMarkRead(eventId: string) {
    await markRead(eventId, selectedAlias);
  }

  async function handleAck(eventId: string) {
    await ackEvent(eventId, selectedAlias);
  }

  async function handleClearRead() {
    await clearReadFor(selectedAlias);
  }

  let deliveringId = $state<string | null>(null);
  let deliverError = $state<string | null>(null);

  async function handleDeliver(eventId: string) {
    deliveringId = eventId;
    deliverError = null;
    try {
      await mailboxDeliverToPane(eventId);
    } catch (err) {
      deliverError = `Delivery failed: ${String(err)}`;
    } finally {
      deliveringId = null;
    }
  }

  /**
   * True when the event's recipient currently has a pane bound — we
   * only enable the Deliver button in that case. Computed lazily per
   * event row.
   */
  function recipientHasPane(toAlias: string | null): boolean {
    if (!toAlias) return false;
    return $aliases.some((a) => a.alias === toAlias && a.paneId !== null);
  }

  onMount(() => {
    // No teardown needed — the global `mailbox-event` listener is
    // installed at app bootstrap (see `App.svelte` wiring), not here.
  });
</script>

<div
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <SidebarPanelHeader title="Mailbox">
    {#snippet actions()}
      <div
        class="flex items-center gap-0.5 rounded border border-border-subtle bg-bg-surface/40 p-0.5"
        title="Switch between per-alias inbox and the full event firehose"
      >
        <button
          class="rounded px-2 py-0.5 text-[11px] {view === 'mailbox'
            ? 'bg-bg-hover text-text-primary'
            : 'text-text-muted hover:text-text-primary'}"
          onclick={() => (view = "mailbox")}
          title="Mailbox view — events addressed to the selected alias"
        >Inbox</button>
        <button
          class="rounded px-2 py-0.5 text-[11px] {view === 'firehose'
            ? 'bg-bg-hover text-text-primary'
            : 'text-text-muted hover:text-text-primary'}"
          onclick={() => (view = "firehose")}
          title="Firehose view — every event in the store, newest first"
        >All</button>
      </div>
      <button
        class="cursor-pointer rounded border border-transparent bg-transparent px-2 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
        onclick={() => (composeOpen = !composeOpen)}
        title={composeOpen ? "Cancel" : "Compose"}
      >{composeOpen ? "cancel" : "+ new"}</button>
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <CollapseSidebarButton
        onclick={onclose}
        label="Collapse mailbox sidebar"
        title="Collapse mailbox sidebar"
      />
    {/snippet}
  </SidebarPanelHeader>

  {#if composeOpen}
    <form
      class="flex flex-col gap-1 border-b border-border-subtle bg-bg-surface/30 p-2"
      onsubmit={(e) => {
        e.preventDefault();
        void handlePost();
      }}
    >
      <div class="flex gap-1">
        <input
          class="flex-1 rounded border border-border-subtle bg-bg-deep px-2 py-1 text-[11px] text-text-primary placeholder:text-text-muted/60"
          bind:value={composeTo}
          placeholder="To (alias)"
          list="mailbox-compose-aliases"
          autocomplete="off"
          required
        />
        <select
          class="rounded border border-border-subtle bg-bg-deep px-1 py-1 text-[11px] text-text-primary"
          bind:value={composeKind}
        >
          <option value="task">task</option>
          <option value="question">question</option>
          <option value="result">result</option>
          <option value="fyi">fyi</option>
        </select>
      </div>
      <!--
        Native datalist: the input shows whatever the user types verbatim,
        and the browser surfaces matching `$aliases` entries as suggestions.
        New aliases the user types are still accepted — `mailbox-post` will
        ensure-create them on the backend.

        Labels include enough context to disambiguate Claude/Codex/shell
        panes that happen to share a name across sessions, plus a hint
        when the alias was auto-claimed from the pane's name.
      -->
      <datalist id="mailbox-compose-aliases">
        {#each sortedAliases as a (a.alias + (a.projectId ?? ""))}
          {@const status = a.paneId
            ? a.autoClaimed
              ? "auto-claimed"
              : "claimed"
            : a.sessionId
              ? "session"
              : "unbound"}
          {@const projectSuffix = a.projectId ? ` · @${a.projectId}` : ""}
          <option
            value={a.alias}
            label={`${a.alias}${projectSuffix} · ${status}`}
          ></option>
        {/each}
      </datalist>
      <input
        class="rounded border border-border-subtle bg-bg-deep px-2 py-1 text-[11px] text-text-primary placeholder:text-text-muted/60"
        bind:value={composeSubject}
        placeholder="Subject (optional)"
      />
      <textarea
        class="min-h-[60px] resize-y rounded border border-border-subtle bg-bg-deep px-2 py-1 text-[11px] text-text-primary placeholder:text-text-muted/60"
        bind:value={composeBody}
        placeholder="Body"
        required
      ></textarea>
      {#if postError}
        <div class="text-[10px] text-red">{postError}</div>
      {/if}
      <div class="flex justify-end">
        <button
          type="submit"
          class="cursor-pointer rounded border border-accent-dim bg-accent/20 px-2 py-1 text-[11px] text-text-primary hover:bg-accent/30 disabled:opacity-50"
          disabled={posting}
        >{posting ? "Sending…" : "Send"}</button>
      </div>
    </form>
  {/if}

  {#if view === "mailbox"}
    <!-- Alias selector strip -->
    <div
      class="flex shrink-0 gap-1 overflow-x-auto border-b border-border-subtle p-1"
    >
      {#each sortedAliases as a (a.alias + (a.projectId ?? ""))}
        {@const u = unreadFor(a.alias)}
        <button
          class="flex shrink-0 items-center gap-1 rounded border px-2 py-1 text-[11px] {selectedAlias ===
          a.alias
            ? 'border-accent-dim bg-accent/15 text-text-primary'
            : 'border-border-subtle bg-transparent text-text-muted hover:bg-bg-hover hover:text-text-primary'}"
          onclick={() => (selectedAlias = a.alias)}
        >
          <span>{a.alias}</span>
          {#if u > 0}
            <span class="rounded bg-accent px-1 text-[9px] text-bg-deep">{u}</span>
          {/if}
          {#if a.projectId}
            <span class="text-[9px] text-text-muted/70">@{a.projectId}</span>
          {/if}
        </button>
      {:else}
        <span class="px-2 py-1 text-[11px] text-text-muted">No aliases</span>
      {/each}
    </div>

    <div class="flex-1 overflow-y-auto p-2">
      {#if recipientEvents.length === 0}
        <div
          class="flex h-full items-center justify-center text-sm text-text-muted"
        >No mail for {selectedAlias}</div>
      {:else}
        {#each recipientEvents as e (e.id)}
          <article
            class="mb-2 flex flex-col gap-1 rounded-lg border border-border-subtle bg-bg-surface/30 px-2 py-1.5"
          >
            <header class="flex items-center gap-2">
              <span
                class="inline-block h-2 w-2 shrink-0 rounded-full {kindColor(
                  e.kind,
                )}"
              ></span>
              <span class="text-[10px] uppercase tracking-wider text-text-muted"
                >{e.kind}</span
              >
              {#if e.from}
                <span class="truncate text-[10px] text-text-muted"
                  >from {e.from}</span
                >
              {/if}
              <span class="ml-auto shrink-0 text-[10px] text-text-muted/70"
                >{formatRelative(e.createdAt)}</span
              >
            </header>
            {#if e.subject}
              <h4 class="text-[11px] font-medium text-text-primary">
                {e.subject}
              </h4>
            {/if}
            <p class="whitespace-pre-wrap text-[11px] text-text-secondary">
              {e.body}
            </p>
            {#if e.topic}
              <span class="text-[9px] text-text-muted">topic: {e.topic}</span>
            {/if}
            <footer class="flex flex-wrap gap-1">
              <button
                class="cursor-pointer rounded border border-transparent bg-transparent px-1.5 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
                onclick={() => handleMarkRead(e.id)}
              >mark read</button>
              <button
                class="cursor-pointer rounded border border-transparent bg-transparent px-1.5 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
                onclick={() => handleAck(e.id)}
              >ack</button>
              {#if recipientHasPane(e.to)}
                <button
                  class="cursor-pointer rounded border border-accent-dim/60 bg-accent/10 px-1.5 py-0.5 text-[10px] text-accent hover:bg-accent/20 disabled:opacity-50"
                  onclick={() => handleDeliver(e.id)}
                  disabled={deliveringId === e.id}
                  title="Type this message into the recipient's pane and ack it. Bypasses the agent's drain step — use when you want immediate delivery."
                >{deliveringId === e.id ? "delivering…" : "deliver →"}</button>
              {/if}
            </footer>
          </article>
        {/each}
      {/if}
    </div>

    {#if deliverError}
      <div class="shrink-0 border-t border-red-500/40 bg-red-500/10 px-2 py-1 text-[10px] text-red-300">
        {deliverError}
      </div>
    {/if}

    {#if recipientEvents.length > 0}
      <div class="border-t border-border-subtle p-1.5">
        <button
          class="cursor-pointer rounded border border-transparent bg-transparent px-2 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
          onclick={handleClearRead}
        >clear read</button>
      </div>
    {/if}
  {:else}
    <!-- Firehose: every event newest first, no per-recipient state -->
    <div class="flex-1 overflow-y-auto p-2">
      {#if firehoseEvents.length === 0}
        <div
          class="flex h-full items-center justify-center text-sm text-text-muted"
        >No events</div>
      {:else}
        {#each firehoseEvents as e (e.id)}
          <article
            class="mb-1 flex flex-col gap-0.5 rounded border border-transparent bg-transparent px-2 py-1 hover:border-border-subtle hover:bg-bg-hover"
          >
            <div class="flex items-center gap-2 text-[10px]">
              <span
                class="inline-block h-2 w-2 shrink-0 rounded-full {kindColor(
                  e.kind,
                )}"
              ></span>
              <span class="text-text-muted">{e.kind}</span>
              {#if e.from}<span class="truncate text-text-muted"
                  >from {e.from}</span
                >{/if}
              {#if e.to}<span class="truncate text-text-muted"
                  >→ {e.to}</span
                >{/if}
              {#if e.topic}<span class="truncate text-text-muted"
                  >· {e.topic}</span
                >{/if}
              <span class="ml-auto shrink-0 text-text-muted/70"
                >{formatRelative(e.createdAt)}</span
              >
            </div>
            <p class="truncate text-[11px] text-text-secondary">
              {e.subject ?? e.body}
            </p>
          </article>
        {/each}
      {/if}
    </div>
  {/if}
</div>
