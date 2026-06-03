<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { THEME_DEFINITIONS } from "$lib/themes";
  import { quitApp } from "$lib/tauri";
  import {
    settings,
    setStartupTarget,
    updateSetting,
  } from "$lib/stores/settings";
  import {
    sidebarLayout,
    setRailSide,
    type Side,
  } from "$lib/stores/sidebarLayout";
  import { updateStatus, runManualCheck, performInstall } from "$lib/stores/updater";
  import type { ExternalTool, StartupTarget, UpdateChannel } from "$lib/bindings";

  const STARTUP_TARGET_OPTIONS: { id: StartupTarget; label: string }[] = [
    { id: "restore", label: "Restore previous" },
    { id: "sessionsSidebar", label: "Sessions sidebar" },
    { id: "lastSession", label: "Last session" },
    { id: "kanbanWide", label: "Kanban wide view" },
    { id: "externalTool", label: "External tool" },
    { id: "none", label: "None" },
  ];

  let appVersion = $state<string>("...");

  onMount(async () => {
    try {
      appVersion = await getVersion();
    } catch {
      appVersion = "unknown";
    }
  });

  function describeError(reason: "network" | "signature-invalid" | "unknown"): string {
    switch (reason) {
      case "network":
        return "Couldn't reach the update server.";
      case "signature-invalid":
        return "Update signature invalid. Download blocked.";
      case "unknown":
        return "Update check failed.";
    }
  }

  function globalExternalTools(): ExternalTool[] {
    return ($settings.externalTools ?? []).filter((tool) => tool.enabled !== false && !(tool.requiresSession ?? false));
  }
