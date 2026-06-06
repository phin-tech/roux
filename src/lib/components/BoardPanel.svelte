<script lang="ts">
  import { derived, get } from "svelte/store";
  import {
    itemsByColumn,
    WORK_ITEM_COLUMNS,
    COLUMN_LABELS,
    startWorkItem,
    stopWorkItemRun,
    planWorkItem,
    acceptWorkItemReview,
    requestWorkItemChanges,
    pendingDecisionByItem,
    activePlanningRunByItem,
    attachmentsByWorkItem,
    runsByItem,
    workItemRunEvents,
    type WorkItemStatus,
    type WorkItemRun,
  } from "$lib/stores/workItems";
  import { sessionList } from "$lib/stores/sessions";
  import type { SessionStatus } from "$lib/types";
  import PanelTopOpen from "@lucide/svelte/icons/panel-top-open";
  import { openMainView } from "$lib/stores/mainView";
  import {
    openNewWorkItemEditor,
    openWorkItemEditor,
    openWorkItemSessionStart,
  } from "$lib/stores/ui";
  import { openSessionById } from "$lib/panes/openSession";
  import { formatWorkItemStartError } from "$lib/board/startErrors";
  import {
    deleteWorkItemWithMode,
    type WorkItemDeleteMode,
  } from "$lib/workItems/deleteFlow";
  import {
    canStartImplementationFromPlanning,
    hasAttachedPlan,
  } from "$lib/workItems/planningGate";
  import { workItemPhase } from "$lib/workItems/phase";
  import {
    buildWorkItemReviewPackage,
    type WorkItemReviewPackage,
  } from "$lib/workItems/reviewPackage";
  import { resolveReviewAgentRepoRoot } from "$lib/workItems/reviewAgent";
  import type { WorkItem } from "$lib/bindings";
  import { createSessionShell, openPathInFinder } from "$lib/tauri";
  import { addSession, setActiveSession } from "$lib/stores/sessions";
  import { projects } from "$lib/stores/projects";
  import { settings } from "$lib/stores/settings";
  import { defaultAgentProfileId } from "$lib/panes/defaultAgent";
  import { reviewAgentProfileId } from "$lib/workItems/workflow";
  import SidebarPanelHeader from "./SidebarPanelHeader.svelte";
  import CollapseSidebarButton from "./CollapseSidebarButton.svelte";
  import PinButton from "./PinButton.svelte";
  import WorkItemCard from "./WorkItemCard.svelte";
  import AddCardInput from "./AddCardInput.svelte";
  import WorkItemDeleteDialog from "./WorkItemDeleteDialog.svelte";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();
  let startingItemIds = $state<Record<string, boolean>>({});
  let planningItemIds = $state<Record<string, boolean>>({});
  let acceptingItemIds = $state<Record<string, boolean>>({});
  let requestingChangesItemIds = $state<Record<string, boolean>>({});
  let openingAgentItemIds = $state<Record<string, boolean>>({});
  let startErrors = $state<Record<string, string>>({});
  let planErrors = $state<Record<string, string>>({});
  let deleteTarget = $state<WorkItem | null>(null);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);

  interface WorkItemStartActionOptions {
    forceStart?: boolean;
    fixCi?: boolean;
  }

  const sessionStatusMap = derived(sessionList, ($sessions) => {
    const m = new Map<string, SessionStatus>();
    for (const s of $sessions) m.set(s.id, s.status);
    return m;
  });

  function withoutKey<T>(
    record: Record<string, T>,
    key: string,
  ): Record<string, T> {
    const { [key]: _removed, ...rest } = record;
    return rest;
  }

  function needsStartConfig(item: WorkItem): boolean {
    return !item.repoPath && !item.projectId;
  }

  function attachedSessionIdsForItem(
    item: WorkItem,
    runs: WorkItemRun[],
    planningSessionId: string | null,
  ): string[] {
    const ids = new Set<string>();
    if (item.sessionId) ids.add(item.sessionId);
    if (planningSessionId) ids.add(planningSessionId);
    for (const run of runs) {
      if (run.sessionId) ids.add(run.sessionId);
    }
    return [...ids];
  }

  async function handleStart(
    id: string,
    item: WorkItem,
    options: WorkItemStartActionOptions = {},
  ) {
    const forceStart = !!options.forceStart;
    const fixCi = !!options.fixCi;
    const planningRun = get(activePlanningRunByItem).get(id);
    const attachments = get(attachmentsByWorkItem).get(id) ?? [];
    if (
      item.status === "ready" &&
      !canStartImplementationFromPlanning(attachments, forceStart)
    ) {
      if (planningRun?.sessionId) await handleOpen(planningRun.sessionId);
      else {
        startErrors = {
          ...startErrors,
          [id]: "Attach a plan before starting implementation.",
        };
      }
      return;
    }
    if (needsStartConfig(item)) {
      openWorkItemSessionStart({
        itemId: item.id,
        title: item.title,
        ...(forceStart ? { forceStart: true } : {}),
        ...(fixCi ? { fixCi: true } : {}),
      });
      return;
    }
    if (startingItemIds[id]) return;
    startingItemIds = { ...startingItemIds, [id]: true };
    startErrors = withoutKey(startErrors, id);

    // Start creates the session/worktree and moves the card after prompt dispatch.
    try {
      if (item.status === "ready" && planningRun) {
        await stopWorkItemRun(planningRun.id);
      }
      if (forceStart || fixCi) {
        await startWorkItem(id, {
          ...(forceStart ? { forceStart: true } : {}),
          ...(fixCi ? { fixCi: true } : {}),
        });
      } else await startWorkItem(id);
    } catch (err) {
      startErrors = { ...startErrors, [id]: formatWorkItemStartError(err) };
      console.error("Failed to start work item", err);
    } finally {
      startingItemIds = withoutKey(startingItemIds, id);
    }
  }

  function formatPlanError(err: unknown): string {
    const message = err instanceof Error ? err.message : String(err);
    return message ? `Plan failed: ${message}` : "Plan failed.";
  }

  async function handlePlan(
    id: string,
    _item: WorkItem,
    replaceActive = false,
  ) {
    if (planningItemIds[id]) return;
    planningItemIds = { ...planningItemIds, [id]: true };
    planErrors = withoutKey(planErrors, id);
    try {
      const sessionId = replaceActive
        ? await planWorkItem(id, { replaceActive: true })
        : await planWorkItem(id);
      await handleOpen(sessionId);
    } catch (err) {
      planErrors = { ...planErrors, [id]: formatPlanError(err) };
      console.error("Failed to plan work item", err);
    } finally {
      planningItemIds = withoutKey(planningItemIds, id);
    }
  }

  async function handleAcceptReview(id: string, _item?: WorkItem) {
    if (acceptingItemIds[id]) return;
    acceptingItemIds = { ...acceptingItemIds, [id]: true };
    startErrors = withoutKey(startErrors, id);
    try {
      await acceptWorkItemReview(id);
    } catch (err) {
      startErrors = { ...startErrors, [id]: "Failed to accept review." };
      console.error("Failed to accept work item review", err);
    } finally {
      acceptingItemIds = withoutKey(acceptingItemIds, id);
    }
  }

  async function handleRequestChanges(
    id: string,
    _item: WorkItem,
    note: string,
  ) {
    if (requestingChangesItemIds[id]) return;
    requestingChangesItemIds = { ...requestingChangesItemIds, [id]: true };
    startErrors = withoutKey(startErrors, id);
    try {
      await requestWorkItemChanges(id, note);
    } catch (err) {
      startErrors = { ...startErrors, [id]: "Failed to request changes." };
      console.error("Failed to request work item changes", err);
      throw err;
    } finally {
      requestingChangesItemIds = withoutKey(requestingChangesItemIds, id);
    }
  }

  async function handleOpenWorktree(path: string) {
    try {
      await openPathInFinder(path);
    } catch (err) {
      console.error("Failed to open work item worktree", err);
    }
  }

  async function handleOpenAgent(
    item: WorkItem,
    reviewPackage: WorkItemReviewPackage,
  ) {
    const worktreePath = reviewPackage.worktreePath;
    if (!worktreePath || openingAgentItemIds[item.id]) return;
    openingAgentItemIds = { ...openingAgentItemIds, [item.id]: true };
    startErrors = withoutKey(startErrors, item.id);
    try {
      const projectRepoRoots = item.projectId
        ? (get(projects).find((project) => project.id === item.projectId)
            ?.repoRoots ?? [])
        : [];
      const repoPath = resolveReviewAgentRepoRoot({
        itemRepoPath: item.repoPath,
        projectRepoRoots,
        worktreePath,
      });
      if (!repoPath) {
        throw new Error("review worktree repo root is not configured");
      }
      const profileId =
        reviewAgentProfileId(get(settings).kanban, item.reviewStageId) ??
        item.agentProfile ??
        defaultAgentProfileId();
      const profileRef = { kind: "registered" as const, id: profileId };
      const [
        { resolveProfileRef },
        { runProfileInPane },
        { initSessionWithProfile },
        { connectPaneTerminal },
      ] = await Promise.all([
        import("$lib/panes/profiles"),
        import("$lib/panes/profileRunner"),
        import("$lib/panes/actions"),
        import("$lib/panes/terminals"),
      ]);
      const session = await createSessionShell(
        repoPath,
        `${item.title} review`,
        worktreePath,
        null,
        { profile: profileId },
      );
      addSession(session);
      const mainPaneId = initSessionWithProfile(session.id, profileRef);
      await connectPaneTerminal(mainPaneId);
      const profile = resolveProfileRef(profileRef);
      if (profile) await runProfileInPane(session.id, profile, {});
      setActiveSession(session.id);
    } catch (err) {
      startErrors = { ...startErrors, [item.id]: "Failed to open agent." };
      console.error("Failed to open review agent", err);
    } finally {
      openingAgentItemIds = withoutKey(openingAgentItemIds, item.id);
    }
  }

  async function handleOpen(sessionId: string) {
    const result = await openSessionById(sessionId);
    if (result === "gone") {
      console.error(`Session ${sessionId} is no longer running`);
    }
  }

  function handleDelete(_id: string, item: WorkItem) {
    deleteTarget = item;
    deleteError = null;
  }

  async function confirmDelete(mode: WorkItemDeleteMode) {
    if (!deleteTarget) return;
    deleting = true;
    deleteError = null;
    try {
      await deleteWorkItemWithMode(deleteTarget, mode);
      deleteTarget = null;
    } catch (err) {
      deleteError = "Failed to delete card.";
      console.error("Failed to delete work item", err);
    } finally {
      deleting = false;
    }
  }

  function handleCreate(status: WorkItemStatus) {
    openNewWorkItemEditor({ status });
  }
