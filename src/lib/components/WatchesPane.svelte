<script lang="ts">
  import { watchState, removeWatchFromStore } from "$lib/stores/watches";
  import { sessionState } from "$lib/stores/sessions";
  import { removeWatch, pauseWatch, resumeWatch } from "$lib/tauri";
  import type { Watch, WatchOutcome } from "$lib/types";
  import { openUrl } from "@tauri-apps/plugin-opener";

  import PinButton from "./PinButton.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();
  let expandedId = $state<string | null>(null);

  let grouped = $derived.by(() => {
    const groups: { label: string; key: string; watches: Watch[] }[] = [];
    const globalWatches = $watchState.filter((w) => w.scope.type === "global");
    if (globalWatches.length > 0) {
      groups.push({ label: "Global", key: "global", watches: globalWatches });
    }
    const sessionMap = new Map<string, Watch[]>();
    const projectMap = new Map<string, Watch[]>();
    for (const w of $watchState) {
      if (w.scope.type === "session") {
        const list = sessionMap.get(w.scope.sessionId) ?? [];
        list.push(w);
        sessionMap.set(w.scope.sessionId, list);
      } else if (w.scope.type === "project") {
        const list = projectMap.get(w.scope.projectId) ?? [];
        list.push(w);
        projectMap.set(w.scope.projectId, list);
      }
    }
    for (const [id, watches] of sessionMap) {
      const session = $sessionState.sessions.find((s) => s.id === id);
      const label = session ? session.name : `Session: ${id.slice(0, 8)}`;
      groups.push({ label, key: `s-${id}`, watches });
    }
    for (const [id, watches] of projectMap) {
      groups.push({ label: `Project: ${id.slice(0, 8)}`, key: `p-${id}`, watches });
    }
    return groups;
  });

  function outcomeColor(outcome: WatchOutcome | null): string {
    switch (outcome) {
      case "success": return "bg-green";
      case "failure": return "bg-red";
      case "inProgress": return "bg-amber";
      default: return "bg-gray";
    }
  }

  function outcomeShadow(outcome: WatchOutcome | null): string {
    switch (outcome) {
      case "success": return "shadow-[0_0_6px_var(--color-green-dim)]";
      case "failure": return "shadow-[0_0_6px_var(--color-red-dim)]";
      case "inProgress": return "shadow-[0_0_6px_var(--color-amber-dim)]";
      default: return "";
    }
  }

  function formatTime(ts: number | null): string {
    if (!ts) return "never";
    const secs = Math.floor((Date.now() - ts) / 1000);
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
    return `${Math.floor(secs / 3600)}h ago`;
  }

  async function handleRemove(id: string) {
    await removeWatch(id);
    removeWatchFromStore(id);
  }

  async function handlePause(id: string) {
    await pauseWatch(id);
  }

  async function handleResume(id: string) {
    await resumeWatch(id);
  }

  function toggleExpand(id: string) {
    expandedId = expandedId === id ? null : id;
  }

  function stateLabel(watch: Watch): string {
    const outcome = watch.lastResult?.outcome;
    if (outcome === "success") return "succeeded";
    if (outcome === "failure") return "failed";
    if (outcome === "inProgress") return "in progress";
    return watch.runtimeState.type;
  }
</script>

