<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";
  import type { KanbanSettings } from "$lib/bindings";

  const KANBAN_DEFAULTS: KanbanSettings = {
    defaultAgentProfile: "claude",
    planningPromptAppend: "",
    implementationPromptAppend: "",
    reviewPromptAppend: "",
    startupSidebar: "restore",
  };

  function kanbanSettings(): KanbanSettings {
    return { ...KANBAN_DEFAULTS, ...($settings.kanban ?? {}) };
  }

  let kanban = $derived(kanbanSettings());

  function updateKanban<K extends keyof KanbanSettings>(
    key: K,
    value: KanbanSettings[K],
  ): void {
    updateSetting("kanban", { ...kanbanSettings(), [key]: value });
  }
</script>

<div class="py-2">
  <div class="text-[13px]">Planning instructions</div>
  <div class="mt-0.5 text-[11px] text-text-muted">Appended after Roux's required planning prompt.</div>
  <textarea
    class="mt-2 min-h-24 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
    value={kanban.planningPromptAppend}
    oninput={(e) => updateKanban("planningPromptAppend", e.currentTarget.value)}
  ></textarea>
</div>

<div class="py-2">
  <div class="text-[13px]">Implementation instructions</div>
  <div class="mt-0.5 text-[11px] text-text-muted">Appended after Roux's required Start prompt.</div>
  <textarea
    class="mt-2 min-h-24 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
    value={kanban.implementationPromptAppend}
    oninput={(e) => updateKanban("implementationPromptAppend", e.currentTarget.value)}
  ></textarea>
</div>

<div class="py-2">
  <div class="text-[13px]">Review handoff instructions</div>
  <div class="mt-0.5 text-[11px] text-text-muted">Included in the implementation prompt until automated review runs exist.</div>
  <textarea
    class="mt-2 min-h-24 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
    value={kanban.reviewPromptAppend}
    oninput={(e) => updateKanban("reviewPromptAppend", e.currentTarget.value)}
  ></textarea>
</div>
