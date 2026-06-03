<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { commands } from "$lib/bindings";
  import type {
    OnPaneCloseMode,
    WorktreeCleanupMode,
    WorktreeDefaultBase,
  } from "$lib/bindings";
  import { settings, updateSetting } from "$lib/stores/settings";

  const PANE_CLOSE_OPTIONS = [
    { id: "kill", label: "Kill" },
    { id: "detach", label: "Detach" },
  ] as const;

  const PREVIEW_DEBOUNCE_MS = 200;
  const PREVIEW_FALLBACK = "~/src/my-project";

  let repoRootDraft = $state("");
  let previewText = $state<string>("");

  async function browseWorktreeBase() {
    const selected = await open({
      directory: true,
      title: "Select Worktree Base Directory",
    });
    if (selected) updateSetting("worktreeBasePath", selected as string);
  }

  async function browseDefaultProject() {
    const selected = await open({
      directory: true,
      title: "Select Default Project Directory",
    });
    if (selected) updateSetting("defaultProjectPath", selected as string);
  }

  function sanitizePathInput(path: string): string {
    return path.trim();
  }

  function addRepoRoot(path: string) {
    const nextPath = sanitizePathInput(path);
    if (!nextPath) return;
    const existing = $settings.repoRoots ?? [];
    if (existing.includes(nextPath)) {
      repoRootDraft = "";
      return;
    }
    updateSetting("repoRoots", [...existing, nextPath]);
    repoRootDraft = "";
  }

  function removeRepoRoot(path: string) {
    updateSetting(
      "repoRoots",
      ($settings.repoRoots ?? []).filter((root) => root !== path),
    );
  }

  function previewRepoPath(): string {
    const roots = $settings.repoRoots ?? [];
    if (roots.length > 0) return roots[0];
    if ($settings.defaultProjectPath) return $settings.defaultProjectPath;
    return PREVIEW_FALLBACK;
  }

  $effect(() => {
    const tpl = $settings.worktreeBasePath ?? "";
    const repo = previewRepoPath();
    let stale = false;
    const timer = setTimeout(() => {
      commands
        .cmdPreviewWorktreeBase(tpl, repo)
        .then((r) => {
          if (!stale) previewText = r;
        })
        .catch(() => {
          if (!stale) previewText = "";
        });
    }, PREVIEW_DEBOUNCE_MS);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  });

  function setCleanupMode(mode: WorktreeCleanupMode) {
    updateSetting("worktreeCleanupOnClose", mode);
    // Keep the legacy boolean in sync so older settings readers agree.
    updateSetting("cleanupWorktreesOnClose", mode === "always");
  }

  function setDefaultBase(mode: WorktreeDefaultBase) {
    updateSetting("worktreeDefaultBase", mode);
  }

  function setPaneCloseMode(mode: OnPaneCloseMode) {
    updateSetting("onPaneClose", mode);
  }

  async function browseAndAddRepoRoot() {
    const selected = await open({
      directory: true,
      title: "Select Repo Root Directory",
    });
    if (selected) addRepoRoot(selected as string);
  }
</script>

