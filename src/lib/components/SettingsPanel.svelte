<script lang="ts">
  import Settings from "@lucide/svelte/icons/settings";
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import TerminalIcon from "@lucide/svelte/icons/terminal";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Bell from "@lucide/svelte/icons/bell";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Wrench from "@lucide/svelte/icons/wrench";
  import Plug from "@lucide/svelte/icons/plug";
  import NotebookPen from "@lucide/svelte/icons/notebook-pen";
  import ClipboardList from "@lucide/svelte/icons/clipboard-list";
  import FlaskConical from "@lucide/svelte/icons/flask-conical";
  import X from "@lucide/svelte/icons/x";
  import SettingsAdvancedSection from "$lib/components/settings/SettingsAdvancedSection.svelte";
  import SettingsAgentsSection from "$lib/components/settings/SettingsAgentsSection.svelte";
  import SettingsExperimentsSection from "$lib/components/settings/SettingsExperimentsSection.svelte";
  import SettingsExternalToolsSection from "$lib/components/settings/SettingsExternalToolsSection.svelte";
  import SettingsGeneralSection from "$lib/components/settings/SettingsGeneralSection.svelte";
  import SettingsIntegrationsSection from "$lib/components/settings/SettingsIntegrationsSection.svelte";
  import SettingsKanbanSection from "$lib/components/settings/SettingsKanbanSection.svelte";
  import SettingsKeyboardSection from "$lib/components/settings/SettingsKeyboardSection.svelte";
  import SettingsNotesSection from "$lib/components/settings/SettingsNotesSection.svelte";
  import SettingsNotificationsSection from "$lib/components/settings/SettingsNotificationsSection.svelte";
  import SettingsSessionsSection from "$lib/components/settings/SettingsSessionsSection.svelte";
  import SettingsTerminalSection from "$lib/components/settings/SettingsTerminalSection.svelte";
  import { settingsFocus } from "$lib/stores/settingsFocus";
  import {
    normalizeSettingsCategoryId,
    type SettingsCategoryId,
  } from "$lib/settings/categories";

  const CATEGORIES: { id: SettingsCategoryId; label: string; icon: typeof Settings }[] = [
    { id: "general", label: "General", icon: Settings },
    { id: "sessions", label: "Sessions", icon: FolderTree },
    { id: "terminal", label: "Terminal", icon: TerminalIcon },
    { id: "agents", label: "Agents", icon: Sparkles },
    { id: "kanban", label: "Kanban", icon: ClipboardList },
    { id: "externalTools", label: "External Tools", icon: Wrench },
    { id: "notes", label: "Notes", icon: NotebookPen },
    { id: "integrations", label: "Integrations", icon: Plug },
    { id: "notifications", label: "Notifications", icon: Bell },
    { id: "keyboard", label: "Keyboard", icon: Keyboard },
    { id: "experiments", label: "Experiments", icon: FlaskConical },
    { id: "advanced", label: "Advanced", icon: Wrench },
  ];

  interface Props {
    visible: boolean;
    onclose: () => void;
    initialCategory?: SettingsCategoryId | null;
    externalToolId?: string | null;
  }

  let { visible, onclose, initialCategory = null, externalToolId = null }: Props = $props();

  let selected = $state<SettingsCategoryId>("general");
  let focusedExternalToolId = $state<string | null>(null);
  let externalToolFocusToken = $state(0);
  let nextExternalToolFocusToken = 0;
  let wasVisible = false;

  function focusExternalTool(id: string | null): void {
    focusedExternalToolId = id;
    externalToolFocusToken = ++nextExternalToolFocusToken;
  }

  function clearExternalToolFocus(): void {
    focusedExternalToolId = null;
    externalToolFocusToken = 0;
  }

  $effect(() => {
    const justOpened = visible && !wasVisible;
    wasVisible = visible;
    if (!visible) return;

    const focus = $settingsFocus;
    if (focus?.category) {
      selected = normalizeSettingsCategoryId(focus.category);
      if (focus.category === "externalTools" && "externalToolId" in focus) {
        focusExternalTool(focus.externalToolId ?? null);
      }
      settingsFocus.set(null);
      return;
    }

    if (initialCategory) {
      selected = normalizeSettingsCategoryId(initialCategory);
      if (initialCategory === "externalTools") {
        focusExternalTool(externalToolId ?? null);
      }
      return;
    }

    if (justOpened) selected = "general";
  });

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={visible ? handleKey : undefined} />

{#if visible}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="flex h-full min-h-0 overflow-hidden bg-bg-deep"
    role="region"
    aria-label="Preferences"
    onkeydown={handleKey}
    tabindex="-1"
  >
    <aside class="flex w-[180px] shrink-0 flex-col border-r border-hairline bg-bg-surface/30 py-3">
      <div class="flex items-center gap-2 px-3 pb-2">
        <button
          aria-label="Close settings"
          class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
          onclick={onclose}
        >
          <X size={14} />
        </button>
        <div class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Preferences</div>
      </div>
      <nav class="flex flex-col gap-0.5 px-2">
        {#each CATEGORIES as cat}
          {@const Icon = cat.icon}
          <button
            class="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors
              {selected === cat.id
                ? 'bg-accent-dim text-text-primary'
                : 'text-text-secondary hover:bg-bg-hover'}"
            onclick={() => (selected = cat.id)}
          >
            <Icon size={14} />
            <span>{cat.label}</span>
          </button>
        {/each}
      </nav>
    </aside>

    <div class="flex min-w-0 flex-1 flex-col">
      <div class="flex h-10 shrink-0 items-center border-b border-hairline px-4">
        <h2 class="text-sm font-semibold tracking-tight">
          {CATEGORIES.find((c) => c.id === selected)?.label}
        </h2>
      </div>

      <div class="app-scrollbar flex-1 overflow-y-auto px-5 py-4">
        {#if selected === "general"}
          <SettingsGeneralSection />
        {:else if selected === "sessions"}
          <SettingsSessionsSection />
        {:else if selected === "terminal"}
          <SettingsTerminalSection />
        {:else if selected === "agents"}
          <SettingsAgentsSection />
        {:else if selected === "kanban"}
          <SettingsKanbanSection />
        {:else if selected === "externalTools"}
          <SettingsExternalToolsSection
            focusedToolId={focusedExternalToolId}
            focusToken={externalToolFocusToken}
            onfocusapplied={clearExternalToolFocus}
          />
        {:else if selected === "notes"}
          <SettingsNotesSection />
        {:else if selected === "integrations"}
          <SettingsIntegrationsSection />
        {:else if selected === "notifications"}
          <SettingsNotificationsSection />
        {:else if selected === "keyboard"}
          <SettingsKeyboardSection />
        {:else if selected === "experiments"}
          <SettingsExperimentsSection />
        {:else if selected === "advanced"}
          <SettingsAdvancedSection />
        {/if}
      </div>
    </div>
  </div>
{/if}
