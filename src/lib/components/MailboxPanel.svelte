<script lang="ts">
  import { onMount } from "svelte";
  import {
    aliases,
    aliasKey,
    ackEvent,
    clearReadFor,
    events,
    hydrateMailbox,
    mailboxMutationTick,
    markRead,
    postMailboxMessage,
    refreshUnreadCount,
    unreadByAlias,
  } from "$lib/stores/mailbox";
  import {
    createSubscription,
    deleteSubscription,
    subscriptions,
  } from "$lib/stores/subscriptions";
  import {
    mailboxDeliverToPane,
    mailboxListForRecipient,
    mailboxReadState,
  } from "$lib/tauri";
  import type {
    AgentAlias,
    EventKind,
    MailboxEventPayload,
    ReadState,
  } from "$lib/tauri";
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
  // `selectedAlias` and `selectedProjectId` together identify a single
  // entry in `$aliases`. Tracked as separate fields (rather than a
  // compound key string) so filtering / lookup helpers stay obvious.
  // Same alias name in different projects is a real case — without the
  // projectId, "reviewer@proj-a" and "reviewer@proj-b" would silently
  // share an inbox view.
  let selectedAlias = $state<string>("me");
  let selectedProjectId = $state<string | null>(null);
  let composeOpen = $state(false);
  let composeTo = $state("");
  let composeSubject = $state("");
  let composeBody = $state("");
  let composeKind = $state<EventKind>("task");
  let posting = $state(false);
  let postError = $state<string | null>(null);
  let view = $state<"mailbox" | "firehose" | "subscriptions">("mailbox");

  // Subscription compose form state. Shown only when the Subscriptions
  // tab is active. Pattern is validated server-side; we surface the
  // backend error inline to keep the form concrete-feedback-driven.
  let subAlias = $state("");
  let subPattern = $state("");
  let subProjectId = $state<string | null>(null);
  let subSubmitting = $state(false);
  let subError = $state<string | null>(null);
  let deletingSubId = $state<string | null>(null);

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

  // ── Inbox: backend-driven listing ───────────────────────────────────────────
  // The Inbox view pulls from `mailboxListForRecipient` rather than
  // filtering `$events` so the backend's read/clear semantics flow
  // through correctly: cleared events drop out, unread-only filtering
  // works, and mark-read/ack actions visibly affect the displayed list
  // when paired with the unread-only toggle. Refetched on selection
  // changes and on every `mailboxMutationTick` bump so backend
  // mutations propagate without polling.
  let recipientEvents = $state<MailboxEventPayload[]>([]);
  let recipientReadStates = $state<Map<string, ReadState | null>>(new Map());
  let unreadOnly = $state(false);

  $effect(() => {
    // Touch each dependency so the effect re-runs on any of them.
    void $mailboxMutationTick;
    const alias = selectedAlias;
    const project = selectedProjectId;
    const filter = unreadOnly;
    if (!visible) {
      recipientEvents = [];
      recipientReadStates = new Map();
      return;
    }

    void (async () => {
      try {
        const evs = await mailboxListForRecipient(alias, {
          unreadOnly: filter,
          projectId: project,
          global: project == null,
        });
        // Selection — including unreadOnly + visibility — may have
        // changed during the await. Bail if any input shifted so an
        // older response can't overwrite a newer fetch's results.
        if (
          !visible ||
          selectedAlias !== alias ||
          selectedProjectId !== project ||
          unreadOnly !== filter
        ) {
          return;
        }
        recipientEvents = evs;

        // Fetch read state per event so we can show greyed-out / acked
        // styling. Cheap (in-memory backend); fine for inboxes with
        // dozens of events.
        const states = await Promise.all(
          evs.map((e) =>
            mailboxReadState(e.id, alias).catch(() => null),
          ),
        );
        if (
          !visible ||
          selectedAlias !== alias ||
          selectedProjectId !== project ||
          unreadOnly !== filter
        ) {
          return;
        }
        const map = new Map<string, ReadState | null>();
        evs.forEach((e, i) => map.set(e.id, states[i]));
        recipientReadStates = map;
      } catch (err) {
        console.warn("inbox fetch failed", err);
      }
    })();
  });

  function eventReadState(id: string): ReadState | null {
    return recipientReadStates.get(id) ?? null;
  }

  let firehoseEvents = $derived(visible ? $events : []);

  let sortedAliases = $derived.by((): AgentAlias[] => {
    const list = $aliases.slice();
    list.sort((a, b) => {
      // `me` always first.
      if (a.alias === "me") return -1;
      if (b.alias === "me") return 1;
      const ua = $unreadByAlias.get(aliasKey(a.alias, a.projectId)) ?? 0;
      const ub = $unreadByAlias.get(aliasKey(b.alias, b.projectId)) ?? 0;
      if (ub !== ua) return ub - ua;
      return a.alias.localeCompare(b.alias);
    });
    return list;
  });

  // ── Handlers ────────────────────────────────────────────────────────────────
  function unreadFor(alias: string, projectId: string | null): number {
    return $unreadByAlias.get(aliasKey(alias, projectId)) ?? 0;
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
      // Pass `undefined` so the helper refreshes every known scope for
      // that name (we don't track projectId on the compose form yet).
      void refreshUnreadCount(composeTo, undefined);
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

  async function handleSubscribe(): Promise<void> {
    if (subSubmitting) return;
    subSubmitting = true;
    subError = null;
    try {
      await createSubscription(
        subAlias.trim(),
        subPattern.trim(),
        subProjectId,
      );
      // Reset on success — the store updates from the
      // `subscription-event` listener so the new row appears in the list.
      subPattern = "";
    } catch (err) {
      subError = String(err);
    } finally {
      subSubmitting = false;
    }
  }

  async function handleUnsubscribe(id: string): Promise<void> {
    if (deletingSubId) return;
    deletingSubId = id;
    try {
      await deleteSubscription(id);
    } catch (err) {
      subError = String(err);
    } finally {
      deletingSubId = null;
    }
  }

  // Pre-fill the alias field from the current selection so a user
  // working in the Inbox can switch tabs and see "this alias's
  // subscriptions" without re-typing.
  $effect(() => {
    if (view === "subscriptions" && subAlias === "") {
      subAlias = selectedAlias === "me" ? "" : selectedAlias;
    }
  });

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
   * only enable the Deliver button in that case. Scoped by both alias
   * and projectId, otherwise an alias of the same name in a different
   * project would falsely enable Deliver here.
   */
  function recipientHasPane(
    toAlias: string | null,
    projectId: string | null,
  ): boolean {
    if (!toAlias) return false;
    return $aliases.some(
      (a) =>
        a.alias === toAlias &&
        (a.projectId ?? null) === projectId &&
        a.paneId !== null,
    );
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
        <button
          class="rounded px-2 py-0.5 text-[11px] {view === 'subscriptions'
            ? 'bg-bg-hover text-text-primary'
            : 'text-text-muted hover:text-text-primary'}"
          onclick={() => (view = "subscriptions")}
          title="Bus subscriptions — alias receives matching topic events"
        >Subs</button>
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
        {@const u = unreadFor(a.alias, a.projectId)}
        {@const isSelected =
          selectedAlias === a.alias && selectedProjectId === a.projectId}
        <button
          class="flex shrink-0 items-center gap-1 rounded border px-2 py-1 text-[11px] {isSelected
            ? 'border-accent-dim bg-accent/15 text-text-primary'
            : 'border-border-subtle bg-transparent text-text-muted hover:bg-bg-hover hover:text-text-primary'}"
          onclick={() => {
            selectedAlias = a.alias;
            selectedProjectId = a.projectId;
          }}
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
        >No {unreadOnly ? "unread" : ""} mail for {selectedAlias}</div>
      {:else}
        {#each recipientEvents as e (e.id)}
          {@const state = eventReadState(e.id)}
          {@const isRead = state?.readAt != null}
          {@const isAcked = state?.ackedAt != null}
          <article
            class="mb-2 flex flex-col gap-1 rounded-lg border border-border-subtle bg-bg-surface/30 px-2 py-1.5 {isRead
              ? 'opacity-60'
              : ''}"
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
              {#if isAcked}
                <span class="text-[10px] text-green" title={state?.ackResult ?? "Acked"}
                  >✓ ack{state?.ackResult ? `: ${state.ackResult}` : ""}</span
                >
              {:else if isRead}
                <span class="text-[10px] text-text-muted">read</span>
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
                class="cursor-pointer rounded border border-transparent bg-transparent px-1.5 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
                onclick={() => handleMarkRead(e.id)}
                disabled={isRead}
              >mark read</button>
              <button
                class="cursor-pointer rounded border border-transparent bg-transparent px-1.5 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
                onclick={() => handleAck(e.id)}
                disabled={isAcked}
              >ack</button>
              {#if recipientHasPane(e.to, e.projectId)}
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

    <div class="flex items-center gap-2 border-t border-border-subtle p-1.5">
      <label class="flex items-center gap-1 text-[10px] text-text-muted">
        <input type="checkbox" bind:checked={unreadOnly} class="h-3 w-3" />
        unread only
      </label>
      {#if recipientEvents.length > 0}
        <button
          class="ml-auto cursor-pointer rounded border border-transparent bg-transparent px-2 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
          onclick={handleClearRead}
          title="Hide read mail from this view. Underlying events are preserved for audit."
        >clear read</button>
      {/if}
    </div>
  {:else if view === "firehose"}
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
  {:else}
    <!-- Subscriptions: list + create form. The list updates live via
         the `subscription-event` Tauri channel; mutations here go through
         the same Tauri commands that CLI/MCP use. -->
    <form
      class="flex flex-col gap-1 border-b border-border-subtle bg-bg-surface/30 p-2"
      onsubmit={(e) => {
        e.preventDefault();
        void handleSubscribe();
      }}
    >
      <div class="flex gap-1">
        <input
          class="flex-1 rounded border border-border-subtle bg-bg-deep px-2 py-1 text-[11px] text-text-primary placeholder:text-text-muted/60"
          bind:value={subAlias}
          placeholder="Alias (e.g. auditor)"
          list="mailbox-compose-aliases"
          autocomplete="off"
          required
        />
        <input
          class="flex-[2] rounded border border-border-subtle bg-bg-deep px-2 py-1 text-[11px] text-text-primary placeholder:text-text-muted/60"
          bind:value={subPattern}
          placeholder="Pattern: repo-a.* or **.completed"
          autocomplete="off"
          required
        />
      </div>
      <div class="flex items-center gap-2 text-[10px] text-text-muted/80">
        <span>* matches one segment, ** matches many. Patterns and aliases are lowercase, hyphens allowed.</span>
        <button
          type="submit"
          class="ml-auto cursor-pointer rounded border border-accent-dim bg-accent/20 px-2 py-1 text-[11px] text-text-primary hover:bg-accent/30 disabled:opacity-50"
          disabled={subSubmitting}
        >{subSubmitting ? "Adding…" : "Subscribe"}</button>
      </div>
      {#if subError}
        <div class="rounded border border-red-500/40 bg-red-500/10 px-2 py-1 text-[10px] text-red-300">
          {subError}
        </div>
      {/if}
    </form>

    <div class="flex-1 overflow-y-auto p-2">
      {#if $subscriptions.length === 0}
        <div
          class="flex h-full items-center justify-center text-center text-sm text-text-muted"
        >No subscriptions yet. Add one above to push topic events into
          an alias's mailbox.</div>
      {:else}
        {#each $subscriptions as s (s.id)}
          <article
            class="mb-1 flex items-center gap-2 rounded border border-border-subtle bg-bg-surface/30 px-2 py-1.5"
          >
            <span class="rounded bg-bg-hover px-1.5 py-0.5 text-[10px] text-text-primary"
              >@{s.alias}</span
            >
            <code class="text-[11px] text-text-secondary">{s.pattern}</code>
            {#if s.projectId}
              <span class="text-[9px] text-text-muted">scope: {s.projectId}</span>
            {:else}
              <span class="text-[9px] text-text-muted/70">scope: global</span>
            {/if}
            <span class="ml-auto shrink-0 text-[10px] text-text-muted/70"
              >{formatRelative(s.createdAt)}</span
            >
            <button
              class="cursor-pointer rounded border border-transparent bg-transparent px-1.5 py-0.5 text-[10px] text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary disabled:opacity-50"
              onclick={() => handleUnsubscribe(s.id)}
              disabled={deletingSubId === s.id}
              title="Remove this subscription"
            >{deletingSubId === s.id ? "…" : "remove"}</button>
          </article>
        {/each}
      {/if}
    </div>
  {/if}
</div>
