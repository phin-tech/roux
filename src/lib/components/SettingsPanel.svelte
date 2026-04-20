<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";
  import { open } from "@tauri-apps/plugin-dialog";
  import { THEME_DEFINITIONS } from "$lib/themes";
  import { getLogPath, setLoggingEnabled } from "$lib/logging";
  import { notificationsPush } from "$lib/tauri";
  import { commands } from "$lib/bindings";
  import type { UpdateChannel, WorktreeCleanupMode, WorktreeDefaultBase } from "$lib/bindings";
  import { updateStatus, runManualCheck, performInstall } from "$lib/stores/updater";
  import { getVersion } from "@tauri-apps/api/app";
  import { quitApp } from "$lib/tauri";
  import { onMount } from "svelte";
  import Settings from "@lucide/svelte/icons/settings";
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import TerminalIcon from "@lucide/svelte/icons/terminal";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Bell from "@lucide/svelte/icons/bell";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Wrench from "@lucide/svelte/icons/wrench";
  import Plug from "@lucide/svelte/icons/plug";
  import NotebookPen from "@lucide/svelte/icons/notebook-pen";
  import X from "@lucide/svelte/icons/x";
  import DoctorPanel from "$lib/components/DoctorPanel.svelte";

  type CategoryId = "general" | "sessions" | "terminal" | "claude" | "notes" | "integrations" | "notifications" | "keyboard" | "advanced";

  const CATEGORIES: { id: CategoryId; label: string; icon: typeof Settings }[] = [
    { id: "general", label: "General", icon: Settings },
    { id: "sessions", label: "Sessions", icon: FolderTree },
    { id: "terminal", label: "Terminal", icon: TerminalIcon },
    { id: "claude", label: "Claude", icon: Sparkles },
    { id: "notes", label: "Notes", icon: NotebookPen },
    { id: "integrations", label: "Integrations", icon: Plug },
    { id: "notifications", label: "Notifications", icon: Bell },
    { id: "keyboard", label: "Keyboard", icon: Keyboard },
    { id: "advanced", label: "Advanced", icon: Wrench },
  ];

  let selected = $state<CategoryId>("general");

  let appVersion = $state<string>("…");
  let repoRootDraft = $state("");
  onMount(async () => {
    try { appVersion = await getVersion(); } catch { appVersion = "unknown"; }
  });

  function describeError(reason: "network" | "signature-invalid" | "unknown"): string {
    switch (reason) {
      case "network": return "Couldn't reach the update server.";
      case "signature-invalid": return "Update signature invalid. Download blocked.";
      case "unknown": return "Update check failed.";
    }
  }

  let notifTestStatus = $state<"idle" | "sent" | "error">("idle");
  let notifTestError = $state<string | null>(null);

  async function sendTestNotification() {
    notifTestStatus = "idle";
    notifTestError = null;
    try {
      await notificationsPush({
        level: "attention",
        source: { type: "cli" },
        title: "Roux notification test",
        subtitle: null,
        body: "If you saw a macOS notification, permissions are set up correctly.",
        sessionId: null,
        actions: [],
        dedupKey: null,
      });
      notifTestStatus = "sent";
    } catch (e) {
      notifTestStatus = "error";
      notifTestError = e instanceof Error ? e.message : String(e);
    }
  }

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();

  $effect(() => {
    if (visible) selected = "general";
  });

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  async function browseClaudeBinary() {
    const selected = await open({ directory: false, title: "Select Claude Binary" });
    if (selected) updateSetting("claudeBinaryPath", selected as string);
  }

  async function browseGhBinary() {
    const selected = await open({ directory: false, title: "Select gh (GitHub CLI) Binary" });
    if (selected) updateSetting("ghBinaryPath", selected as string);
  }

  async function browseWorktreeBase() {
    const selected = await open({ directory: true, title: "Select Worktree Base Directory" });
    if (selected) updateSetting("worktreeBasePath", selected as string);
  }

  async function browseDefaultProject() {
    const selected = await open({ directory: true, title: "Select Default Project Directory" });
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

  // Pick a repo path to use for the Settings worktree-base-path preview.
  // Falls back to an illustrative placeholder when the user hasn't
  // configured any repo roots or a default project yet — the preview is
  // purely informational, so a made-up path is fine.
  const PREVIEW_FALLBACK = "/Users/you/src/my-project";
  let previewText = $state<string>("");
  function previewRepoPath(): string {
    const roots = $settings.repoRoots ?? [];
    if (roots.length > 0) return roots[0];
    if ($settings.defaultProjectPath) return $settings.defaultProjectPath;
    return PREVIEW_FALLBACK;
  }
  $effect(() => {
    const tpl = $settings.worktreeBasePath ?? "";
    const repo = previewRepoPath();
    // Cancel-on-stale guard: only keep the latest preview result.
    let stale = false;
    commands
      .cmdPreviewWorktreeBase(tpl, repo)
      .then((r) => { if (!stale) previewText = r; })
      .catch(() => { if (!stale) previewText = ""; });
    return () => { stale = true; };
  });

  function setCleanupMode(mode: WorktreeCleanupMode) {
    updateSetting("worktreeCleanupOnClose", mode);
    // Keep the legacy boolean in sync so any older readers (settings files,
    // pre-migration code) still agree with the frontend.
    updateSetting("cleanupWorktreesOnClose", mode === "always");
  }

  function setDefaultBase(mode: WorktreeDefaultBase) {
    updateSetting("worktreeDefaultBase", mode);
  }

  async function browseAndAddRepoRoot() {
    const selected = await open({ directory: true, title: "Select Repo Root Directory" });
    if (selected) addRepoRoot(selected as string);
  }

  async function browseNotesVault() {
    const selected = await open({ directory: true, title: "Select Notes Vault Location" });
    if (selected) updateSetting("notesVaultRoot", selected as string);
  }
</script>

<svelte:window onkeydown={visible ? handleKey : undefined} />

{#if visible}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
    role="presentation"
    onclick={onclose}
  >
    <div
      class="flex h-[520px] w-[720px] overflow-hidden rounded-2xl border border-hairline bg-bg-deep shadow-[0_24px_64px_rgba(2,6,23,0.6),0_0_0_1px_rgba(255,255,255,0.04)]"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      tabindex="-1"
    >
      <!-- Sidebar -->
      <aside class="flex w-[180px] shrink-0 flex-col border-r border-hairline bg-bg-surface/30 py-3">
        <div class="px-3 pb-2 text-[11px] font-semibold uppercase tracking-widest text-text-muted">Settings</div>
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

      <!-- Detail pane -->
      <div class="flex min-w-0 flex-1 flex-col">
        <div class="flex h-10 shrink-0 items-center justify-between border-b border-hairline px-4">
          <h2 class="text-sm font-semibold tracking-tight">
            {CATEGORIES.find((c) => c.id === selected)?.label}
          </h2>
          <button
            aria-label="Close settings"
            class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1 text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
            onclick={onclose}
          >
            <X size={14} />
          </button>
        </div>

        <div class="app-scrollbar flex-1 overflow-y-auto px-5 py-4">
          {#if selected === "general"}
            <div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-start justify-between gap-3">
                <div>
                  <div class="text-[13px]">Theme</div>
                  <div class="text-[11px] text-text-muted mt-0.5">Choose the color preset for the app chrome and terminals</div>
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
              <span class="text-[13px]">Tab position</span>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$settings.tabPosition}
                onchange={(e) => updateSetting("tabPosition", e.currentTarget.value as "left" | "right")}
              >
                <option value="left">Left</option>
                <option value="right">Right</option>
              </select>
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Status bar position</span>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$settings.statusBarPosition ?? "bottom"}
                onchange={(e) => updateSetting("statusBarPosition", e.currentTarget.value as "top" | "bottom")}
              >
                <option value="top">Top</option>
                <option value="bottom">Bottom</option>
              </select>
            </div>
          {:else if selected === "sessions"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Confirm on close</div>
                <div class="text-[11px] text-text-muted mt-0.5">Prompt before closing active sessions</div>
              </div>
              <button
                aria-label="Toggle confirm on close"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.confirmOnClose ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("confirmOnClose", !$settings.confirmOnClose)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.confirmOnClose ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Restore on launch</div>
                <div class="text-[11px] text-text-muted mt-0.5">Show previous sessions on startup</div>
              </div>
              <button
                aria-label="Toggle restore sessions on launch"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.restoreSessionsOnLaunch ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("restoreSessionsOnLaunch", !$settings.restoreSessionsOnLaunch)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.restoreSessionsOnLaunch ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Default project path</span>
              <div class="flex gap-1">
                <input
                  class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-48 text-right focus:border-accent-dim"
                  value={$settings.defaultProjectPath ?? ""}
                  oninput={(e) => updateSetting("defaultProjectPath", e.currentTarget.value || null)}
                  placeholder="~/src"
                />
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={browseDefaultProject}
                >...</button>
              </div>
            </div>
            <div class="py-2">
              <div class="text-[13px]">Repository roots</div>
              <div class="text-[11px] text-text-muted mt-0.5">Quick-pick sources for New Session (keeps file picker available)</div>
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
                  onclick={() => addRepoRoot(repoRootDraft)}
                >Add</button>
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={browseAndAddRepoRoot}
                >...</button>
              </div>
              {#if ($settings.repoRoots ?? []).length > 0}
                <div class="mt-2 flex flex-col gap-1">
                  {#each ($settings.repoRoots ?? []) as root (root)}
                    <div class="flex items-center gap-2 rounded border border-border-subtle bg-bg-surface/35 px-2 py-1">
                      <span class="font-mono text-[11px] text-text-secondary flex-1 truncate" title={root}>{root}</span>
                      <button
                        class="text-[10px] text-text-muted hover:text-red cursor-pointer"
                        onclick={() => removeRepoRoot(root)}
                      >Remove</button>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Exclude worktrees from roots</div>
                <div class="text-[11px] text-text-muted mt-0.5">Hide linked git worktrees from root-folder quick-pick results</div>
              </div>
              <button
                aria-label="Toggle excluding worktrees from root discovery"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {($settings.excludeWorktreesFromRepoRoots ?? true) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("excludeWorktreesFromRepoRoots", !($settings.excludeWorktreesFromRepoRoots ?? true))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {($settings.excludeWorktreesFromRepoRoots ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="py-2">
              <div class="flex items-center justify-between">
                <div>
                  <div class="text-[13px]">Worktree base path</div>
                  <div class="text-[11px] text-text-muted mt-0.5">
                    Where to create new worktrees. Supports
                    <code class="font-mono">{'{project_dir}'}</code>,
                    <code class="font-mono">{'{git_root}'}</code>,
                    <code class="font-mono">{'{project_name}'}</code>,
                    <code class="font-mono">{'{home}'}</code>.
                  </div>
                </div>
                <div class="flex gap-1">
                  <input
                    class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
                    value={$settings.worktreeBasePath ?? ""}
                    oninput={(e) => updateSetting("worktreeBasePath", e.currentTarget.value || null)}
                    placeholder="{'{project_dir}'}/.worktrees"
                  />
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    onclick={browseWorktreeBase}
                  >...</button>
                </div>
              </div>
              {#if previewText}
                <div class="mt-1.5 text-[11px] text-text-muted font-mono truncate" title={previewText}>
                  → {previewText}
                </div>
              {/if}
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">On session close</div>
                <div class="text-[11px] text-text-muted mt-0.5">What to do with the session's worktree</div>
              </div>
              <div class="flex rounded border border-border bg-bg-deep overflow-hidden">
                {#each [
                  { id: "never", label: "Keep" },
                  { id: "prompt", label: "Ask" },
                  { id: "always", label: "Remove" },
                ] as const as opt}
                  {@const active = ($settings.worktreeCleanupOnClose ?? "prompt") === opt.id}
                  <button
                    class="px-2.5 py-1 text-[11px] cursor-pointer transition-colors
                      {active ? 'bg-accent-dim text-text-primary' : 'text-text-secondary hover:bg-bg-hover'}"
                    onclick={() => setCleanupMode(opt.id)}
                  >{opt.label}</button>
                {/each}
              </div>
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">New Worktree default</div>
                <div class="text-[11px] text-text-muted mt-0.5">Starting point when you click "New Worktree" directly (hover still exposes all three)</div>
              </div>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$settings.worktreeDefaultBase ?? "currentBranch"}
                onchange={(e) => setDefaultBase(e.currentTarget.value as WorktreeDefaultBase)}
              >
                <option value="currentBranch">Current branch</option>
                <option value="main">main</option>
                <option value="originMain">origin/main</option>
              </select>
            </div>
          {:else if selected === "terminal"}
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Font size</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-20 text-right focus:border-accent-dim"
                type="number"
                value={$settings.fontSize}
                oninput={(e) => updateSetting("fontSize", parseInt(e.currentTarget.value) || 14)}
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Terminal font</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-56 text-right focus:border-accent-dim"
                value={$settings.fontFamily}
                oninput={(e) => updateSetting("fontFamily", e.currentTarget.value)}
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">UI font</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-56 text-right focus:border-accent-dim"
                value={$settings.uiFontFamily}
                oninput={(e) => updateSetting("uiFontFamily", e.currentTarget.value)}
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Scrollback lines</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-24 text-right focus:border-accent-dim"
                type="number"
                value={$settings.scrollback}
                oninput={(e) => updateSetting("scrollback", parseInt(e.currentTarget.value) || 5000)}
              />
            </div>
          {:else if selected === "claude"}
            <div class="flex items-center justify-between py-2">
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
          {:else if selected === "notes"}
            <div class="py-2">
              <div class="flex items-center justify-between">
                <div>
                  <div class="text-[13px]">Vault location</div>
                  <div class="text-[11px] text-text-muted mt-0.5">Where Roux stores notes. Works as a standalone folder or a subdirectory inside an Obsidian vault.</div>
                </div>
              </div>
              <div class="mt-2 flex gap-1">
                <input
                  class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none flex-1 focus:border-accent-dim"
                  value={$settings.notesVaultRoot ?? ""}
                  oninput={(e) => updateSetting("notesVaultRoot", e.currentTarget.value || null)}
                  placeholder="~/Documents/Roux"
                />
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={browseNotesVault}
                >...</button>
              </div>
              <div class="mt-1.5 text-[11px] text-text-muted">
                Leave blank to use the default location. Changing this does not move existing notes.
              </div>
            </div>
            <div class="flex items-center justify-between py-2 mt-2">
              <div>
                <div class="text-[13px]">Include web anchors</div>
                <div class="text-[11px] text-text-muted mt-0.5">Add HTML anchor tags for compatibility with static site generators. Disable for cleaner markdown in Obsidian.</div>
              </div>
              <button
                aria-label="Toggle web anchors in notes"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {($settings.notesIncludeWebAnchors ?? true) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("notesIncludeWebAnchors", !($settings.notesIncludeWebAnchors ?? true))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {($settings.notesIncludeWebAnchors ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
          {:else if selected === "integrations"}
            <div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="text-[13px] font-semibold">GitHub CLI</div>
              <div class="mt-0.5 text-[11px] text-text-muted">
                Used for "Session from PR" and PR watches. Roux auto-detects
                <code class="font-mono">gh</code> via your login shell's PATH (including fish). Set this only if
                auto-detection misses your install — paste the output of <code class="font-mono">which gh</code>.
                Takes effect after restarting Roux.
              </div>
              <div class="mt-3 flex items-center justify-between gap-2">
                <span class="text-[13px]">Binary path</span>
                <div class="flex gap-1">
                  <input
                    class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
                    value={$settings.ghBinaryPath ?? ""}
                    oninput={(e) => updateSetting("ghBinaryPath", e.currentTarget.value || null)}
                    placeholder="/opt/homebrew/bin/gh"
                  />
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    onclick={browseGhBinary}
                  >...</button>
                </div>
              </div>
            </div>
          {:else if selected === "notifications"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Enable OS notifications</div>
                <div class="text-[11px] text-text-muted mt-0.5">Master switch for macOS notification fan-out. The in-app pane always works.</div>
              </div>
              <button
                aria-label="Toggle OS notifications"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.notificationsEnabled ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("notificationsEnabled", !$settings.notificationsEnabled)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.notificationsEnabled ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>

            <div class="mt-3 rounded-lg border border-amber/20 bg-amber/5 p-3 text-[11px] text-text-secondary">
              <div class="mb-1 font-semibold text-amber">macOS quirk</div>
              <div class="leading-relaxed">
                In dev mode Roux borrows <span class="font-mono text-[10px]">com.apple.Terminal</span>'s notification identity (unsigned binaries can't own a bundle id). If you don't see the test notification below, open <span class="text-text-primary">System Settings → Notifications → Terminal</span> and make sure "Allow Notifications" is on. Bundled release builds use <span class="font-mono text-[10px]">com.phin-tech.roux</span>.
              </div>
            </div>

            <div class="mt-3 flex items-center justify-between gap-3">
              <div>
                <div class="text-[13px]">Test notification</div>
                <div class="text-[11px] text-text-muted mt-0.5">
                  {#if notifTestStatus === "sent"}
                    Test fired — check macOS notification center. If nothing shows, fix permissions above.
                  {:else if notifTestStatus === "error"}
                    <span class="text-red">Failed: {notifTestError}</span>
                  {:else}
                    Pushes an Attention-level notification through the service
                  {/if}
                </div>
              </div>
              <button
                class="shrink-0 cursor-pointer rounded-lg border border-border-subtle bg-bg-deep px-3 py-1.5 text-[12px] font-semibold text-text-primary hover:border-accent hover:bg-bg-hover"
                onclick={sendTestNotification}
              >
                Send test
              </button>
            </div>
          {:else if selected === "keyboard"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Show pane hint overlay when holding Option</div>
                <div class="text-[11px] text-text-muted mt-0.5">Reveals pane numbers while ⌥ is held. Option+digit shortcuts still work either way.</div>
              </div>
              <button
                aria-label="Toggle pane hint overlay on Option"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.showPaneHintsOnOption ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("showPaneHintsOnOption", !$settings.showPaneHintsOnOption)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.showPaneHintsOnOption ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Show session hint overlay when holding Command</div>
                <div class="text-[11px] text-text-muted mt-0.5">Reveals session shortcuts while ⌘ is held. Command chord shortcuts still work either way.</div>
              </div>
              <button
                aria-label="Toggle session hint overlay on Command"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.showSessionHintsOnCommand !== false ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("showSessionHintsOnCommand", !($settings.showSessionHintsOnCommand !== false))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.showSessionHintsOnCommand !== false ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
          {:else if selected === "advanced"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Current version</div>
                <div class="text-[11px] text-text-muted mt-0.5 font-mono">{appVersion}</div>
              </div>
              <button
                class="rounded border border-border px-2.5 py-1 text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-50"
                disabled={$updateStatus.kind === "checking" || $updateStatus.kind === "downloading"}
                onclick={() => void runManualCheck()}
              >
                {$updateStatus.kind === "checking" ? "Checking…" : "Check for updates"}
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
                  Downloading update{$updateStatus.progress !== null ? ` (${Math.round($updateStatus.progress * 100)}%)` : "…"}
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
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
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
                onclick={() => updateSetting('updateCheckOnLaunch', !($settings.updateCheckOnLaunch ?? true))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {($settings.updateCheckOnLaunch ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>

            <div class="mt-4 flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Enable logging</div>
                <div class="text-[11px] text-text-muted mt-0.5">Write logs to disk for debugging</div>
              </div>
              <button
                aria-label="Toggle logging"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.enableLogging ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => {
                  const next = !$settings.enableLogging;
                  setLoggingEnabled(next);
                  updateSetting("enableLogging", next);
                }}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.enableLogging ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            {#if $settings.enableLogging}
              <div class="text-[11px] text-text-muted font-mono mt-1 break-all">{getLogPath()}</div>
            {/if}

            <div class="mt-6 border-t border-hairline pt-5">
              <DoctorPanel mode="settings" />
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}