<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Confirm on close</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Prompt before closing active sessions
    </div>
  </div>
  <button
    aria-label="Toggle confirm on close"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {$settings.confirmOnClose
      ? 'bg-accent-dim border-accent'
      : 'bg-bg-deep border-border'}"
    onclick={() => updateSetting("confirmOnClose", !$settings.confirmOnClose)}
  >
    <div
      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {$settings.confirmOnClose
        ? 'left-[18px] bg-accent'
        : 'left-0.5 bg-text-secondary'}"
    ></div>
  </button>
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Restore on launch</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Show previous sessions on startup
    </div>
  </div>
  <button
    aria-label="Toggle restore sessions on launch"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {$settings.restoreSessionsOnLaunch
      ? 'bg-accent-dim border-accent'
      : 'bg-bg-deep border-border'}"
    onclick={() =>
      updateSetting(
        "restoreSessionsOnLaunch",
        !$settings.restoreSessionsOnLaunch,
      )}
  >
    <div
      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {$settings.restoreSessionsOnLaunch
        ? 'left-[18px] bg-accent'
        : 'left-0.5 bg-text-secondary'}"
    ></div>
  </button>
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">On pane close</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Kill the terminal by default, or keep it running detached for later
      reconnect.
    </div>
  </div>
  <div class="flex overflow-hidden rounded border border-border bg-bg-deep">
    {#each PANE_CLOSE_OPTIONS as opt}
      {@const active = ($settings.onPaneClose ?? "kill") === opt.id}
      <button
        class="px-2.5 py-1 text-[11px] cursor-pointer transition-colors
          {active
          ? 'bg-accent-dim text-text-primary'
          : 'text-text-secondary hover:bg-bg-hover'}"
        aria-pressed={active}
        onclick={() => setPaneCloseMode(opt.id)}>{opt.label}</button
      >
    {/each}
  </div>
</div>
<div class="flex items-center justify-between py-2">
  <span class="text-[13px]">Default project path</span>
  <div class="flex gap-1">
    <input
      class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-48 text-right focus:border-accent-dim"
      value={$settings.defaultProjectPath ?? ""}
      oninput={(e) =>
        updateSetting("defaultProjectPath", e.currentTarget.value || null)}
      placeholder="~/src"
    />
    <button
      class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
      onclick={browseDefaultProject}>...</button
    >
  </div>
</div>
<div class="py-2">
  <div class="text-[13px]">Repository roots</div>
  <div class="text-[11px] text-text-muted mt-0.5">
    Quick-pick sources for New Session (keeps file picker available)
  </div>
  <div class="mt-2 flex gap-1">
    <input
      class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none flex-1 focus:border-accent-dim"
      value={repoRootDraft}
      oninput={(e) => (repoRootDraft = e.currentTarget.value)}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          addRepoRoot(repoRootDraft);
        }
      }}
      placeholder="~/src"
    />
    <button
      class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
      onclick={() => addRepoRoot(repoRootDraft)}>Add</button
    >
    <button
      class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
      onclick={browseAndAddRepoRoot}>...</button
    >
  </div>
  {#if ($settings.repoRoots ?? []).length > 0}
    <div class="mt-2 flex flex-col gap-1">
      {#each $settings.repoRoots ?? [] as root (root)}
        <div
          class="flex items-center gap-2 rounded border border-border-subtle bg-bg-surface/35 px-2 py-1"
        >
          <span
            class="font-mono text-[11px] text-text-secondary flex-1 truncate"
            title={root}>{root}</span
          >
          <button
            class="text-[10px] text-text-muted hover:text-red cursor-pointer"
            onclick={() => removeRepoRoot(root)}>Remove</button
          >
        </div>
      {/each}
    </div>
  {/if}
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Exclude worktrees from roots</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Hide linked git worktrees from root-folder quick-pick results
    </div>
  </div>
  <button
    aria-label="Toggle excluding worktrees from root discovery"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {($settings.excludeWorktreesFromRepoRoots ?? true)
      ? 'bg-accent-dim border-accent'
      : 'bg-bg-deep border-border'}"
    onclick={() =>
      updateSetting(
        "excludeWorktreesFromRepoRoots",
        !($settings.excludeWorktreesFromRepoRoots ?? true),
      )}
  >
    <div
      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {($settings.excludeWorktreesFromRepoRoots ?? true)
        ? 'left-[18px] bg-accent'
        : 'left-0.5 bg-text-secondary'}"
    ></div>
  </button>
</div>
<div class="py-2">
  <div class="flex items-center justify-between">
    <div>
      <div class="text-[13px]">Worktree base path</div>
      <div class="text-[11px] text-text-muted mt-0.5">
        Where to create new worktrees. Supports
        <code class="font-mono">{"{project_dir}"}</code>,
        <code class="font-mono">{"{git_root}"}</code>,
        <code class="font-mono">{"{project_name}"}</code>,
        <code class="font-mono">{"{home}"}</code>.
      </div>
    </div>
    <div class="flex gap-1">
      <input
        class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
        value={$settings.worktreeBasePath ?? ""}
        oninput={(e) =>
          updateSetting("worktreeBasePath", e.currentTarget.value || null)}
        placeholder="{'{project_dir}'}/.worktrees"
      />
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        onclick={browseWorktreeBase}>...</button
      >
    </div>
  </div>
  {#if previewText}
    <div
      class="mt-1.5 text-[11px] text-text-muted font-mono truncate"
      title={previewText}
    >
      -> {previewText}
    </div>
  {/if}
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">On session close</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      What to do with the session's worktree
    </div>
  </div>
  <div class="flex rounded border border-border bg-bg-deep overflow-hidden">
    {#each [{ id: "never", label: "Keep" }, { id: "prompt", label: "Ask" }, { id: "always", label: "Remove" }] as const as opt}
      {@const active =
        ($settings.worktreeCleanupOnClose ?? "prompt") === opt.id}
      <button
        class="px-2.5 py-1 text-[11px] cursor-pointer transition-colors
          {active
          ? 'bg-accent-dim text-text-primary'
          : 'text-text-secondary hover:bg-bg-hover'}"
        onclick={() => setCleanupMode(opt.id)}>{opt.label}</button
      >
    {/each}
  </div>
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">New Worktree default</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Default starting point for new worktree branches - applies to the New
      Session dialog and the "New Worktree" context-menu click. Hover / command
      palette always expose all three.
    </div>
  </div>
  <select
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
    value={$settings.worktreeDefaultBase ?? "currentBranch"}
    onchange={(e) =>
      setDefaultBase(e.currentTarget.value as WorktreeDefaultBase)}
  >
    <option value="currentBranch">Current branch</option>
    <option value="main">main</option>
    <option value="originMain">origin/main</option>
  </select>
</div>
