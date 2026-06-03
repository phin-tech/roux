<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { commands } from "$lib/bindings";
  import type {
    IntegrationDetection,
    McpHostConfigPreview,
    McpStatus,
    WorktreeProvider,
    WorktrunkDetection,
  } from "$lib/bindings";
  import { settings, updateSetting } from "$lib/stores/settings";

  let ghDetection = $state<IntegrationDetection | null>(null);
  let gitDetection = $state<IntegrationDetection | null>(null);
  let worktrunkDetection = $state<WorktrunkDetection | null>(null);
  let mcpStatus = $state<McpStatus | null>(null);
  let mcpPreviewByHost = $state<Record<string, McpHostConfigPreview | null>>(
    {},
  );
  let mcpMessageByHost = $state<Record<string, string | null>>({});
  let mcpErrorByHost = $state<Record<string, string | null>>({});
  let mcpBusyByHost = $state<Record<string, "preview" | "configure" | null>>(
    {},
  );
  let ghDetectionRun = 0;
  let gitDetectionRun = 0;
  let worktrunkDetectionRun = 0;
  let mcpStatusRun = 0;

  type McpHostIdT = "claudeDesktop" | "claudeCode" | "codex";

  async function browseGhBinary() {
    const selected = await open({
      directory: false,
      title: "Select gh (GitHub CLI) Binary",
    });
    if (selected) updateSetting("ghBinaryPath", selected as string);
  }

  async function browseGitBinary() {
    const selected = await open({
      directory: false,
      title: "Select git Binary",
    });
    if (selected) updateSetting("gitBinaryPath", selected as string);
  }

  async function browseWorktrunkBinary() {
    const selected = await open({
      directory: false,
      title: "Select wt (worktrunk) Binary",
    });
    if (selected) updateSetting("worktrunkBinaryPath", selected as string);
  }

  async function refreshGhDetection(run: number) {
    try {
      const result = await commands.cmdDetectGh();
      if (run === ghDetectionRun) ghDetection = result;
    } catch {
      if (run === ghDetectionRun)
        ghDetection = { binaryPath: null, version: null };
    }
  }

  async function refreshGitDetection(run: number) {
    try {
      const result = await commands.cmdDetectGit();
      if (run === gitDetectionRun) gitDetection = result;
    } catch {
      if (run === gitDetectionRun)
        gitDetection = { binaryPath: null, version: null };
    }
  }

  async function refreshWorktrunkDetection(run: number) {
    try {
      const result = await commands.cmdDetectWorktrunk(null);
      if (run === worktrunkDetectionRun) worktrunkDetection = result;
    } catch {
      if (run === worktrunkDetectionRun) {
        worktrunkDetection = {
          binaryPath: null,
          version: null,
          hasConfig: false,
        };
      }
    }
  }

  async function refreshMcpStatus(run: number) {
    try {
      const result = await commands.cmdMcpStatus();
      if (run === mcpStatusRun) mcpStatus = result;
    } catch {
      if (run === mcpStatusRun) mcpStatus = null;
    }
  }

  $effect(() => {
    const path = $settings.ghBinaryPath;
    void path;
    const run = ++ghDetectionRun;
    const timer = setTimeout(() => void refreshGhDetection(run), 250);
    return () => {
      ghDetectionRun += 1;
      clearTimeout(timer);
    };
  });

  $effect(() => {
    const path = $settings.gitBinaryPath;
    void path;
    const run = ++gitDetectionRun;
    const timer = setTimeout(() => void refreshGitDetection(run), 250);
    return () => {
      gitDetectionRun += 1;
      clearTimeout(timer);
    };
  });

  $effect(() => {
    const path = $settings.worktrunkBinaryPath;
    void path;
    const run = ++worktrunkDetectionRun;
    const timer = setTimeout(() => void refreshWorktrunkDetection(run), 250);
    return () => {
      worktrunkDetectionRun += 1;
      clearTimeout(timer);
    };
  });

  $effect(() => {
    const enabled = $settings.mcpEnabled;
    void enabled;
    const run = ++mcpStatusRun;
    const timer = setTimeout(() => void refreshMcpStatus(run), 250);
    return () => {
      mcpStatusRun += 1;
      clearTimeout(timer);
    };
  });

  async function previewMcpHostConfig(hostId: McpHostIdT, label: string) {
    mcpBusyByHost = { ...mcpBusyByHost, [hostId]: "preview" };
    mcpErrorByHost = { ...mcpErrorByHost, [hostId]: null };
    mcpMessageByHost = { ...mcpMessageByHost, [hostId]: null };
    try {
      const result = await commands.cmdPreviewMcpHostConfig(hostId);
      if (result.status === "error") {
        throw new Error(result.error);
      }
      mcpPreviewByHost = { ...mcpPreviewByHost, [hostId]: result.data };
      mcpMessageByHost = {
        ...mcpMessageByHost,
        [hostId]: result.data.configured
          ? `${label} is already configured.`
          : "Preview ready.",
      };
    } catch (e) {
      mcpPreviewByHost = { ...mcpPreviewByHost, [hostId]: null };
      mcpErrorByHost = {
        ...mcpErrorByHost,
        [hostId]: e instanceof Error ? e.message : String(e),
      };
    } finally {
      mcpBusyByHost = { ...mcpBusyByHost, [hostId]: null };
    }
  }

  async function configureMcpHost(hostId: McpHostIdT, label: string) {
    mcpBusyByHost = { ...mcpBusyByHost, [hostId]: "configure" };
    mcpErrorByHost = { ...mcpErrorByHost, [hostId]: null };
    mcpMessageByHost = { ...mcpMessageByHost, [hostId]: null };
    try {
      const result = await commands.cmdConfigureMcpHost(hostId);
      if (result.status === "error") {
        throw new Error(result.error);
      }
      mcpPreviewByHost = { ...mcpPreviewByHost, [hostId]: result.data };
      mcpMessageByHost = {
        ...mcpMessageByHost,
        [hostId]: result.data.configured
          ? `${label} was already configured.`
          : `${label} configuration updated.`,
      };
      const run = ++mcpStatusRun;
      await refreshMcpStatus(run);
    } catch (e) {
      mcpErrorByHost = {
        ...mcpErrorByHost,
        [hostId]: e instanceof Error ? e.message : String(e),
      };
    } finally {
      mcpBusyByHost = { ...mcpBusyByHost, [hostId]: null };
    }
  }

  function formatMcpConfiguredAt(ms: number | null | undefined): string {
    if (!ms) return "Never";
    return new Date(ms).toLocaleString();
  }
