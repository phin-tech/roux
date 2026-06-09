<script lang="ts">
  import { derived, get } from "svelte/store";
  import {
    itemsByColumn,
    WORK_ITEM_COLUMNS,
    COLUMN_LABELS,
    archivedWorkItems,
    archiveWorkItem,
    restoreWorkItem,
    moveWorkItem,
    startWorkItem,
    stopWorkItemRun,
    planWorkItem,
    runWorkItemStage,
    acceptWorkItemReview,
    requestWorkItemChanges,
    pendingDecisionByItem,
    pendingQuestionByItem,
    activePlanningRunByItem,
    latestPlanningRunByItem,
    attachmentsByWorkItem,
    runsByItem,
    workItemRunEvents,
    type WorkItemStatus,
    type WorkItemRun,
    type WorkItemStartActionOptions,
  } from "$lib/stores/workItems";
  import { sessionList } from "$lib/stores/sessions";
  import type { SessionStatus } from "$lib/types";
  import {
    openNewWorkItemEditor,
    openWorkItemEditor,
    openWorkItemSessionStart,
  } from "$lib/stores/ui";
  import { closeMainView } from "$lib/stores/mainView";
  import { openSessionById } from "$lib/panes/openSession";
  import { formatWorkItemStartError } from "$lib/board/startErrors";
  import {
    deleteWorkItemWithMode,
    type WorkItemDeleteMode,
  } from "$lib/workItems/deleteFlow";
  import { nextWorkItemStatuses } from "$lib/workItems/statusFlow";
  import {
    canStartImplementationFromPlanning,
    hasAttachedPlan,
  } from "$lib/workItems/planningGate";
  import { workItemPhase } from "$lib/workItems/phase";
  import {
    buildWorkItemReviewPackage,
    type WorkItemReviewPackage,
  } from "$lib/workItems/reviewPackage";
  import { resolveWorkItemOpenTarget } from "$lib/workItems/openTarget";
  import { resolveReviewAgentRepoRoot } from "$lib/workItems/reviewAgent";
  import type { WorkItem } from "$lib/bindings";
  import { createSessionShell, openPathInFinder } from "$lib/tauri";
  import { addSession, setActiveSession } from "$lib/stores/sessions";
  import { projects } from "$lib/stores/projects";
  import { settings } from "$lib/stores/settings";
  import { defaultAgentProfileId } from "$lib/panes/defaultAgent";
  import { reviewAgentProfileId } from "$lib/workItems/workflow";
  import { hasWorkItemDragData, readWorkItemDragData } from "$lib/board/drag";
  import { launchReviewDiff } from "$lib/externalTools/reviewDiff";
  import WorkItemCard from "./WorkItemCard.svelte";
  import AddCardInput from "./AddCardInput.svelte";
  import WorkItemDeleteDialog from "./WorkItemDeleteDialog.svelte";
  import WorkItemReviewModal from "./WorkItemReviewModal.svelte";
  import { reviewStageLabel } from "$lib/workItems/reviewStages";
  import { workflowStage, workflowStageActionLabel } from "$lib/workItems/workflow";

  const sessionStatusMap = derived(sessionList, ($sessions) => {
    const m = new Map<string, SessionStatus>();
    for (const s of $sessions) m.set(s.id, s.status);
    return m;
  });

  const reviewModalData = $derived.by(() => {
    const itemId = reviewModalItemId;
    if (!itemId) return null;
    const items = get(itemsByColumn);
    let item: WorkItem | null = null;
    for (const colItems of items.values()) {
      const found = colItems.find((i) => i.id === itemId);
      if (found) { item = found; break; }
    }
    if (!item || item.status !== "review") return null;
    const kanban = get(settings).kanban;
    const itemRuns = (get(runsByItem).get(item!.id) ?? []);
    const itemAttachments = (get(attachmentsByWorkItem).get(item!.id) ?? []);
    const events = get(workItemRunEvents);
    const rp = buildWorkItemReviewPackage(item, itemRuns, itemAttachments, events);
    const target = resolveWorkItemOpenTarget(item, itemRuns, rp);
    const stageName = reviewStageLabel(item.reviewStageId, kanban);
    const stage = workflowStage(kanban, item.workflowStageId);
    const stageActionText = workflowStageActionLabel(item.workflowStageId, kanban) ?? stageName ?? "Run";
    const wsActionLabel = stage?.actionLabel;
    const canRunStage = !!item.workflowStageId;
    const canFixCi = item.status === "review" &&
      item.reviewStageId === "pr_review" && !!item.pinnedPrUrl;
    const canOpenAgent = !!rp.worktreePath;
    const canOpenWt = !!rp.worktreePath;
    const reviewSessionId = target?.sessionId ?? rp.sessionId ?? null;
    const canViewDiff =
      !!get(settings).reviewDiffToolId && !!reviewSessionId;
    return {
      item,
      reviewPackage: rp,
      openTarget: target,
      reviewStageName: stageName,
      acceptReviewText: stageName ? `Accept ${stageName}` : "Accept done",
      workflowStageActionText: wsActionLabel ?? stageActionText,
      canRunWorkflowStage: canRunStage,
      canFixCi,
      canOpenReviewAgent: canOpenAgent,
      canOpenReviewWorktree: canOpenWt,
      canViewDiff,
      reviewSessionId,
    };
  });

  function handleOpenReview(itemId: string) {
    reviewModalItemId = itemId;
  }

  function handleCloseReview() {
    reviewModalItemId = null;
  }

  // Column currently under a valid drag, for the drop-target highlight.
  let dragOverColumn = $state<WorkItemStatus | null>(null);
  let startingItemIds = $state<Record<string, boolean>>({});
  let planningItemIds = $state<Record<string, boolean>>({});
  let runningStageItemIds = $state<Record<string, boolean>>({});
  let acceptingItemIds = $state<Record<string, boolean>>({});
  let requestingChangesItemIds = $state<Record<string, boolean>>({});
  let openingAgentItemIds = $state<Record<string, boolean>>({});
  let viewDiffItemIds = $state<Record<string, boolean>>({});
  let archivingItemIds = $state<Record<string, boolean>>({});
  let restoringItemIds = $state<Record<string, boolean>>({});
  let startErrors = $state<Record<string, string>>({});
  let planErrors = $state<Record<string, string>>({});
  let archiveErrors = $state<Record<string, string>>({});
  let deleteTarget = $state<WorkItem | null>(null);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);
  let reviewModalItemId = $state<string | null>(null);

  async function handleMove(id: string, status: WorkItemStatus) {
    await moveWorkItem(id, status, Date.now());
  }

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
      item.status === "planning" &&
      !canStartImplementationFromPlanning(attachments, forceStart)
    ) {
      if (planningRun?.sessionId) {
        await handleOpen(planningRun.sessionId, planningRun.ptyId);
      } else {
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
      if (item.status === "planning" && planningRun) {
        await stopWorkItemRun(planningRun.id);
      }
      await startWorkItem(id, {
        ...(forceStart ? { forceStart: true } : {}),
        ...(fixCi ? { fixCi: true } : {}),
      });
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
      const target = replaceActive
        ? await planWorkItem(id, { replaceActive: true })
        : await planWorkItem(id);
      await handleOpen(target.sessionId, target.ptyId);
    } catch (err) {
      planErrors = { ...planErrors, [id]: formatPlanError(err) };
      console.error("Failed to plan work item", err);
    } finally {
      planningItemIds = withoutKey(planningItemIds, id);
    }
  }

  async function handleRunStage(id: string, item: WorkItem) {
    if (runningStageItemIds[id]) return;
    if (!item.workflowStageId) {
      startErrors = { ...startErrors, [id]: "No workflow stage assigned." };
      return;
    }
    runningStageItemIds = { ...runningStageItemIds, [id]: true };
    startErrors = withoutKey(startErrors, id);
    try {
      const result = await runWorkItemStage(id, {
        stageId: item.workflowStageId,
      });
      if (result.run.sessionId) await handleOpen(result.run.sessionId);
    } catch (err) {
      startErrors = {
        ...startErrors,
        [id]: err instanceof Error ? err.message : "Failed to run stage.",
      };
      console.error("Failed to run work item stage", err);
    } finally {
      runningStageItemIds = withoutKey(runningStageItemIds, id);
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

  async function handleViewDiff(item: WorkItem) {
    if (!reviewModalData || viewDiffItemIds[item.id]) return;
    const toolId = get(settings).reviewDiffToolId;
    const sessionId = reviewModalData.reviewSessionId;
    if (!toolId || !sessionId) return;
    viewDiffItemIds = { ...viewDiffItemIds, [item.id]: true };
    startErrors = withoutKey(startErrors, item.id);
    try {
      await launchReviewDiff(toolId, sessionId, reviewModalData.reviewPackage);
    } catch (err) {
      startErrors = {
        ...startErrors,
        [item.id]: err instanceof Error ? err.message : String(err),
      };
    } finally {
      viewDiffItemIds = withoutKey(viewDiffItemIds, item.id);
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
      closeMainView();
    } catch (err) {
      startErrors = { ...startErrors, [item.id]: "Failed to open agent." };
      console.error("Failed to open review agent", err);
    } finally {
      openingAgentItemIds = withoutKey(openingAgentItemIds, item.id);
    }
  }

  function handleCreate(status: WorkItemStatus) {
    openNewWorkItemEditor({ status });
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

  async function handleOpen(sessionId: string, ptyId?: string | null) {
    const result = ptyId
      ? await openSessionById(sessionId, { ptyId })
      : await openSessionById(sessionId);
    if (result === "gone") {
      console.error(`Session ${sessionId} is no longer running`);
      return;
    }
    // Reveal the terminal we just focused — the main view covers it.
    closeMainView();
  }

  function handleDragOver(event: DragEvent, col: WorkItemStatus) {
    if (!hasWorkItemDragData(event.dataTransfer)) return;
    // preventDefault marks this element as a valid drop target.
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    dragOverColumn = col;
  }

  function handleDragLeave(col: WorkItemStatus) {
    if (dragOverColumn === col) dragOverColumn = null;
  }

  async function handleDrop(event: DragEvent, col: WorkItemStatus) {
    dragOverColumn = null;
    const payload = readWorkItemDragData(event.dataTransfer);
    if (!payload) return;
    event.preventDefault();
    if (payload.fromStatus === col) return;
    if (payload.fromStatus === "review" && col === "done") {
      await handleAcceptReview(payload.itemId);
      return;
    }
    if (!nextWorkItemStatuses(payload.fromStatus).includes(col)) return;
    await handleMove(payload.itemId, col);
  }

  async function handleArchive(id: string): Promise<void> {
    archivingItemIds = { ...archivingItemIds, [id]: true };
    archiveErrors = withoutKey(archiveErrors, id);
    try {
      await archiveWorkItem(id);
    } catch (err) {
      archiveErrors = {
        ...archiveErrors,
        [id]: err instanceof Error ? err.message : String(err),
      };
    } finally {
      archivingItemIds = withoutKey(archivingItemIds, id);
    }
  }

  async function handleRestore(id: string): Promise<void> {
    restoringItemIds = { ...restoringItemIds, [id]: true };
    archiveErrors = withoutKey(archiveErrors, id);
    try {
      await restoreWorkItem(id);
    } catch (err) {
      archiveErrors = {
        ...archiveErrors,
        [id]: err instanceof Error ? err.message : String(err),
      };
    } finally {
      restoringItemIds = withoutKey(restoringItemIds, id);
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col bg-bg-deep">
  <div class="flex min-h-0 flex-1 flex-row gap-3 overflow-x-auto p-4">
    {#each WORK_ITEM_COLUMNS as col (col)}
      {@const items = $itemsByColumn.get(col) ?? []}
      <section
        class="flex w-72 shrink-0 flex-col rounded-lg border bg-bg-base/40 transition-colors"
        class:border-accent={dragOverColumn === col}
        class:border-border-subtle={dragOverColumn !== col}
        data-testid="board-column"
        data-column={col}
        role="group"
        aria-label={COLUMN_LABELS[col]}
        ondragover={(e) => handleDragOver(e, col)}
        ondragleave={() => handleDragLeave(col)}
        ondrop={(e) => handleDrop(e, col)}
      >
        <div class="flex items-center gap-1.5 px-3 py-2">
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

        <div
          class="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto px-2 pt-1"
        >
          {#if items.length > 0}
            {#each items as item (item.id)}
              {@const sessionStatus = item.sessionId
                ? ($sessionStatusMap.get(item.sessionId) ?? null)
                : null}
              {@const pendingDecision =
                $pendingDecisionByItem.get(item.id) ?? null}
              {@const pendingQuestion =
                $pendingQuestionByItem.get(item.id) ?? null}
              {@const activePlanningRun =
                $activePlanningRunByItem.get(item.id) ?? null}
              {@const planningRun =
                item.status === "planning"
                  ? ($latestPlanningRunByItem.get(item.id) ?? activePlanningRun)
                  : activePlanningRun}
              {@const itemRuns = $runsByItem.get(item.id) ?? []}
              {@const itemAttachments =
                $attachmentsByWorkItem.get(item.id) ?? []}
              {@const attachedSessionIds = attachedSessionIdsForItem(
                item,
                itemRuns,
                planningRun?.sessionId ?? null,
              )}
              {@const attentionSessionId =
                pendingQuestion?.sessionId ??
                attachedSessionIds.find(
                  (sessionId) =>
                    $sessionStatusMap.get(sessionId) === "attention",
                ) ??
                null}
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
              {@const openTarget = resolveWorkItemOpenTarget(
                item,
                itemRuns,
                reviewPackage
              )}
              <WorkItemCard
                {item}
                {sessionStatus}
                {phase}
                {reviewPackage}
                {openTarget}
                {attachedSessionIds}
                {attentionSessionId}
                draggable
                onStart={handleStart}
                onPlan={handlePlan}
                onRunStage={handleRunStage}
                onOpen={handleOpen}
                onEdit={openWorkItemEditor}
                onDelete={handleDelete}
                onArchive={item.status === "done" ? handleArchive : undefined}
                onAcceptReview={handleAcceptReview}
                onRequestChanges={handleRequestChanges}
                onOpenWorktree={handleOpenWorktree}
                onOpenAgent={handleOpenAgent}
                onOpenReview={handleOpenReview}
                startPending={!!startingItemIds[item.id]}
                planPending={!!planningItemIds[item.id]}
                stagePending={!!runningStageItemIds[item.id]}
                acceptPending={!!acceptingItemIds[item.id]}
                requestChangesPending={!!requestingChangesItemIds[item.id]}
                openAgentPending={!!openingAgentItemIds[item.id]}
                archivePending={!!archivingItemIds[item.id]}
                startError={startErrors[item.id] ??
                  planErrors[item.id] ??
                  archiveErrors[item.id] ??
                  item.startError ??
                  null}
              />
            {/each}
          {:else}
            <p class="px-1 py-2 text-xs text-text-muted/50">Empty</p>
          {/if}
        </div>

        {#if col === "todo"}
          <div class="shrink-0 px-2 pb-2 pt-1">
            <AddCardInput onCreate={() => handleCreate(col)} />
          </div>
        {/if}
      </section>
    {/each}
  </div>

  {#if $archivedWorkItems.length > 0}
    <section
      class="shrink-0 border-t border-hairline bg-bg-base/35 px-4 py-3"
      aria-label="Archived cards"
    >
      <div class="mb-2 flex items-center gap-2">
        <h2
          class="text-[11px] font-semibold uppercase tracking-wide text-text-muted"
        >
          Archived
        </h2>
        <span
          class="rounded-full bg-surface-2 px-1.5 py-0.5 text-[9px] font-medium text-text-muted"
        >
          {$archivedWorkItems.length}
        </span>
      </div>
      <div class="flex gap-2 overflow-x-auto pb-1">
        {#each $archivedWorkItems as item (item.id)}
          <div
            class="flex min-w-56 max-w-72 items-center gap-2 rounded-md border border-border-subtle bg-bg-surface/70 px-3 py-2"
          >
            <button
              type="button"
              class="min-w-0 flex-1 truncate text-left text-[12px] font-medium text-text-secondary hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
              onclick={() => openWorkItemEditor(item.id)}
            >
              {item.title}
            </button>
            <button
              type="button"
              class="shrink-0 rounded border border-border-subtle px-2 py-1 text-[11px] font-medium text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 disabled:cursor-wait disabled:opacity-60"
              onclick={() => void handleRestore(item.id)}
              disabled={!!restoringItemIds[item.id]}
              aria-label={`Restore ${item.title}`}
            >
              {restoringItemIds[item.id] ? "Restoring..." : "Restore"}
            </button>
          </div>
        {/each}
      </div>
    </section>
  {/if}
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

{#if reviewModalData}
  <WorkItemReviewModal
    item={reviewModalData.item}
    reviewPackage={reviewModalData.reviewPackage}
    openTarget={reviewModalData.openTarget}
    reviewStageName={reviewModalData.reviewStageName}
    acceptReviewText={reviewModalData.acceptReviewText}
    open={true}
    onAccept={handleAcceptReview}
    onRequestChanges={handleRequestChanges}
    onOpenSession={handleOpen}
    onOpenAgent={handleOpenAgent}
    onOpenWorktree={handleOpenWorktree}
    onRunStage={handleRunStage}
    onFixCi={(id, item) => handleStart(id, item, { fixCi: true })}
    onClose={handleCloseReview}
    error={startErrors[reviewModalItemId ?? ""] ??
      reviewModalData.item.startError ??
      null}
    acceptPending={!!acceptingItemIds[reviewModalItemId ?? ""]}
    requestChangesPending={!!requestingChangesItemIds[reviewModalItemId ?? ""]}
    stagePending={!!runningStageItemIds[reviewModalItemId ?? ""]}
    startPending={!!startingItemIds[reviewModalItemId ?? ""]}
    openAgentPending={!!openingAgentItemIds[reviewModalItemId ?? ""]}
    viewDiffPending={!!viewDiffItemIds[reviewModalItemId ?? ""]}
    canRunWorkflowStage={reviewModalData.canRunWorkflowStage}
    canFixCi={reviewModalData.canFixCi}
    canOpenReviewAgent={reviewModalData.canOpenReviewAgent}
    canOpenReviewWorktree={reviewModalData.canOpenReviewWorktree}
    canViewDiff={reviewModalData.canViewDiff}
    onViewDiff={handleViewDiff}
    workflowStageActionText={reviewModalData.workflowStageActionText}
  />
{/if}
