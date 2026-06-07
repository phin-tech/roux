<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import type {
    KanbanWorkflowPhaseSettings,
    KanbanWorkflowStageSettings,
  } from "$lib/bindings";
  import { profileList, type SpawnProfile } from "$lib/panes/profiles";
  import {
    persistSettingsImmediately,
    settings,
    settlePendingSettingsPersist,
    updateSetting,
  } from "$lib/stores/settings";
  import {
    createKanbanWorkflowExample,
    kanbanWorkflowConfigDir,
    saveKanbanWorkflowJson,
    validateKanbanWorkflow,
  } from "$lib/tauri";
  import {
    WORKFLOW_PHASE_IDS,
    normalizeKanbanSettings,
    type RequiredKanbanSettings,
    type WorkflowPhaseId,
  } from "$lib/workItems/workflow";

  const workflowPhases: { id: WorkflowPhaseId; title: string }[] = [
    { id: "todo", title: "To Do" },
    { id: "planning", title: "Planning" },
    { id: "doing", title: "Doing" },
    { id: "review", title: "Review" },
    { id: "done", title: "Done" },
  ];

  let workflowActionBusy = $state<
    "browse" | "validate" | "copy" | "save" | "reveal" | null
  >(null);
  let workflowActionError = $state<string | null>(null);
  let workflowActionStatus = $state<string | null>(null);

  const workflowActionButton =
    "inline-flex items-center gap-1 rounded border border-border bg-bg-deep px-2 py-1 text-[11px] text-text-secondary hover:border-accent-dim hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-50";

  const availableProfiles = $derived.by<SpawnProfile[]>(() => {
    const byId = new Map<string, SpawnProfile>();
    for (const profile of $profileList) byId.set(profile.id, profile);
    for (const profile of $settings.spawnProfiles ?? []) {
      byId.set(profile.id, { ...profile, source: "user" });
    }
    return Array.from(byId.values()).filter((profile) => {
      const provider = profile.provider;
      const command = (profile.startupCommand ?? "").trim();
      // Planning/start runs reject plain shells and profiles without commands.
      return (
        (provider === "claude" || provider === "codex") &&
        profile.startupBehavior !== "typeOnly" &&
        command.length > 0
      );
    });
  });

  const kanban = $derived(normalizeKanbanSettings($settings.kanban));
  const workflowJsonBacked = $derived(Boolean(kanban.workflowPath?.trim()));
  const orderedPhaseIds = $derived(
    kanban.workflow.phaseOrder.filter((id): id is WorkflowPhaseId =>
      (WORKFLOW_PHASE_IDS as readonly string[]).includes(id),
    ),
  );

  function updateKanban(next: RequiredKanbanSettings): void {
    updateSetting("kanban", next);
  }

  function updateWorkflowLabel(label: string): void {
    updateKanban({
      ...kanban,
      workflow: { ...kanban.workflow, label },
    });
  }

  function updateWorkflowPath(path: string): void {
    const workflowPath = path.trim();
    workflowActionError = null;
    workflowActionStatus = null;
    updateKanban({
      ...kanban,
      workflowPath: workflowPath.length > 0 ? workflowPath : null,
      workflowLoadError: null,
    });
  }

  function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  function settingsWithKanban(next: RequiredKanbanSettings) {
    return { ...$settings, kanban: next };
  }

  async function applyValidatedKanban(
    next: RequiredKanbanSettings,
  ): Promise<RequiredKanbanSettings> {
    await settlePendingSettingsPersist();
    const updated = await validateKanbanWorkflow(settingsWithKanban(next));
    const validatedKanban = normalizeKanbanSettings(updated.kanban);
    let merged = settingsWithKanban(validatedKanban);
    settings.update((current) => {
      merged = { ...current, kanban: validatedKanban };
      return merged;
    });
    await persistSettingsImmediately(merged);
    return validatedKanban;
  }

  async function browseWorkflowJson(): Promise<void> {
    workflowActionBusy = "browse";
    workflowActionError = null;
    workflowActionStatus = null;
    try {
      const selected = await open({
        directory: false,
        multiple: false,
        title: "Select Kanban workflow JSON",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof selected !== "string") return;
      const next = {
        ...kanban,
        workflowPath: selected,
        workflowLoadError: null,
      };
      const validated = await applyValidatedKanban(next);
      workflowActionStatus = validated.workflowLoadError
        ? null
        : "Workflow JSON is valid.";
    } catch (error) {
      workflowActionError = errorMessage(error);
    } finally {
      workflowActionBusy = null;
    }
  }

  async function validateWorkflowJson(): Promise<void> {
    workflowActionBusy = "validate";
    workflowActionError = null;
    workflowActionStatus = null;
    try {
      const validated = await applyValidatedKanban(kanban);
      workflowActionStatus = validated.workflowLoadError
        ? null
        : "Workflow JSON is valid.";
    } catch (error) {
      workflowActionError = errorMessage(error);
    } finally {
      workflowActionBusy = null;
    }
  }

  async function copyExampleWorkflow(): Promise<void> {
    workflowActionBusy = "copy";
    workflowActionError = null;
    workflowActionStatus = null;
    try {
      const result = await createKanbanWorkflowExample();
      const next = {
        ...kanban,
        workflowPath: result.workflowPath,
        workflowLoadError: null,
      };
      const validated = await applyValidatedKanban(next);
      workflowActionStatus = validated.workflowLoadError
        ? null
        : `Using ${result.workflowPath}.`;
    } catch (error) {
      workflowActionError = errorMessage(error);
    } finally {
      workflowActionBusy = null;
    }
  }

  async function saveWorkflowJson(): Promise<void> {
    const workflowPath = kanban.workflowPath?.trim() ?? "";
    if (!workflowPath) {
      workflowActionError = "Set a JSON file path before saving.";
      workflowActionStatus = null;
      return;
    }
    workflowActionBusy = "save";
    workflowActionError = null;
    workflowActionStatus = null;
    try {
      const path = await saveKanbanWorkflowJson(workflowPath, kanban.workflow);
      const validated = await applyValidatedKanban(kanban);
      workflowActionStatus = validated.workflowLoadError
        ? null
        : `Saved ${path}.`;
    } catch (error) {
      workflowActionError = errorMessage(error);
    } finally {
      workflowActionBusy = null;
    }
  }

  async function revealWorkflowConfigDir(): Promise<void> {
    workflowActionBusy = "reveal";
    workflowActionError = null;
    workflowActionStatus = null;
    try {
      const dir = await kanbanWorkflowConfigDir();
      await revealItemInDir(dir);
    } catch (error) {
      workflowActionError = errorMessage(error);
    } finally {
      workflowActionBusy = null;
    }
  }

  function updatePhase(
    phaseId: WorkflowPhaseId,
    patch: Partial<KanbanWorkflowPhaseSettings>,
  ): void {
    updateKanban({
      ...kanban,
      workflow: {
        ...kanban.workflow,
        phases: {
          ...kanban.workflow.phases,
          [phaseId]: {
            ...kanban.workflow.phases[phaseId],
            ...patch,
          },
        },
      },
    });
  }

  function updateStage(
    phaseId: WorkflowPhaseId,
    stageId: string,
    patch: Partial<KanbanWorkflowStageSettings>,
  ): void {
    const phase = kanban.workflow.phases[phaseId];
    updatePhase(phaseId, {
      stages: {
        ...phase.stages,
        [stageId]: {
          ...phase.stages[stageId],
          ...patch,
        },
      },
    });
  }
</script>

<div class="py-2">
  <label for="kanban-workflow-label" class="text-[13px] font-semibold"
    >Workflow</label
  >
  <input
    id="kanban-workflow-label"
    aria-label="Workflow label"
    class="mt-2 w-full rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
    value={kanban.workflow.label}
    disabled={workflowJsonBacked}
    oninput={(e) => updateWorkflowLabel(e.currentTarget.value)}
  />
  <label
    for="kanban-workflow-path"
    class="mt-3 block text-[11px] uppercase tracking-wider text-text-muted"
    >JSON file</label
  >
  <input
    id="kanban-workflow-path"
    aria-label="Workflow JSON file"
    class="mt-1 w-full rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
    value={kanban.workflowPath ?? ""}
    placeholder="workflow.json"
    oninput={(e) => updateWorkflowPath(e.currentTarget.value)}
  />
  <div class="mt-2 flex flex-wrap gap-2">
    <button
      type="button"
      class={workflowActionButton}
      disabled={workflowActionBusy !== null}
      onclick={() => void browseWorkflowJson()}
    >
      <FolderOpen size={12} />
      Browse
    </button>
    <button
      type="button"
      class={workflowActionButton}
      disabled={workflowActionBusy !== null}
      onclick={() => void validateWorkflowJson()}
    >
      <Check size={12} />
      Validate
    </button>
    <button
      type="button"
      class={workflowActionButton}
      disabled={workflowActionBusy !== null}
      onclick={() => void copyExampleWorkflow()}
    >
      <Copy size={12} />
      Copy example
    </button>
    <button
      type="button"
      class={workflowActionButton}
      disabled={workflowActionBusy !== null || !kanban.workflowPath}
      onclick={() => void saveWorkflowJson()}
    >
      <Check size={12} />
      Save JSON
    </button>
    <button
      type="button"
      class={workflowActionButton}
      disabled={workflowActionBusy !== null}
      onclick={() => void revealWorkflowConfigDir()}
    >
      <Folder size={12} />
      Reveal
    </button>
  </div>
  {#if workflowActionError}
    <div class="mt-2 text-xs text-red" role="alert">{workflowActionError}</div>
  {:else if workflowActionStatus}
    <div class="mt-2 text-xs text-text-muted">{workflowActionStatus}</div>
  {/if}
  {#if kanban.workflowLoadError}
    <div class="mt-2 text-xs text-red" role="alert">
      {kanban.workflowLoadError}
    </div>
  {/if}
  {#if kanban.workflowPath}
    <div class="mt-2 text-xs text-text-muted">
      Workflow fields are read-only while JSON owns the workflow. Use Validate
      after changing the file on disk.
    </div>
  {/if}
</div>

{#each orderedPhaseIds as phaseId (phaseId)}
  {@const phaseInfo = workflowPhases.find(
    (candidate) => candidate.id === phaseId,
  ) ?? {
    id: phaseId,
    title: phaseId,
  }}
  {@const phase = kanban.workflow.phases[phaseInfo.id]}
  <div class="mt-3 rounded border border-border-subtle bg-bg-surface/35 p-3">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <div class="text-[13px] font-semibold">{phaseInfo.title}</div>
        <div class="mt-1 flex flex-wrap items-center gap-2">
          <label
            class="text-[11px] uppercase tracking-wider text-text-muted"
            for={`kanban-${phaseInfo.id}-label`}
          >
            Label
          </label>
          <input
            id={`kanban-${phaseInfo.id}-label`}
            aria-label={`${phaseInfo.title} label`}
            class="w-40 rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-primary outline-none focus:border-accent-dim"
            value={phase.label}
            disabled={workflowJsonBacked}
            oninput={(e) =>
              updatePhase(phaseInfo.id, { label: e.currentTarget.value })}
          />
        </div>
      </div>
      <label
        class="flex flex-col gap-1 text-[11px] uppercase tracking-wider text-text-muted"
      >
        Agent
        <select
          aria-label={`${phaseInfo.title} agent`}
          class="max-w-[14rem] cursor-pointer appearance-none rounded border border-border bg-bg-deep px-2 py-1 pr-6 text-xs text-text-primary outline-none focus:border-accent-dim"
          value={phase.agentProfile ?? ""}
          disabled={workflowJsonBacked}
          onchange={(e) =>
            updatePhase(phaseInfo.id, {
              agentProfile: e.currentTarget.value || null,
            })}
        >
          <option value="">Global default</option>
          {#each availableProfiles as profile (profile.id)}
            <option value={profile.id}>{profile.name}</option>
          {/each}
        </select>
      </label>
    </div>

    <textarea
      aria-label={`${phaseInfo.title} instructions`}
      class="mt-3 min-h-20 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
      value={phase.instructions}
      disabled={workflowJsonBacked}
      oninput={(e) =>
        updatePhase(phaseInfo.id, { instructions: e.currentTarget.value })}
    ></textarea>

    <div class="mt-3 space-y-3">
      {#each phase.stageOrder as stageId (stageId)}
        {@const stage = phase.stages[stageId]}
        {#if stage}
          <div class="border-t border-border-subtle pt-3">
            <div class="flex flex-wrap items-start justify-between gap-3">
              <div class="flex min-w-0 flex-wrap gap-2">
                <label
                  class="flex flex-col gap-1 text-[11px] uppercase tracking-wider text-text-muted"
                >
                  Stage
                  <input
                    aria-label={`${stageId} label`}
                    class="w-40 rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-primary outline-none focus:border-accent-dim"
                    value={stage.label}
                    disabled={workflowJsonBacked}
                    oninput={(e) =>
                      updateStage(phaseInfo.id, stageId, {
                        label: e.currentTarget.value,
                      })}
                  />
                </label>
                <label
                  class="flex flex-col gap-1 text-[11px] uppercase tracking-wider text-text-muted"
                >
                  Button
                  <input
                    aria-label={`${stageId} action label`}
                    class="w-32 rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-primary outline-none focus:border-accent-dim"
                    value={stage.actionLabel ?? ""}
                    disabled={workflowJsonBacked}
                    oninput={(e) =>
                      updateStage(phaseInfo.id, stageId, {
                        actionLabel: e.currentTarget.value.trim() || null,
                      })}
                  />
                </label>
              </div>
              <label
                class="flex flex-col gap-1 text-[11px] uppercase tracking-wider text-text-muted"
              >
                Agent
                <select
                  aria-label={`${stage.label || stageId} stage agent`}
                  class="max-w-[14rem] cursor-pointer appearance-none rounded border border-border bg-bg-deep px-2 py-1 pr-6 text-xs text-text-primary outline-none focus:border-accent-dim"
                  value={stage.agentProfile ?? ""}
                  disabled={workflowJsonBacked}
                  onchange={(e) =>
                    updateStage(phaseInfo.id, stageId, {
                      agentProfile: e.currentTarget.value || null,
                    })}
                >
                  <option value="">Phase/default</option>
                  {#each availableProfiles as profile (profile.id)}
                    <option value={profile.id}>{profile.name}</option>
                  {/each}
                </select>
              </label>
            </div>
            <textarea
              aria-label={`${stage.label || stageId} stage instructions`}
              class="mt-2 min-h-16 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
              value={stage.instructions}
              disabled={workflowJsonBacked}
              oninput={(e) =>
                updateStage(phaseInfo.id, stageId, {
                  instructions: e.currentTarget.value,
                })}
            ></textarea>
          </div>
        {/if}
      {/each}
    </div>
  </div>
{/each}