</script>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between">
    <div class="text-[13px] font-semibold">Roux MCP</div>
    {#if mcpStatus?.enabled}
      <span
        class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
        >enabled</span
      >
    {:else}
      <span
        class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted"
        >off</span
      >
    {/if}
  </div>
  <div class="mt-0.5 text-[11px] text-text-muted">
    Lets MCP clients launch <code class="font-mono">roux mcp</code> and use Roux sessions,
    panes, and notes through the running app.
  </div>
  <div class="mt-3 flex items-center justify-between py-1">
    <div>
      <div class="text-[13px]">Enable Roux MCP</div>
      <div class="mt-0.5 text-[11px] text-text-muted">
        Safe action tools are available by default; destructive tools are not
        exposed.
      </div>
    </div>
    <button
      aria-label="Toggle Roux MCP"
      class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
        {($settings.mcpEnabled ?? false)
        ? 'bg-accent-dim border-accent'
        : 'bg-bg-deep border-border'}"
      onclick={() =>
        updateSetting("mcpEnabled", !($settings.mcpEnabled ?? false))}
    >
      <div
        class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
        {($settings.mcpEnabled ?? false)
          ? 'left-[18px] bg-accent'
          : 'left-0.5 bg-text-secondary'}"
      ></div>
    </button>
  </div>
  {#if mcpStatus}
    <div class="mt-2 grid grid-cols-2 gap-2 text-[11px]">
      <div class="rounded border border-border-subtle bg-bg-deep/70 px-2 py-1">
        <div class="text-text-muted">CLI</div>
        <div
          class={mcpStatus.cliInstalled && mcpStatus.cliCurrent
            ? "text-green"
            : "text-red"}
        >
          {mcpStatus.cliInstalled
            ? mcpStatus.cliCurrent
              ? "current"
              : "stale"
            : "missing"}
        </div>
      </div>
      <div class="rounded border border-border-subtle bg-bg-deep/70 px-2 py-1">
        <div class="text-text-muted">Server path</div>
        <div
          class="truncate font-mono text-text-secondary"
          title={mcpStatus.cliPath}
        >
          {mcpStatus.cliPath}
        </div>
      </div>
      <div
        class="col-span-2 rounded border border-border-subtle bg-bg-deep/70 px-2 py-1"
      >
        <div class="text-text-muted">Last config update</div>
        <div class="text-text-secondary">
          {mcpStatus.lastConfiguredAtMs
            ? formatMcpConfiguredAt(mcpStatus.lastConfiguredAtMs)
            : "Never"}
          {mcpStatus.lastConfiguredHost
            ? ` · ${mcpStatus.lastConfiguredHost}`
            : ""}
        </div>
      </div>
    </div>
  {/if}
  {#if mcpStatus}
    {#each mcpStatus.hosts as host (host.id)}
      {@const hostId = host.id as McpHostIdT}
      {@const busy = mcpBusyByHost[hostId] ?? null}
      {@const message = mcpMessageByHost[hostId] ?? null}
      {@const error = mcpErrorByHost[hostId] ?? null}
      {@const preview = mcpPreviewByHost[hostId] ?? null}
      <div class="mt-3 rounded border border-border-subtle bg-bg-deep/60 p-2">
        <div class="flex items-center justify-between gap-2">
          <div>
            <div class="text-[12px] font-medium">{host.label}</div>
            <div
              class="mt-0.5 max-w-[22rem] truncate font-mono text-[10px] text-text-muted"
              title={host.configPath ?? ""}
            >
              {host.configPath ?? "Config path unavailable"}
            </div>
          </div>
          {#if host.configured}
            <span
              class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
              >configured</span
            >
          {:else if host.error}
            <span
              class="rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red"
              >needs attention</span
            >
          {:else}
            <span
              class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted"
              >not configured</span
            >
          {/if}
        </div>
        <div class="mt-2 flex gap-1">
          <button
            class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
            disabled={!($settings.mcpEnabled ?? false) || busy !== null}
            onclick={() => previewMcpHostConfig(hostId, host.label)}
            >{busy === "preview" ? "Previewing" : "Preview"}</button
          >
          <button
            class="rounded border border-accent-dim bg-accent/15 px-2 py-1 text-[11px] text-text-primary hover:bg-accent/25 disabled:cursor-not-allowed disabled:opacity-40"
            disabled={!($settings.mcpEnabled ?? false) || busy !== null}
            onclick={() => configureMcpHost(hostId, host.label)}
            >{busy === "configure"
              ? "Adding..."
              : `Add to ${host.label}`}</button
          >
        </div>
        {#if message}
          <div class="mt-2 text-[11px] text-green">{message}</div>
        {/if}
        {#if error || host.error}
          <div class="mt-2 text-[11px] text-red">{error ?? host.error}</div>
        {/if}
        {#if preview}
          <div
            class="mt-2 rounded border border-border-subtle bg-bg-deep/70 p-2"
          >
            <div
              class="mb-1 text-[10px] uppercase tracking-wider text-text-muted"
            >
              Roux entry
            </div>
            <pre
              class="app-scrollbar max-h-28 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] text-text-secondary">{preview.nextEntryJson}</pre>
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</div>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between">
    <div class="text-[13px] font-semibold">GitHub CLI</div>
    {#if ghDetection?.binaryPath}
      <span
        class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
        >detected{ghDetection.version ? ` ${ghDetection.version}` : ""}</span
      >
    {:else if ghDetection !== null}
      <span
        class="rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red"
        >missing</span
      >
    {/if}
  </div>
  <div class="mt-0.5 text-[11px] text-text-muted">
    Used for "Session from PR" and PR watches. Roux auto-detects
    <code class="font-mono">gh</code> via your login shell's PATH (including
    fish). Set this only if auto-detection misses your install - paste the
    output of <code class="font-mono">which gh</code>.
  </div>
  {#if ghDetection?.binaryPath}
    <div class="mt-2 font-mono text-[10px] text-text-muted">
      {ghDetection.binaryPath}
    </div>
  {/if}
  <div class="mt-3 flex items-center justify-between gap-2">
    <span class="text-[13px]">Binary path</span>
    <div class="flex gap-1">
      <input
        class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
        value={$settings.ghBinaryPath ?? ""}
        oninput={(e) =>
          updateSetting("ghBinaryPath", e.currentTarget.value || null)}
        placeholder="/opt/homebrew/bin/gh"
      />
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        onclick={browseGhBinary}>...</button
      >
    </div>
  </div>
  <div class="mt-3 flex items-center justify-between py-1">
    <div>
      <div class="text-[13px]">Look up PR for session branch</div>
      <div class="text-[11px] text-text-muted mt-0.5">
        Run <code class="font-mono">gh pr list --head &lt;branch&gt;</code> on session
        activation so the status bar can show a clickable PR link.
      </div>
    </div>
    <button
      aria-label="Toggle session PR lookup"
      class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
        {$settings.autoLookupSessionPr !== false
        ? 'bg-accent-dim border-accent'
        : 'bg-bg-deep border-border'}"
      onclick={() =>
        updateSetting(
          "autoLookupSessionPr",
          $settings.autoLookupSessionPr === false,
        )}
    >
      <div
        class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
        {$settings.autoLookupSessionPr !== false
          ? 'left-[18px] bg-accent'
          : 'left-0.5 bg-text-secondary'}"
      ></div>
    </button>
  </div>
  <div class="flex items-center justify-between py-1">
    <div>
      <div class="text-[13px]">Auto-create PR watch for sessions</div>
      <div class="text-[11px] text-text-muted mt-0.5">
        When the lookup finds a PR, create a session-scoped GitHub PR watch
        automatically. Requires the lookup above.
      </div>
    </div>
    <button
      aria-label="Toggle auto PR watch"
      disabled={$settings.autoLookupSessionPr === false}
      class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
        {$settings.autoWatchSessionPr
        ? 'bg-accent-dim border-accent'
        : 'bg-bg-deep border-border'}
        {$settings.autoLookupSessionPr === false
        ? 'opacity-40 cursor-not-allowed'
        : ''}"
      onclick={() =>
        updateSetting("autoWatchSessionPr", !$settings.autoWatchSessionPr)}
    >
      <div
        class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
        {$settings.autoWatchSessionPr
          ? 'left-[18px] bg-accent'
          : 'left-0.5 bg-text-secondary'}"
      ></div>
    </button>
  </div>
</div>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between">
    <div class="text-[13px] font-semibold">Git</div>
    {#if gitDetection?.binaryPath}
      <span
        class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
        >detected{gitDetection.version ? ` ${gitDetection.version}` : ""}</span
      >
    {:else if gitDetection !== null}
      <span
        class="rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red"
        >missing</span
      >
    {/if}
  </div>
  <div class="mt-0.5 text-[11px] text-text-muted">
    Used for git-backed Library sources. Roux checks this override, then <code
      class="font-mono">ROUX_GIT</code
    >, then your login shell's PATH, then the app PATH. Set this only if
    auto-detection misses your install.
  </div>
  {#if gitDetection?.binaryPath}
    <div class="mt-2 font-mono text-[10px] text-text-muted">
      {gitDetection.binaryPath}
    </div>
  {/if}
  <div class="mt-3 flex items-center justify-between gap-2">
    <span class="text-[13px]">Binary path</span>
    <div class="flex gap-1">
      <input
        class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
        value={$settings.gitBinaryPath ?? ""}
        oninput={(e) =>
          updateSetting("gitBinaryPath", e.currentTarget.value || null)}
        placeholder="/opt/homebrew/bin/git"
      />
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        onclick={browseGitBinary}>...</button
      >
    </div>
  </div>
</div>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between">
    <div class="text-[13px] font-semibold">Worktrunk (wt)</div>
    {#if worktrunkDetection?.binaryPath}
      <span
        data-testid="worktrunk-detected-badge"
        class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
        >detected{worktrunkDetection.version
          ? ` ${worktrunkDetection.version}`
          : ""}</span
      >
    {:else}
      <span
        data-testid="worktrunk-not-detected-badge"
        class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted"
        >not detected</span
      >
    {/if}
  </div>
  <div class="mt-0.5 text-[11px] text-text-muted">
    When available, Roux enriches the New Session worktree picker with <code
      class="font-mono">wt</code
    >'s richer metadata (dirty state, ahead/behind, locked/prunable,
    current/previous). Opt-in - no regression for users without
    <code class="font-mono">wt</code>. Set the path only if auto-detection
    misses your install.
  </div>
  {#if worktrunkDetection?.binaryPath}
    <div class="mt-2 font-mono text-[10px] text-text-muted">
      {worktrunkDetection.binaryPath}
    </div>
  {/if}
  <div class="mt-3 flex items-center justify-between gap-2">
    <span class="text-[13px]">Binary path</span>
    <div class="flex gap-1">
      <input
        class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
        value={$settings.worktrunkBinaryPath ?? ""}
        oninput={(e) =>
          updateSetting("worktrunkBinaryPath", e.currentTarget.value || null)}
        placeholder="/opt/homebrew/bin/wt"
      />
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        onclick={browseWorktrunkBinary}>...</button
      >
    </div>
  </div>
  <div class="mt-3 flex items-center justify-between gap-2">
    <div>
      <div class="text-[13px]">Worktree provider</div>
      <div class="mt-0.5 text-[11px] text-text-muted">
        How Roux creates new worktrees. Auto uses <code class="font-mono"
          >wt</code
        > when detected and falls back to git otherwise.
      </div>
    </div>
    <div
      data-testid="worktrunk-provider-selector"
      class="flex overflow-hidden rounded border border-border bg-bg-deep"
    >
      {#each [{ id: "auto", label: "Auto" }, { id: "git", label: "Git" }, { id: "worktrunk", label: "wt" }] as const as opt}
        {@const active = ($settings.worktreeProvider ?? "auto") === opt.id}
        <button
          data-testid={`worktrunk-provider-${opt.id}`}
          class="cursor-pointer px-2.5 py-1 text-[11px] transition-colors
            {active
            ? 'bg-accent-dim text-text-primary'
            : 'text-text-secondary hover:bg-bg-hover'}"
          onclick={() =>
            updateSetting("worktreeProvider", opt.id as WorktreeProvider)}
          >{opt.label}</button
        >
      {/each}
    </div>
  </div>
</div>