</script>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-start justify-between gap-3">
    <div>
      <div class="text-[13px]">Theme</div>
      <div class="text-[11px] text-text-muted mt-0.5">Color preset for the app chrome. Terminal palette is configured separately under Terminal.</div>
    </div>
    <select
      class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
      value={$settings.theme}
      onchange={(e) => updateSetting("theme", e.currentTarget.value as typeof $settings.theme)}
    >
      {#each THEME_DEFINITIONS as theme}
        <option value={theme.id}>{theme.label}</option>
      {/each}
    </select>
  </div>
  <p class="mt-2 text-[11px] text-text-muted">
    {THEME_DEFINITIONS.find((theme) => theme.id === $settings.theme)?.description}
  </p>
</div>

<div class="mt-4 flex items-center justify-between py-2">
  <span class="text-[13px]">UI font</span>
  <input
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-56 text-right focus:border-accent-dim"
    value={$settings.uiFontFamily}
    oninput={(e) => updateSetting("uiFontFamily", e.currentTarget.value)}
  />
</div>

<div class="mt-4 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-start justify-between gap-3">
    <div>
      <label for="settings-startup-target" class="text-[13px]">Open on launch</label>
      <div class="mt-0.5 text-[11px] text-text-muted">Choose the initial Roux surface after startup.</div>
    </div>
    <select
      id="settings-startup-target"
      aria-label="Open on launch"
      class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
      value={$settings.startupTarget ?? "restore"}
      onchange={(e) => setStartupTarget(e.currentTarget.value as StartupTarget)}
    >
      {#each STARTUP_TARGET_OPTIONS as option}
        <option value={option.id}>{option.label}</option>
      {/each}
    </select>
  </div>
  {#if ($settings.startupTarget ?? "restore") === "externalTool"}
    <div class="mt-3 flex items-center justify-between gap-3">
      <label for="settings-startup-external-tool" class="text-[13px]">Launch external tool</label>
      <select
        id="settings-startup-external-tool"
        aria-label="Launch external tool"
        class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
        value={$settings.startupExternalToolId ?? ""}
        onchange={(e) => updateSetting("startupExternalToolId", e.currentTarget.value || null)}
      >
        {#each globalExternalTools() as tool (tool.id)}
          <option value={tool.id}>{tool.name}</option>
        {/each}
      </select>
    </div>
    {#if globalExternalTools().length === 0}
      <div class="mt-2 text-[11px] text-amber">No enabled global external tools are available.</div>
    {/if}
  {/if}
</div>

<div class="mt-4 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between py-1">
    <div>
      <div class="text-[13px] font-semibold">Version</div>
      <div class="text-[11px] text-text-muted mt-0.5">{appVersion}</div>
    </div>
    <button
      class="rounded border border-border px-2.5 py-1 text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-50"
      disabled={$updateStatus.kind === "checking" || $updateStatus.kind === "downloading"}
      onclick={() => void runManualCheck()}
    >
      {$updateStatus.kind === "checking" ? "Checking..." : "Check for updates"}
    </button>
  </div>

  {#if $updateStatus.kind === "no-update"}
    <div class="mt-2 text-[11px] text-text-secondary">You're on the latest version.</div>
  {:else if $updateStatus.kind === "available"}
    <div class="mt-3 rounded-lg border border-accent/30 bg-accent/5 p-3">
      <div class="text-[12px] font-semibold text-text-primary">Update available: {$updateStatus.version}</div>
      {#if $updateStatus.notes}
        <pre class="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap text-[11px] text-text-secondary">{$updateStatus.notes}</pre>
      {/if}
      <button
        class="mt-3 rounded border border-accent bg-accent-dim px-3 py-1 text-[11px] font-semibold text-text-primary hover:bg-accent/40"
        onclick={() => void performInstall()}
      >
        Install and restart
      </button>
    </div>
  {:else if $updateStatus.kind === "downloading"}
    <div class="mt-3 rounded-lg border border-border-subtle bg-bg-surface/35 p-3">
      <div class="text-[11px] text-text-secondary">
        Downloading update{$updateStatus.progress !== null ? ` (${Math.round($updateStatus.progress * 100)}%)` : "..."}
      </div>
      <div class="mt-2 h-1.5 w-full overflow-hidden rounded bg-bg-deep">
        <div
          class="h-full bg-accent transition-[width] duration-200"
          style="width: {$updateStatus.progress !== null ? Math.round($updateStatus.progress * 100) : 30}%"
        ></div>
      </div>
    </div>
  {:else if $updateStatus.kind === "installed-restart-required"}
    <div class="mt-3 rounded-lg border border-accent/30 bg-accent/5 p-3">
      <div class="text-[12px] font-semibold text-text-primary">Update installed</div>
      <div class="text-[11px] text-text-secondary mt-0.5">Quit and reopen Roux to finish.</div>
      <button
        class="mt-3 rounded border border-accent bg-accent-dim px-3 py-1 text-[11px] font-semibold text-text-primary hover:bg-accent/40"
        onclick={() => void quitApp()}
      >
        Quit Roux
      </button>
    </div>
  {:else if $updateStatus.kind === "error"}
    <div class="mt-2 text-[11px] text-red">{describeError($updateStatus.reason)}</div>
  {/if}

  <div class="mt-4 flex items-center justify-between py-2">
    <div>
      <div class="text-[13px]">Update channel</div>
      <div class="text-[11px] text-text-muted mt-0.5">Switching to Stable takes effect on the next stable release at or above your current version.</div>
    </div>
    <select
      class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
      value={$settings.updateChannel ?? "stable"}
      onchange={(e) => updateSetting("updateChannel", e.currentTarget.value as UpdateChannel)}
    >
      <option value="stable">Stable</option>
      <option value="preRelease">Pre-release (Alpha)</option>
    </select>
  </div>

  <div class="mt-4 flex items-center justify-between py-2">
    <div>
      <div class="text-[13px]">Check for updates on launch</div>
      <div class="text-[11px] text-text-muted mt-0.5">Silently check in the background when Roux starts</div>
    </div>
    <button
      aria-label="Toggle auto-check on launch"
      class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
        {($settings.updateCheckOnLaunch ?? true) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
      onclick={() => updateSetting("updateCheckOnLaunch", !($settings.updateCheckOnLaunch ?? true))}
    >
      <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
        {($settings.updateCheckOnLaunch ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
    </button>
  </div>
</div>

<div class="mt-4 flex items-center justify-between py-2">
  <span class="text-[13px]">Sidebar position</span>
  <select
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
    value={$sidebarLayout.railSide}
    onchange={(e) => setRailSide(e.currentTarget.value as Side)}
  >
    <option value="left">Left</option>
    <option value="right">Right</option>
  </select>
</div>
<div class="flex items-center justify-between py-2">
  <span class="text-[13px]">Status bar position</span>
  <select
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
    value={$settings.statusBarPosition ?? "bottom"}
    onchange={(e) => updateSetting("statusBarPosition", e.currentTarget.value as "top" | "bottom")}
  >
    <option value="top">Top</option>
    <option value="bottom">Bottom</option>
  </select>
</div>
