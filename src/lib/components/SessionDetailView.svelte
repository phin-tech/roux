<script lang="ts">
  import Pencil from "@lucide/svelte/icons/pencil";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Terminal from "@lucide/svelte/icons/terminal";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import {
    renameSession,
    sessionDisplayName,
    sessionList,
    setActiveSession,
  } from "$lib/stores/sessions";
  import { projects } from "$lib/stores/projects";
  import { closeMainView } from "$lib/stores/mainView";
  import { continueSession } from "$lib/sessions/reconnect";
  import { collectLeafIds, sessionLayouts } from "$lib/panes/layout";
  import { paneInstances, type PaneInstance } from "$lib/panes/instances";
  import { getDocument, listDocuments } from "$lib/stores/workItems";
  import type { Attachment, AttachmentDocument } from "$lib/types/workItems";
  import type { Session } from "$lib/types";

  interface Props {
    sessionId: string;
  }

  let { sessionId }: Props = $props();

  let editingName = $state(false);
  let nameDraft = $state("");
  let documents = $state<Attachment[]>([]);
  let selectedDocument = $state<AttachmentDocument | null>(null);
  let documentsLoading = $state(false);
  let documentLoadingId = $state<string | null>(null);
  let documentError = $state<string | null>(null);
  let reconnecting = $state(false);
  let refreshGeneration = 0;
  let documentOpenGeneration = 0;

  let session = $derived($sessionList.find((s) => s.id === sessionId) ?? null);
  let displayName = $derived(session ? sessionDisplayName(session) : "");
  let project = $derived(
    session?.projectId ? ($projects.find((p) => p.id === session.projectId) ?? null) : null,
  );
  let paneRows = $derived.by(() => {
    const layout = $sessionLayouts.get(sessionId);
    if (!layout) return [];
    return collectLeafIds(layout).map((paneId) => ({
      paneId,
      pane: $paneInstances.get(paneId) ?? null,
    }));
  });

  $effect(() => {
    if (!editingName) nameDraft = displayName;
  });

  $effect(() => {
    const currentSession = session;
    const targetId = sessionId;
    selectedDocument = null;
    if (!currentSession) {
      documents = [];
      return;
    }
    void refreshDocuments(targetId);
  });

  function metadataRows(s: Session): Array<[string, string | null]> {
    return [
      ["Status", s.status],
      ["Project", project?.name ?? s.projectId ?? null],
      ["Repository", s.repoRoot],
      ["Worktree", s.worktreePath],
      ["Branch", s.branch || null],
      ["Model", s.model],
      ["Cost", s.cost == null ? null : `$${s.cost.toFixed(2)}`],
      ["Created", formatEpochSeconds(s.createdAt)],
      ["Primary PTY", s.primaryPtyId ?? null],
      ["Pinned PR", s.pinnedPrUrl ?? null],
    ];
  }

  async function refreshDocuments(targetId = sessionId): Promise<void> {
    const generation = ++refreshGeneration;
    documentsLoading = true;
    documentError = null;
    try {
      const next = await listDocuments("session", targetId);
      if (generation !== refreshGeneration || targetId !== sessionId) return;
      documents = next;
    } catch (err) {
      if (generation !== refreshGeneration || targetId !== sessionId) return;
      documentError = formatError(err, "Failed to load attachments.");
      documents = [];
    } finally {
      if (generation === refreshGeneration && targetId === sessionId) {
        documentsLoading = false;
      }
    }
  }

  async function openDocument(attachment: Attachment): Promise<void> {
    const generation = ++documentOpenGeneration;
    documentLoadingId = attachment.id;
    documentError = null;
    try {
      const next = await getDocument(attachment.documentId);
      if (generation !== documentOpenGeneration) return;
      selectedDocument = next;
    } catch (err) {
      if (generation !== documentOpenGeneration) return;
      documentError = formatError(err, "Failed to read attachment.");
    } finally {
      if (generation === documentOpenGeneration) {
        documentLoadingId = null;
      }
    }
  }

  function startRename(): void {
    nameDraft = displayName;
    editingName = true;
  }

  function cancelRename(): void {
    nameDraft = displayName;
    editingName = false;
  }

  function commitRename(): void {
    if (!session) return;
    const trimmed = nameDraft.trim();
    editingName = false;
    if (!trimmed || trimmed === displayName) return;
    renameSession(session.id, trimmed);
  }

  function openTerminal(): void {
    if (!session) return;
    setActiveSession(session.id);
    closeMainView();
  }

  async function handleContinue(): Promise<void> {
    if (!session || reconnecting) return;
    reconnecting = true;
    try {
      await continueSession(session);
    } finally {
      reconnecting = false;
    }
  }

  function paneName(paneId: string, pane: PaneInstance | null): string {
    return pane?.name?.trim() || pane?.docPath?.split(/[\\/]+/).pop() || paneId;
  }

  function profileLabel(pane: PaneInstance | null): string {
    const ref = pane?.spawnProfileRef;
    if (!ref) return "plain";
    return ref.kind === "registered" ? ref.id : (ref.profile.name || "custom");
  }

  function formatEpochSeconds(epoch: number | null | undefined): string | null {
    if (!epoch) return null;
    return new Date(epoch * 1000).toLocaleString();
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function formatError(err: unknown, fallback: string): string {
    return err instanceof Error && err.message ? err.message : fallback;
  }
</script>

{#if !session}
  <div class="flex h-full items-center justify-center text-sm text-text-muted">
    Session no longer available
  </div>
{:else}
  <div class="app-scrollbar h-full overflow-y-auto bg-bg-deep">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-5 p-5">
      <section class="border-b border-hairline pb-4">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div class="min-w-0 flex-1">
            {#if editingName}
              <input
                class="h-9 max-w-xl border border-accent-dim/40 bg-bg-base px-2.5 text-base font-semibold text-text-primary outline-none focus:border-accent"
                bind:value={nameDraft}
                aria-label="Session name"
                onblur={commitRename}
                onkeydown={(e) => {
                  if (e.key === "Enter") {
                    e.stopPropagation();
                    commitRename();
                  }
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    cancelRename();
                  }
                }}
              />
            {:else}
              <div class="flex min-w-0 items-center gap-2">
                <h2 class="truncate text-base font-semibold text-text-primary">{displayName}</h2>
                <button
                  type="button"
                  class="flex h-7 w-7 items-center justify-center rounded text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
                  aria-label="Rename session"
                  title="Rename session"
                  onclick={startRename}
                >
                  <Pencil size={13} />
                </button>
              </div>
            {/if}
            <div class="mt-1 truncate font-mono text-[11px] text-text-muted">{session.id}</div>
          </div>
          <div class="flex shrink-0 items-center gap-2">
            {#if session.status === "disconnected"}
              <button
                type="button"
                class="inline-flex h-8 items-center gap-1.5 rounded border border-accent-dim/30 bg-accent-dim/15 px-3 text-xs font-semibold text-accent transition-colors hover:bg-accent-dim/24 disabled:cursor-wait disabled:opacity-60"
                onclick={handleContinue}
                disabled={reconnecting}
                aria-label="Continue session"
              >
                <RotateCcw size={13} />
                <span>{reconnecting ? "Continuing..." : "Continue session"}</span>
              </button>
            {/if}
            <button
              type="button"
              class="inline-flex h-8 items-center gap-1.5 rounded border border-accent-dim/30 bg-accent-dim/15 px-3 text-xs font-semibold text-accent transition-colors hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              onclick={openTerminal}
              aria-label="Open terminal"
            >
              <Terminal size={13} />
              <span>Open terminal</span>
            </button>
          </div>
        </div>
      </section>

      <div class="grid gap-5 xl:grid-cols-[minmax(0,1.1fr)_minmax(320px,0.9fr)]">
        <div class="flex min-w-0 flex-col gap-5">
          <section>
            <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-text-muted">Session</h3>
            <dl class="grid gap-px overflow-hidden rounded border border-border-subtle bg-border-subtle sm:grid-cols-[160px_minmax(0,1fr)]">
              {#each metadataRows(session) as [label, value] (label)}
                <dt class="bg-bg-surface px-3 py-2 text-xs text-text-muted">{label}</dt>
                <dd class="min-w-0 bg-bg-base px-3 py-2 font-mono text-xs text-text-primary">
                  {#if value}
                    <span class="break-all">{value}</span>
                  {:else}
                    <span class="text-text-muted">-</span>
                  {/if}
                </dd>
              {/each}
            </dl>
          </section>

          <section>
            <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-text-muted">Panes</h3>
            {#if paneRows.length === 0}
              <div class="rounded border border-border-subtle bg-bg-base px-3 py-3 text-xs text-text-muted">
                No pane layout is registered for this session.
              </div>
            {:else}
              <div class="overflow-hidden rounded border border-border-subtle">
                {#each paneRows as row (row.paneId)}
                  <div class="grid gap-px border-b border-border-subtle bg-border-subtle last:border-b-0 sm:grid-cols-[minmax(0,1fr)_100px_120px_160px]">
                    <div class="min-w-0 bg-bg-base px-3 py-2">
                      <div class="truncate text-xs font-medium text-text-primary">{paneName(row.paneId, row.pane)}</div>
                      <div class="truncate font-mono text-[10px] text-text-muted">{row.paneId}</div>
                    </div>
                    <div class="bg-bg-base px-3 py-2 text-xs text-text-secondary">{row.pane?.type ?? "missing"}</div>
                    <div class="bg-bg-base px-3 py-2 font-mono text-xs text-text-secondary">{profileLabel(row.pane)}</div>
                    <div class="min-w-0 bg-bg-base px-3 py-2 font-mono text-xs text-text-secondary">
                      <span class="block truncate">{row.pane?.ptyId || "-"}</span>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          </section>
        </div>

        <section class="min-w-0">
          <div class="mb-2 flex items-center justify-between gap-2">
            <h3 class="text-xs font-semibold uppercase tracking-wide text-text-muted">Attachments</h3>
            <button
              type="button"
              class="flex h-7 w-7 items-center justify-center rounded text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              aria-label="Refresh attachments"
              title="Refresh attachments"
              onclick={() => refreshDocuments()}
            >
              <RefreshCw size={13} />
            </button>
          </div>
          <div class="grid min-h-[360px] overflow-hidden rounded border border-border-subtle bg-bg-base md:grid-cols-[220px_minmax(0,1fr)] xl:grid-cols-1 2xl:grid-cols-[220px_minmax(0,1fr)]">
            <div class="min-h-0 border-b border-border-subtle md:border-b-0 md:border-r xl:border-b xl:border-r-0 2xl:border-b-0 2xl:border-r">
              {#if documentsLoading}
                <div class="px-3 py-3 text-xs text-text-muted">Loading attachments...</div>
              {:else if documents.length === 0}
                <div class="px-3 py-3 text-xs text-text-muted">No attachments.</div>
              {:else}
                <div class="app-scrollbar max-h-72 overflow-y-auto">
                  {#each documents as attachment (attachment.id)}
                    <button
                      type="button"
                      class="flex w-full flex-col items-start gap-1 border-b border-border-subtle bg-transparent px-3 py-2 text-left last:border-b-0 hover:bg-bg-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent-dim/50"
                      class:bg-bg-active={selectedDocument?.attachment.id === attachment.id}
                      onclick={() => openDocument(attachment)}
                      aria-label={attachment.title || attachment.documentId}
                    >
                      <span class="line-clamp-2 text-xs font-medium text-text-primary">
                        {attachment.title || attachment.documentId}
                      </span>
                      <span class="font-mono text-[10px] text-text-muted">
                        {formatBytes(attachment.byteLen)}
                      </span>
                    </button>
                  {/each}
                </div>
              {/if}
            </div>

            <div class="app-scrollbar min-h-0 overflow-y-auto p-3">
              {#if documentError}
                <div class="rounded border border-red/30 bg-red/10 px-3 py-2 text-xs text-red" role="alert">{documentError}</div>
              {:else if documentLoadingId}
                <div class="text-xs text-text-muted">Loading attachment...</div>
              {:else if selectedDocument}
                <div class="mb-2 text-xs font-semibold text-text-primary">
                  {selectedDocument.attachment.title || selectedDocument.attachment.documentId}
                </div>
                <pre class="max-h-[520px] whitespace-pre-wrap break-words rounded border border-border-subtle bg-bg-deep p-3 font-mono text-xs leading-5 text-text-primary">{selectedDocument.content}</pre>
              {:else}
                <div class="flex h-full min-h-44 items-center justify-center text-center text-xs text-text-muted">
                  Select an attachment to read it.
                </div>
              {/if}
            </div>
          </div>
        </section>
      </div>
    </div>
  </div>
{/if}
