<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { commands } from "$lib/bindings";
  import type {
    AgentNotificationProviderStatus,
    AgentNotificationSetupStatus,
    CodexNotificationConfigPreview,
  } from "$lib/bindings";
  import {
    settings,
    setDefaultAgentProfile,
    updateSetting,
  } from "$lib/stores/settings";
  import { profileList, type SpawnProfile } from "$lib/panes/profiles";
  import { effectiveDefaultAgentProfileId } from "$lib/panes/defaultAgent";

  const availableProfiles = $derived.by<SpawnProfile[]>(() => {
    const byId = new Map<string, SpawnProfile>();
    for (const profile of $profileList) byId.set(profile.id, profile);
    for (const profile of $settings.spawnProfiles ?? []) {
      byId.set(profile.id, { ...profile, source: "user" });
    }
    return Array.from(byId.values());
  });

  const autonomousProfiles = $derived(
    availableProfiles.filter((profile: SpawnProfile) => {
      const provider = profile.provider ?? (profile.id === "claude" ? "claude" : null);
      const command = (profile.startupCommand ?? profile.setupCommand ?? "").trim();
      return (
        (provider === "claude" || provider === "codex") &&
        profile.startupBehavior !== "typeOnly" &&
        command.length > 0
      );
    }),
  );

  let agentNotificationStatus = $state<AgentNotificationSetupStatus | null>(null);
  let agentNotificationMessage = $state<string | null>(null);
  let agentNotificationError = $state<string | null>(null);
  let codexNotificationPreview = $state<CodexNotificationConfigPreview | null>(null);
  let agentNotificationBusy = $state<"refresh" | "claude" | "codex-preview" | "codex-configure" | null>(null);
  const claudeNotificationProvider = $derived(
    agentNotificationStatus?.providers.find((provider) => provider.provider === "claude") ?? null,
  );
  const codexNotificationProvider = $derived(
    agentNotificationStatus?.providers.find((provider) => provider.provider === "codex") ?? null,
  );
  let agentNotificationStatusRun = 0;

  onMount(() => {
    const run = ++agentNotificationStatusRun;
    const timer = setTimeout(() => void refreshAgentNotificationStatus(run), 250);
    return () => {
      agentNotificationStatusRun += 1;
      clearTimeout(timer);
    };
  });

  async function browseClaudeBinary() {
    const selected = await open({
      directory: false,
      title: "Select Claude Binary",
    });
    if (selected) updateSetting("claudeBinaryPath", selected as string);
  }

  function defaultAgentProfile(): string {
    return effectiveDefaultAgentProfileId($settings);
  }

  async function refreshAgentNotificationStatus(run: number) {
    try {
      const result = await commands.cmdAgentNotificationSetupStatus();
      if (run === agentNotificationStatusRun) {
        agentNotificationStatus = result;
        agentNotificationError = null;
      }
    } catch (e) {
      if (run === agentNotificationStatusRun) {
        agentNotificationStatus = null;
        agentNotificationError = e instanceof Error ? e.message : String(e);
      }
    }
  }

  async function configureClaudeNotifications() {
    agentNotificationBusy = "claude";
    agentNotificationError = null;
    agentNotificationMessage = null;
    try {
      const result = await commands.reinstallHooks();
      if (result.status === "error") throw new Error(result.error);
      agentNotificationMessage = "Claude Code hooks installed.";
      const run = ++agentNotificationStatusRun;
      await refreshAgentNotificationStatus(run);
    } catch (e) {
      agentNotificationError = e instanceof Error ? e.message : String(e);
    } finally {
      agentNotificationBusy = null;
    }
  }

  async function previewCodexNotifications() {
    agentNotificationBusy = "codex-preview";
    agentNotificationError = null;
    agentNotificationMessage = null;
    try {
      const result = await commands.cmdPreviewCodexNotificationConfig();
      if (result.status === "error") throw new Error(result.error);
      codexNotificationPreview = result.data;
      agentNotificationMessage = codexNotificationPreview.configured
        ? "Codex notifications are already configured."
        : "Codex config preview ready.";
    } catch (e) {
      codexNotificationPreview = null;
      agentNotificationError = e instanceof Error ? e.message : String(e);
    } finally {
      agentNotificationBusy = null;
    }
  }

  async function configureCodexNotifications() {
    agentNotificationBusy = "codex-configure";
    agentNotificationError = null;
    agentNotificationMessage = null;
    try {
      const result = await commands.cmdConfigureCodexNotificationConfig();
      if (result.status === "error") throw new Error(result.error);
      agentNotificationMessage = "Codex notifications configured.";
      const preview = await commands.cmdPreviewCodexNotificationConfig();
      if (preview.status === "ok") codexNotificationPreview = preview.data;
      const run = ++agentNotificationStatusRun;
      await refreshAgentNotificationStatus(run);
    } catch (e) {
      agentNotificationError = e instanceof Error ? e.message : String(e);
    } finally {
      agentNotificationBusy = null;
    }
  }

  function notificationProviderLabel(provider: AgentNotificationProviderStatus | null): string {
    if (!provider) return "checking";
    switch (provider.status) {
      case "installed":
        return "configured";
      case "missing":
        return "not configured";
      case "stale":
        return "needs update";
      case "error":
        return "error";
      case "unavailable":
        return "unavailable";
      default:
        return provider.status;
    }
  }

  function notificationProviderClass(provider: AgentNotificationProviderStatus | null): string {
    const status = provider?.status ?? "checking";
    switch (status) {
      case "installed":
        return "rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green";
      case "stale":
        return "rounded bg-amber/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-amber";
      case "error":
        return "rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red";
      default:
        return "rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted";
    }
  }
