<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { open } from "@tauri-apps/plugin-dialog";
  import { THEME_DEFINITIONS } from "$lib/themes";
  import { getLogPath, setLoggingEnabled } from "$lib/logging";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  type SectionId =
    | "appearance"
    | "layout"
    | "terminal"
    | "projects"
    | "worktrees"
    | "sessions"
    | "claude"
    | "security"
    | "logging";

  interface SidebarSection {
    id: SectionId;
    label: string;
  }

  interface SidebarGroup {
    label: string;
    sections: SidebarSection[];
  }

  interface SectionMeta {
    title: string;
    description: string;
  }

  const sidebarGroups: SidebarGroup[] = [
    {
      label: "General",
      sections: [
        { id: "appearance", label: "Appearance" },
        { id: "layout", label: "Layout" },
        { id: "terminal", label: "Terminal" },
      ],
    },
    {
      label: "Workspace",
      sections: [
        { id: "projects", label: "Projects" },
        { id: "worktrees", label: "Worktrees" },
        { id: "sessions", label: "Sessions" },
      ],
    },
    {
      label: "Integrations",
      sections: [
        { id: "claude", label: "Claude" },
      ],
    },
    {
      label: "Advanced",
      sections: [
        { id: "security", label: "Security" },
        { id: "logging", label: "Logging" },
      ],
    },
  ];

  const sectionMeta: Record<SectionId, SectionMeta> = {
    appearance: {
      title: "Appearance",
      description: "Tune the overall look and feel of the app chrome.",
    },
    layout: {
      title: "Layout",
      description: "Control how navigation and session grouping behave.",
    },
    terminal: {
      title: "Terminal",
      description: "Adjust terminal typography and cursor behavior.",
    },
    projects: {
      title: "Projects",
      description: "Set defaults for where new session work starts.",
    },
    worktrees: {
      title: "Worktrees",
      description: "Choose how Roux creates and cleans up worktrees.",
    },
    sessions: {
      title: "Sessions",
      description: "Configure startup and session close behavior.",
    },
    claude: {
      title: "Claude",
      description: "Configure the Claude CLI Roux launches for sessions.",
    },
    security: {
      title: "Security",
      description: "Control secret redaction in terminal output.",
    },
    logging: {
      title: "Logging",
      description: "Enable debug logging and inspect the output location.",
    },
  };

  const rowClass =
    "flex items-start justify-between gap-6 rounded-xl border border-border-subtle bg-bg-surface/35 px-4 py-3";
  const inputClass =
    "w-56 rounded-lg border border-border bg-bg-deep px-3 py-2 text-[13px] text-text-primary outline-none focus:border-accent-dim";
  const monoInputClass =
    "w-56 rounded-lg border border-border bg-bg-deep px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim";
  const selectClass =
    "w-56 cursor-pointer appearance-none rounded-lg border border-border bg-bg-deep px-3 py-2 text-[13px] text-text-primary outline-none focus:border-accent-dim";
  const browseButtonClass =
    "cursor-pointer rounded-lg border border-border-subtle bg-bg-surface px-3 py-2 text-[12px] text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary";

  let { visible, onclose }: Props = $props();

  let activeSection = $state<SectionId>("appearance");
  let dialogEl: HTMLDivElement | undefined = $state();
  let currentSection = $derived(sectionMeta[activeSection]);

  $effect(() => {
    if (visible) {
      requestAnimationFrame(() => dialogEl?.focus());
    }
  });

  function handleKeyDown(e: KeyboardEvent) {
    if (!visible || e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    onclose();
  }

  async function browseClaudeBinary() {
    const selected = await open({ directory: false, title: "Select Claude Binary" });
    if (selected) updateSetting("claudeBinaryPath", selected as string);
  }

  async function browseWorktreeBase() {
    const selected = await open({ directory: true, title: "Select Worktree Base Directory" });
    if (selected) updateSetting("worktreeBasePath", selected as string);
  }

  async function browseDefaultProject() {
    const selected = await open({ directory: true, title: "Select Default Project Directory" });
    if (selected) updateSetting("defaultProjectPath", selected as string);
  }
</script>

<svelte:window onkeydown={handleKeyDown} />

{#if visible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 backdrop-blur-md"
    onclick={(e) => {
      if (e.target === e.currentTarget) onclose();
    }}
    transition:fade={{ duration: 120 }}
  >
    <div
      bind:this={dialogEl}
      aria-labelledby="preferences-title"
      aria-modal="true"
      class="ui-dialog flex h-[min(720px,calc(100vh-48px))] w-[min(940px,calc(100vw-48px))] overflow-hidden rounded-[1.6rem]"
      role="dialog"
      tabindex="-1"
      transition:scale={{ duration: 120, start: 0.985 }}
    >
      <aside class="flex w-[228px] shrink-0 flex-col border-r border-border-subtle bg-[linear-gradient(180deg,rgba(255,255,255,0.04),rgba(255,255,255,0.015))]">
        <div class="border-b border-border-subtle px-4 py-4">
          <div class="mb-3 flex items-center gap-2">
            <button
              aria-label="Close Preferences"
              class="h-3 w-3 cursor-pointer rounded-full border border-black/10 bg-[#ff5f57] shadow-[inset_0_1px_0_rgba(255,255,255,0.28)]"
              onclick={onclose}
            ></button>
            <span class="h-3 w-3 rounded-full border border-black/10 bg-[#febc2e] opacity-80"></span>
            <span class="h-3 w-3 rounded-full border border-black/10 bg-[#28c840] opacity-80"></span>
          </div>
          <div class="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">Roux</div>
          <h2 id="preferences-title" class="mt-1 text-[15px] font-semibold tracking-tight text-text-primary">Preferences</h2>
          <p class="mt-1 text-[12px] leading-5 text-text-muted">Live settings for the workspace and session shell.</p>
        </div>

        <nav class="app-scrollbar flex-1 overflow-y-auto px-3 py-4">
          {#each sidebarGroups as group}
            <div class="mb-5 last:mb-0">
              <div class="px-2 pb-2 text-[10px] font-semibold uppercase tracking-[0.18em] text-text-muted/80">{group.label}</div>
              <div class="space-y-1">
                {#each group.sections as section}
                  <button
                    aria-current={activeSection === section.id ? "page" : undefined}
                    class="flex w-full cursor-pointer items-center rounded-xl border px-3 py-2.5 text-left text-[13px] transition-all
                      {activeSection === section.id
                        ? 'border-border bg-bg-active text-text-primary shadow-[inset_0_1px_0_rgba(255,255,255,0.05)]'
                        : 'border-transparent text-text-secondary hover:border-border-subtle hover:bg-bg-surface/40 hover:text-text-primary'}"
                    onclick={() => (activeSection = section.id)}
                  >
                    <span class="min-w-0 flex-1 truncate">{section.label}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/each}
        </nav>
      </aside>

      <section class="flex min-w-0 flex-1 flex-col bg-bg-base/70">
        <header class="border-b border-border-subtle px-7 py-6">
          <h3 class="text-[24px] font-semibold tracking-tight text-text-primary">{currentSection.title}</h3>
          <p class="mt-1 max-w-[52ch] text-[13px] leading-6 text-text-muted">{currentSection.description}</p>
        </header>

        <div class="app-scrollbar flex-1 overflow-y-auto px-7 py-6">
          {#if activeSection === "appearance"}
            <div class="space-y-6">
              <section>
                <div class="mb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">Theme</div>
                <div class="grid grid-cols-2 gap-3">
                  {#each THEME_DEFINITIONS as theme}
                    <button
                      class="cursor-pointer rounded-2xl border p-3 text-left transition-all
                        {$settings.theme === theme.id
                          ? 'border-accent/40 bg-bg-active shadow-[inset_0_1px_0_rgba(255,255,255,0.06)]'
                          : 'border-border-subtle bg-bg-surface/35 hover:border-border hover:bg-bg-surface/55'}"
                      onclick={() => updateSetting("theme", theme.id)}
                    >
                      <div class="mb-3 flex items-center justify-between gap-3">
                        <div class="flex gap-1.5">
                          <span class="h-3 w-3 rounded-full border border-black/10 bg-bg-deep"></span>
                          <span class="h-3 w-3 rounded-full border border-black/10 bg-bg-surface"></span>
                          <span class="h-3 w-3 rounded-full border border-black/10 bg-accent"></span>
                        </div>
                        {#if $settings.theme === theme.id}
                          <span class="text-[11px] font-semibold uppercase tracking-[0.12em] text-accent">Selected</span>
                        {/if}
                      </div>
                      <div class="text-[14px] font-medium text-text-primary">{theme.label}</div>
                      <div class="mt-1 text-[12px] leading-5 text-text-muted">{theme.description}</div>
                    </button>
                  {/each}
                </div>
              </section>

              <section>
                <div class="mb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">Typography</div>
                <div class="space-y-3">
                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">UI font</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Used for sidebar labels, dialogs, and app chrome.</div>
                    </div>
                    <input
                      class={inputClass}
                      value={$settings.uiFontFamily}
                      oninput={(e) => updateSetting("uiFontFamily", e.currentTarget.value)}
                    />
                  </div>
                </div>
              </section>
            </div>
          {/if}

          {#if activeSection === "layout"}
            <div class="space-y-3">
              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Tab position</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Choose which side of the app holds the session rail.</div>
                </div>
                <select
                  class={selectClass}
                  value={$settings.tabPosition}
                  onchange={(e) => updateSetting("tabPosition", e.currentTarget.value as "left" | "right")}
                >
                  <option value="left">Left</option>
                  <option value="right">Right</option>
                </select>
              </div>

              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Sidebar width</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Default width for the session rail in pixels.</div>
                </div>
                <input
                  class={monoInputClass}
                  type="number"
                  min="180"
                  max="420"
                  value={$settings.tabWidth}
                  oninput={(e) => updateSetting("tabWidth", Math.max(180, Math.min(420, parseInt(e.currentTarget.value) || 260)))}
                />
              </div>

              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Session grouping</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Group the session list by repository or assigned project.</div>
                </div>
                <select
                  class={selectClass}
                  value={$settings.groupBy}
                  onchange={(e) => updateSetting("groupBy", e.currentTarget.value as "repo" | "project")}
                >
                  <option value="repo">Repository</option>
                  <option value="project">Project</option>
                </select>
              </div>
            </div>
          {/if}

          {#if activeSection === "terminal"}
            <div class="space-y-6">
              <section>
                <div class="mb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">Typography</div>
                <div class="space-y-3">
                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Terminal font</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Primary monospace stack for shell panes.</div>
                    </div>
                    <input
                      class={monoInputClass}
                      value={$settings.fontFamily}
                      oninput={(e) => updateSetting("fontFamily", e.currentTarget.value)}
                    />
                  </div>

                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Font size</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Terminal and markdown font size in pixels.</div>
                    </div>
                    <input
                      class={monoInputClass}
                      type="number"
                      min="10"
                      max="32"
                      value={$settings.fontSize}
                      oninput={(e) => updateSetting("fontSize", Math.max(10, Math.min(32, parseInt(e.currentTarget.value) || 14)))}
                    />
                  </div>

                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Line height</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Spacing between terminal rows.</div>
                    </div>
                    <input
                      class={monoInputClass}
                      type="number"
                      min="1"
                      max="2"
                      step="0.05"
                      value={$settings.lineHeight}
                      oninput={(e) => updateSetting("lineHeight", Math.max(1, Math.min(2, parseFloat(e.currentTarget.value) || 1.2)))}
                    />
                  </div>

                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Scrollback lines</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">How much terminal history Roux keeps in memory.</div>
                    </div>
                    <input
                      class={monoInputClass}
                      type="number"
                      min="1000"
                      max="50000"
                      step="1000"
                      value={$settings.scrollback}
                      oninput={(e) => updateSetting("scrollback", Math.max(1000, parseInt(e.currentTarget.value) || 5000))}
                    />
                  </div>
                </div>
              </section>

              <section>
                <div class="mb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">Cursor</div>
                <div class="space-y-3">
                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Cursor style</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Choose the terminal cursor shape.</div>
                    </div>
                    <select
                      class={selectClass}
                      value={$settings.cursorStyle}
                      onchange={(e) => updateSetting("cursorStyle", e.currentTarget.value as "block" | "underline" | "bar")}
                    >
                      <option value="block">Block</option>
                      <option value="underline">Underline</option>
                      <option value="bar">Bar</option>
                    </select>
                  </div>

                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Blink cursor</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Animate the insertion point in shell panes.</div>
                    </div>
                    <button
                      aria-label="Toggle cursor blink"
                      class="relative h-6 w-11 cursor-pointer rounded-full border transition-all
                        {$settings.cursorBlink ? 'border-accent bg-accent-dim' : 'border-border bg-bg-deep'}"
                      onclick={() => updateSetting("cursorBlink", !$settings.cursorBlink)}
                    >
                      <div
                        class="absolute top-[3px] h-4.5 w-4.5 rounded-full bg-white transition-all
                          {$settings.cursorBlink ? 'left-[22px]' : 'left-[3px]'}"
                      ></div>
                    </button>
                  </div>
                </div>
              </section>
            </div>
          {/if}

          {#if activeSection === "projects"}
            <div class="space-y-3">
              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Default project path</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Pre-fill the repository picker when creating a new session.</div>
                </div>
                <div class="flex items-center gap-2">
                  <input
                    class={monoInputClass}
                    value={$settings.defaultProjectPath ?? ""}
                    oninput={(e) => updateSetting("defaultProjectPath", e.currentTarget.value || null)}
                    placeholder="~/src"
                  />
                  <button class={browseButtonClass} onclick={browseDefaultProject}>Browse</button>
                </div>
              </div>
            </div>
          {/if}

          {#if activeSection === "worktrees"}
            <div class="space-y-3">
              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Base path</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Directory where new worktrees should be created.</div>
                </div>
                <div class="flex items-center gap-2">
                  <input
                    class={monoInputClass}
                    value={$settings.worktreeBasePath ?? ""}
                    oninput={(e) => updateSetting("worktreeBasePath", e.currentTarget.value || null)}
                    placeholder="~/worktrees"
                  />
                  <button class={browseButtonClass} onclick={browseWorktreeBase}>Browse</button>
                </div>
              </div>

              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Cleanup on close</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Automatically remove worktrees when their sessions are closed.</div>
                </div>
                <button
                  aria-label="Toggle cleanup worktrees on close"
                  class="relative h-6 w-11 cursor-pointer rounded-full border transition-all
                    {$settings.cleanupWorktreesOnClose ? 'border-accent bg-accent-dim' : 'border-border bg-bg-deep'}"
                  onclick={() => updateSetting("cleanupWorktreesOnClose", !$settings.cleanupWorktreesOnClose)}
                >
                  <div
                    class="absolute top-[3px] h-4.5 w-4.5 rounded-full bg-white transition-all
                      {$settings.cleanupWorktreesOnClose ? 'left-[22px]' : 'left-[3px]'}"
                  ></div>
                </button>
              </div>
            </div>
          {/if}

          {#if activeSection === "sessions"}
            <div class="space-y-3">
              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Confirm on close</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Prompt before closing active sessions.</div>
                </div>
                <button
                  aria-label="Toggle confirm on close"
                  class="relative h-6 w-11 cursor-pointer rounded-full border transition-all
                    {$settings.confirmOnClose ? 'border-accent bg-accent-dim' : 'border-border bg-bg-deep'}"
                  onclick={() => updateSetting("confirmOnClose", !$settings.confirmOnClose)}
                >
                  <div
                    class="absolute top-[3px] h-4.5 w-4.5 rounded-full bg-white transition-all
                      {$settings.confirmOnClose ? 'left-[22px]' : 'left-[3px]'}"
                  ></div>
                </button>
              </div>

              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Restore on launch</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Reopen previous sessions when the app starts.</div>
                </div>
                <button
                  aria-label="Toggle restore sessions on launch"
                  class="relative h-6 w-11 cursor-pointer rounded-full border transition-all
                    {$settings.restoreSessionsOnLaunch ? 'border-accent bg-accent-dim' : 'border-border bg-bg-deep'}"
                  onclick={() => updateSetting("restoreSessionsOnLaunch", !$settings.restoreSessionsOnLaunch)}
                >
                  <div
                    class="absolute top-[3px] h-4.5 w-4.5 rounded-full bg-white transition-all
                      {$settings.restoreSessionsOnLaunch ? 'left-[22px]' : 'left-[3px]'}"
                  ></div>
                </button>
              </div>
            </div>
          {/if}

          {#if activeSection === "claude"}
            <div class="space-y-6">
              <section>
                <div class="mb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">CLI</div>
                <div class="space-y-3">
                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Binary path</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Leave blank to auto-detect the Claude binary from `PATH`.</div>
                    </div>
                    <div class="flex items-center gap-2">
                      <input
                        class={monoInputClass}
                        value={$settings.claudeBinaryPath ?? ""}
                        oninput={(e) => updateSetting("claudeBinaryPath", e.currentTarget.value || null)}
                        placeholder="/usr/local/bin/claude"
                      />
                      <button class={browseButtonClass} onclick={browseClaudeBinary}>Browse</button>
                    </div>
                  </div>

                  <div class={rowClass}>
                    <div>
                      <div class="text-[13px] font-medium text-text-primary">Default model</div>
                      <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Model passed to new sessions unless overridden elsewhere.</div>
                    </div>
                    <input
                      class={monoInputClass}
                      value={$settings.defaultModel ?? ""}
                      oninput={(e) => updateSetting("defaultModel", e.currentTarget.value || null)}
                      placeholder="opus"
                    />
                  </div>
                </div>
              </section>

              <section>
                <div class="mb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">Flags</div>
                <div class={rowClass}>
                  <div>
                    <div class="text-[13px] font-medium text-text-primary">Additional flags</div>
                    <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Extra CLI flags appended to every new session launch.</div>
                  </div>
                  <input
                    class={monoInputClass}
                    value={$settings.additionalFlags.join(" ")}
                    oninput={(e) => updateSetting("additionalFlags", e.currentTarget.value.split(" ").filter(Boolean))}
                    placeholder="--verbose"
                  />
                </div>
              </section>
            </div>
          {/if}

          {#if activeSection === "security"}
            <div class="space-y-3">
              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Redact secrets</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Automatically mask API keys, tokens, and credentials in terminal output.</div>
                </div>
                <button
                  aria-label="Toggle secret redaction"
                  class="relative h-6 w-11 cursor-pointer rounded-full border transition-all
                    {$settings.redactSecrets ? 'border-accent bg-accent-dim' : 'border-border bg-bg-deep'}"
                  onclick={() => updateSetting("redactSecrets", !$settings.redactSecrets)}
                >
                  <div
                    class="absolute top-[3px] h-4.5 w-4.5 rounded-full bg-white transition-all
                      {$settings.redactSecrets ? 'left-[22px]' : 'left-[3px]'}"
                  ></div>
                </button>
              </div>

              {#if $settings.redactSecrets}
                <div class="space-y-2 pl-2">
                  {#each [
                    { key: "apiKeys" as const, label: "API Keys & Tokens", desc: "GitHub tokens, AWS keys, JWTs, and generic API keys" },
                    { key: "credentials" as const, label: "Credentials & Passwords", desc: "Bearer tokens, basic auth headers, and URL-embedded passwords" },
                    { key: "privateKeys" as const, label: "Private Keys", desc: "PEM-encoded RSA, EC, and other private key blocks" },
                    { key: "connectionStrings" as const, label: "Connection Strings", desc: "Database URLs with embedded credentials" },
                  ] as cat}
                    <div class={rowClass}>
                      <div>
                        <div class="text-[13px] font-medium text-text-primary">{cat.label}</div>
                        <div class="mt-0.5 text-[12px] leading-5 text-text-muted">{cat.desc}</div>
                      </div>
                      <button
                        aria-label="Toggle {cat.label} redaction"
                        class="relative h-6 w-11 cursor-pointer rounded-full border transition-all
                          {$settings.redactCategories[cat.key] ? 'border-accent bg-accent-dim' : 'border-border bg-bg-deep'}"
                        onclick={() => {
                          const updated = { ...$settings.redactCategories };
                          updated[cat.key] = !updated[cat.key];
                          updateSetting("redactCategories", updated);
                        }}
                      >
                        <div
                          class="absolute top-[3px] h-4.5 w-4.5 rounded-full bg-white transition-all
                            {$settings.redactCategories[cat.key] ? 'left-[22px]' : 'left-[3px]'}"
                        ></div>
                      </button>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}

          {#if activeSection === "logging"}
            <div class="space-y-3">
              <div class={rowClass}>
                <div>
                  <div class="text-[13px] font-medium text-text-primary">Enable logging</div>
                  <div class="mt-0.5 text-[12px] leading-5 text-text-muted">Write Roux diagnostics to disk for debugging sessions and startup issues.</div>
                </div>
                <button
                  aria-label="Toggle logging"
                  class="relative h-6 w-11 cursor-pointer rounded-full border transition-all
                    {$settings.enableLogging ? 'border-accent bg-accent-dim' : 'border-border bg-bg-deep'}"
                  onclick={() => {
                    const next = !$settings.enableLogging;
                    setLoggingEnabled(next);
                    updateSetting("enableLogging", next);
                  }}
                >
                  <div
                    class="absolute top-[3px] h-4.5 w-4.5 rounded-full bg-white transition-all
                      {$settings.enableLogging ? 'left-[22px]' : 'left-[3px]'}"
                  ></div>
                </button>
              </div>

              <div class="rounded-xl border border-border-subtle bg-bg-surface/25 px-4 py-3">
                <div class="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-muted">Log file</div>
                <div class="mt-2 break-all font-mono text-[12px] leading-5 text-text-secondary">{getLogPath()}</div>
                <div class="mt-2 text-[12px] leading-5 text-text-muted">
                  {$settings.enableLogging ? "Logging is active. New events will be appended here." : "Logging is currently disabled."}
                </div>
              </div>
            </div>
          {/if}
        </div>

        <footer class="flex items-center justify-between border-t border-border-subtle px-7 py-4">
          <div class="text-[12px] text-text-muted">Changes apply immediately.</div>
          <button
            class="cursor-pointer rounded-xl border border-border bg-bg-surface px-4 py-2 text-[13px] font-medium text-text-primary transition-colors hover:bg-bg-hover"
            onclick={onclose}
          >
            Done
          </button>
        </footer>
      </section>
    </div>
  </div>
{/if}