<div
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <div class="flex h-9 shrink-0 items-center justify-between border-b border-hairline bg-bg-surface/30 px-3">
    <span class="text-sm font-semibold tracking-tight">Watches</span>
    <div class="flex items-center gap-1">
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <CollapseSidebarButton
        onclick={onclose}
        label="Collapse watches sidebar"
        title="Collapse watches sidebar"
      />
    </div>
  </div>

  <div class="flex-1 overflow-y-auto p-2">
    {#if $watchState.length === 0}
      <div class="flex h-full items-center justify-center text-sm text-text-muted">
        No watches configured
      </div>
    {:else}
      {#each grouped as group (group.key)}
        <div class="mb-3">
          <div class="mb-1 px-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">
            {group.label}
          </div>
          {#each group.watches as watch (watch.id)}
            {@const outcome = watch.lastResult?.outcome ?? null}
            <button
              class="mb-1 flex w-full cursor-pointer items-center gap-2 rounded-lg border border-transparent bg-transparent px-2 py-1.5 text-left text-sm hover:border-border-subtle hover:bg-bg-hover"
              onclick={() => toggleExpand(watch.id)}
            >
              <span
                class="inline-block h-2 w-2 shrink-0 rounded-full {outcomeColor(outcome)} {outcomeShadow(outcome)}"
                class:animate-pulse={outcome === "inProgress"}
              ></span>
              <span class="min-w-0 flex-1 truncate text-text-primary">{watch.name}</span>
              <span class="shrink-0 text-[10px] text-text-muted">{formatTime(watch.lastChecked)}</span>
            </button>

            {#if expandedId === watch.id}
              <div class="mb-2 ml-4 rounded-lg border border-hairline bg-bg-surface/20 p-2 text-xs">
                <div class="mb-1 text-text-muted">
                  State: <span class="text-text-primary">{stateLabel(watch)}</span>
                </div>

                {#if watch.lastResult?.type === "githubRun"}
                  {@const ghResult = watch.lastResult}
                  <div class="mb-1">
                    <span class="text-text-muted">Run:</span>
                    <button
                      class="cursor-pointer border-none bg-transparent p-0 text-blue hover:underline"
                      onclick={(e) => { e.stopPropagation(); openUrl(ghResult.url); }}
                    >
                      #{ghResult.runId}
                    </button>
                    <span class="ml-1 text-text-muted">
                      {ghResult.status}
                      {#if ghResult.conclusion}({ghResult.conclusion}){/if}
                    </span>
                  </div>
                  {#each ghResult.jobs as job}
                    <div class="flex items-center gap-1 py-0.5 pl-2">
                      <span
                        class="inline-block h-1.5 w-1.5 rounded-full {outcomeColor(
                          job.conclusion === 'success' ? 'success' : job.conclusion === 'failure' ? 'failure' : 'inProgress'
                        )}"
                      ></span>
                      <span class="text-text-primary">{job.name}</span>
                      {#if job.failedStep}
                        <span class="text-red">— {job.failedStep}</span>
                      {/if}
                    </div>
                  {/each}
                {:else if watch.lastResult?.type === "httpCheck"}
                  <div class="text-text-muted">
                    Status: <span class="text-text-primary">{watch.lastResult.statusCode}</span>
                    · {watch.lastResult.responseTimeMs}ms
                  </div>
                {:else if watch.lastResult?.type === "githubPr"}
                  {@const prResult = watch.lastResult}
                  <div class="mb-1">
                    <button
                      class="cursor-pointer border-none bg-transparent p-0 text-blue hover:underline"
                      onclick={(e) => { e.stopPropagation(); openUrl(prResult.url); }}
                    >
                      #{prResult.prNumber}
                    </button>
                    <span class="ml-1 rounded px-1 text-[10px] font-medium {prResult.state === 'merged' ? 'bg-green/20 text-green' : prResult.state === 'open' ? 'bg-purple/20 text-purple' : prResult.state === 'closed' ? 'bg-red/20 text-red' : ''}"
                    >{prResult.state}{prResult.draft ? " (draft)" : ""}</span>
                    <span class="ml-1 truncate text-text-muted">{prResult.title}</span>
                  </div>

                  {#if prResult.reviews.length > 0}
                    <div class="mt-1 text-[10px] uppercase tracking-wider text-text-muted">Reviews</div>
                    {#each prResult.reviews as review}
                      <div class="flex items-center gap-1 py-0.5 pl-2">
                        <span
                          class="inline-block h-1.5 w-1.5 rounded-full {outcomeColor(
                            review.state === 'approved' ? 'success' : review.state === 'changes_requested' ? 'failure' : 'inProgress'
                          )}"
                        ></span>
                        {#if review.url}
                          <button
                            class="cursor-pointer border-none bg-transparent p-0 text-text-primary hover:text-blue hover:underline"
                            onclick={(e) => { e.stopPropagation(); openUrl(review.url!); }}
                          >{review.reviewer}</button>
                        {:else}
                          <span class="text-text-primary">{review.reviewer}</span>
                        {/if}
                        <span class="text-text-muted">— {review.state.replace('_', ' ')}</span>
                      </div>
                    {/each}
                  {/if}

                  {#if prResult.checks.length > 0}
                    <div class="mt-1 text-[10px] uppercase tracking-wider text-text-muted">Checks</div>
                    {#each prResult.checks as check}
                      <div class="flex items-center gap-1 py-0.5 pl-2">
                        <span
                          class="inline-block h-1.5 w-1.5 rounded-full {outcomeColor(
                            check.conclusion === 'success' ? 'success' : check.conclusion === 'failure' ? 'failure' : 'inProgress'
                          )}"
                        ></span>
                        {#if check.url}
                          <button
                            class="cursor-pointer border-none bg-transparent p-0 text-text-primary hover:text-blue hover:underline"
                            onclick={(e) => { e.stopPropagation(); openUrl(check.url!); }}
                          >{check.name}</button>
                        {:else}
                          <span class="text-text-primary">{check.name}</span>
                        {/if}
                      </div>
                    {/each}
                  {/if}
                {:else if watch.lastResult?.type === "commandRun"}
                  <div class="text-text-muted">
                    Exit: <span class="text-text-primary">{watch.lastResult.exitCode}</span>
                  </div>
                  {#if watch.lastResult.stdout}
                    <pre class="mt-1 max-h-24 overflow-auto rounded bg-bg-deep p-1 text-[10px] text-text-muted">{watch.lastResult.stdout}</pre>
                  {/if}
                  {#if watch.lastResult.stderr}
                    <pre class="mt-1 max-h-24 overflow-auto rounded bg-bg-deep p-1 text-[10px] text-red">{watch.lastResult.stderr}</pre>
                  {/if}
                {/if}

                <div class="mt-2 flex gap-2">
                  {#if watch.runtimeState.type === "active"}
                    <button
                      class="rounded border border-hairline bg-transparent px-2 py-0.5 text-[10px] text-text-muted hover:bg-bg-hover hover:text-text-primary"
                      onclick={(e) => { e.stopPropagation(); handlePause(watch.id); }}
                    >Pause</button>
                  {:else if watch.runtimeState.type === "paused"}
                    <button
                      class="rounded border border-hairline bg-transparent px-2 py-0.5 text-[10px] text-text-muted hover:bg-bg-hover hover:text-text-primary"
                      onclick={(e) => { e.stopPropagation(); handleResume(watch.id); }}
                    >Resume</button>
                  {/if}
                  <button
                    class="rounded border border-hairline bg-transparent px-2 py-0.5 text-[10px] text-red hover:bg-red/10"
                    onclick={(e) => { e.stopPropagation(); handleRemove(watch.id); }}
                  >Remove</button>
                </div>
              </div>
            {/if}
          {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>