</script>

<div
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <SidebarPanelHeader title="Board">
    {#snippet actions()}
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-muted transition-colors hover:bg-surface-2 hover:text-text"
        onclick={() => openMainView({ kind: "board" })}
        aria-label="Open board in main view"
        title="Open board in main view"
      >
        <PanelTopOpen size={14} />
      </button>
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <CollapseSidebarButton
        onclick={onclose}
        label="Collapse board sidebar"
        title="Collapse board sidebar"
      />
    {/snippet}
  </SidebarPanelHeader>

  <div class="flex flex-1 min-h-0 flex-col overflow-y-auto px-2 py-2 gap-4">
    {#each WORK_ITEM_COLUMNS as col (col)}
      {@const items = $itemsByColumn.get(col) ?? []}
      <section>
        <div class="mb-1.5 flex items-center gap-1.5 px-1">
          <span
            class="text-[11px] font-semibold uppercase tracking-wide text-text-muted"
          >
            {COLUMN_LABELS[col]}
          </span>
          {#if items.length > 0}
            <span
              class="rounded-full bg-surface-2 px-1.5 py-0.5 text-[9px] font-medium text-text-muted"
            >
              {items.length}
            </span>
          {/if}
        </div>
        {#if items.length > 0}
          <div class="flex flex-col gap-1.5">
            {#each items as item (item.id)}
              {@const sessionStatus = item.sessionId
                ? ($sessionStatusMap.get(item.sessionId) ?? null)
                : null}
              {@const pendingDecision =
                $pendingDecisionByItem.get(item.id) ?? null}
              {@const planningRun =
                $activePlanningRunByItem.get(item.id) ?? null}
              {@const itemRuns = $runsByItem.get(item.id) ?? []}
              {@const itemAttachments =
                $attachmentsByWorkItem.get(item.id) ?? []}
              {@const attachedSessionIds = attachedSessionIdsForItem(
                item,
                itemRuns,
                planningRun?.sessionId ?? null,
              )}
              {@const phase = workItemPhase({
                status: item.status,
                sessionId: item.sessionId,
                activePlanningRun: planningRun,
                hasAttachedPlan: hasAttachedPlan(itemAttachments),
                pendingDecision,
                isStartable: !needsStartConfig(item),
              })}
              {@const reviewPackage = buildWorkItemReviewPackage(
                item,
                itemRuns,
                itemAttachments,
                $workItemRunEvents,
              )}
              <WorkItemCard
                {item}
                {sessionStatus}
                {phase}
                {reviewPackage}
                {attachedSessionIds}
                onStart={handleStart}
                onPlan={handlePlan}
                onOpen={handleOpen}
                onEdit={openWorkItemEditor}
                onDelete={handleDelete}
                onAcceptReview={handleAcceptReview}
                onRequestChanges={handleRequestChanges}
                onOpenWorktree={handleOpenWorktree}
                onOpenAgent={handleOpenAgent}
                startPending={!!startingItemIds[item.id]}
                planPending={!!planningItemIds[item.id]}
                acceptPending={!!acceptingItemIds[item.id]}
                requestChangesPending={!!requestingChangesItemIds[item.id]}
                openAgentPending={!!openingAgentItemIds[item.id]}
                startError={startErrors[item.id] ??
                  planErrors[item.id] ??
                  item.startError ??
                  null}
              />
            {/each}
          </div>
        {:else}
          <p class="px-1 text-xs text-text-muted/50">Empty</p>
        {/if}
        {#if col === "todo"}
          <div class="mt-1.5">
            <AddCardInput onCreate={() => handleCreate(col)} />
          </div>
        {/if}
      </section>
    {/each}
  </div>
</div>

<WorkItemDeleteDialog
  item={deleteTarget}
  {deleting}
  error={deleteError}
  onCancel={() => {
    if (!deleting) deleteTarget = null;
  }}
  onConfirm={confirmDelete}
/>