</script>

<div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-start justify-between gap-3">
    <div>
      <label for="settings-default-agent" class="text-[13px] font-semibold">Default agent</label>
      <div class="mt-0.5 text-[11px] text-text-muted">Used by new sessions, worktree starts, and Kanban cards unless a more specific profile is selected.</div>
    </div>
    <select
      id="settings-default-agent"
      aria-label="Default agent"
      class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6 max-w-[14rem]"
      value={defaultAgentProfile()}
      onchange={(e) => setDefaultAgentProfile(e.currentTarget.value)}
    >
      {#if autonomousProfiles.length === 0}
        <option value="claude">Claude</option>
      {:else}
        {#each autonomousProfiles as profile (profile.id)}
          <option value={profile.id}>{profile.name}</option>
        {/each}
      {/if}
    </select>
  </div>
</div>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="text-[13px] font-semibold">Claude</div>
  <div class="mt-3 flex items-center justify-between py-2">
    <div>
      <div class="text-[13px]">Binary path</div>
      <div class="text-[11px] text-text-muted mt-0.5">Leave blank to auto-detect from PATH</div>
    </div>
    <div class="flex gap-1">
      <input
        class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-48 text-right focus:border-accent-dim"
        value={$settings.claudeBinaryPath ?? ""}
        oninput={(e) => updateSetting("claudeBinaryPath", e.currentTarget.value || null)}
        placeholder="/usr/local/bin/claude"
      />
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        onclick={browseClaudeBinary}
      >...</button>
    </div>
  </div>
  <div class="flex items-center justify-between py-2">
    <span class="text-[13px]">Default model</span>
    <input
      class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-40 text-right focus:border-accent-dim"
      value={$settings.defaultModel ?? ""}
      oninput={(e) => updateSetting("defaultModel", e.currentTarget.value || null)}
      placeholder="opus"
    />
  </div>
  <div class="flex items-center justify-between py-2">
    <span class="text-[13px]">Additional flags</span>
    <input
      class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-56 text-right focus:border-accent-dim"
      value={$settings.additionalFlags.join(" ")}
      oninput={(e) => updateSetting("additionalFlags", e.currentTarget.value.split(" ").filter(Boolean))}
      placeholder="--verbose"
    />
  </div>
</div>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between gap-2">
    <div>
      <div class="text-[13px] font-semibold">Agent notifications</div>
      <div class="mt-0.5 text-[11px] text-text-muted">
        Configure Claude Code hooks and Codex TUI settings so agent events reach Roux.
      </div>
    </div>
    <button
      class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
      disabled={agentNotificationBusy !== null}
      onclick={() => {
        agentNotificationBusy = "refresh";
        agentNotificationMessage = null;
        const run = ++agentNotificationStatusRun;
        void refreshAgentNotificationStatus(run).finally(() => {
          if (agentNotificationBusy === "refresh") agentNotificationBusy = null;
        });
      }}
    >{agentNotificationBusy === "refresh" ? "Refreshing" : "Refresh"}</button>
  </div>

  <div class="mt-3 flex flex-col gap-2">
    <div class="rounded border border-border-subtle bg-bg-deep/60 p-2">
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <div class="text-[12px] font-medium">Claude Code</div>
          <div class="mt-0.5 text-[11px] text-text-muted">
            {claudeNotificationProvider?.detail ?? "Checking Claude Code hook setup."}
          </div>
        </div>
        <span class={notificationProviderClass(claudeNotificationProvider)}>
          {notificationProviderLabel(claudeNotificationProvider)}
        </span>
      </div>
      <div class="mt-2 flex gap-1">
        <button
          class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
          disabled={agentNotificationBusy !== null || !claudeNotificationProvider || claudeNotificationProvider.installable === false}
          onclick={configureClaudeNotifications}
        >{agentNotificationBusy === "claude" ? "Configuring" : (claudeNotificationProvider?.status === "installed" ? "Reinstall" : "Configure")}</button>
      </div>
    </div>

    <div class="rounded border border-border-subtle bg-bg-deep/60 p-2">
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <div class="text-[12px] font-medium">Codex</div>
          <div class="mt-0.5 text-[11px] text-text-muted">
            {codexNotificationProvider?.detail ?? "Checking Codex notification configuration."}
          </div>
          {#if codexNotificationProvider?.configPath}
            <div class="mt-1 max-w-[25rem] truncate font-mono text-[10px] text-text-muted" title={codexNotificationProvider.configPath}>
              {codexNotificationProvider.configPath}
            </div>
          {/if}
        </div>
        <span class={notificationProviderClass(codexNotificationProvider)}>
          {notificationProviderLabel(codexNotificationProvider)}
        </span>
      </div>
      <div class="mt-2 flex gap-1">
        <button
          class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
          disabled={agentNotificationBusy !== null || !codexNotificationProvider || codexNotificationProvider.installable === false}
          onclick={previewCodexNotifications}
        >{agentNotificationBusy === "codex-preview" ? "Previewing" : "Preview"}</button>
        <button
          class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
          disabled={agentNotificationBusy !== null || !codexNotificationProvider || codexNotificationProvider.installable === false}
          onclick={configureCodexNotifications}
        >{agentNotificationBusy === "codex-configure" ? "Configuring" : "Configure"}</button>
      </div>

      {#if codexNotificationPreview}
        <div class="mt-2 rounded border border-border-subtle bg-bg-deep/70 p-2">
          <div class="mb-1 flex items-center justify-between gap-2 text-[10px] uppercase tracking-wider text-text-muted">
            <span>Codex config preview</span>
            <span class="truncate font-mono normal-case tracking-normal" title={codexNotificationPreview.configPath}>
              {codexNotificationPreview.configPath}
            </span>
          </div>
          <pre class="app-scrollbar max-h-32 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] text-text-secondary">{codexNotificationPreview.nextContent}</pre>
        </div>
      {/if}
    </div>
  </div>

  {#if agentNotificationMessage}
    <div class="mt-2 text-[11px] text-green">{agentNotificationMessage}</div>
  {/if}
  {#if agentNotificationError}
    <div class="mt-2 text-[11px] text-red">{agentNotificationError}</div>
  {/if}
</div>
