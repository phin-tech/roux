<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import {
    JSONEditor,
    Mode,
    createAjvValidator,
    type Content,
    type ContentErrors,
    type OnChangeStatus,
  } from "svelte-jsoneditor";
  import "svelte-jsoneditor/themes/jse-theme-dark.css";
  import { commands } from "$lib/bindings";
  import type {
    GpuAcceleration,
    SplitProfileBehavior,
    TerminalDefaults,
    TerminalEnvRule,
  } from "$lib/bindings";
  import {
    settings,
    updateSetting,
    updateSettingsDraft,
  } from "$lib/stores/settings";
  import {
    userTerminalThemes,
    loadUserTerminalThemes,
  } from "$lib/stores/userTerminalThemes";
  import { getAllTerminalThemeDefinitions } from "$lib/themes";

  type TerminalEnvRules = Record<string, TerminalEnvRule>;

  const DEFAULT_TERMINAL_DEFAULTS: TerminalDefaults = {
    env: null,
    beforeShellStarts: null,
    splitProfileBehavior: "plainShell",
  };

  const envRuleValidator = createAjvValidator({
    schema: {
      type: "object",
      additionalProperties: {
        anyOf: [
          { type: "string" },
          {
            type: "object",
            required: ["mode"],
            additionalProperties: false,
            properties: {
              mode: { enum: ["value", "inherit", "unset", "command"] },
              value: { type: "string" },
              command: { type: "string" },
            },
            allOf: [
              {
                if: {
                  properties: { mode: { const: "value" } },
                  required: ["mode"],
                },
                then: { required: ["value"] },
              },
              {
                if: {
                  properties: { mode: { const: "command" } },
                  required: ["mode"],
                },
                then: { required: ["command"] },
              },
            ],
          },
        ],
      },
    },
  });

  let allTerminalThemes = $derived(
    getAllTerminalThemeDefinitions($userTerminalThemes),
  );
  let currentTerminalThemeId = $derived($settings.terminalTheme ?? "match-gui");
  let currentDef = $derived(
    allTerminalThemes.find((theme) => theme.id === currentTerminalThemeId),
  );
  let isMissingUserTheme = $derived(
    !currentDef && currentTerminalThemeId.startsWith("user:"),
  );
  let terminalDefaults = $derived({
    ...DEFAULT_TERMINAL_DEFAULTS,
    ...($settings.terminalDefaults ?? {}),
  });
  let envContent = $state<Content>({ json: {} });
  let envError = $state<string | null>(null);
  let lastEnvJson = $state("");

  $effect(() => {
    const serialized = serializeEnv(terminalDefaults.env);
    if (serialized !== lastEnvJson) {
      envContent = { json: terminalDefaults.env ?? {} };
      envError = null;
      lastEnvJson = serialized;
    }
  });

  async function browseShellBinary() {
    const selected = await open({
      directory: false,
      title: "Select Shell Binary",
    });
    if (selected) updateSetting("shellBinaryPath", selected as string);
  }

  async function revealUserThemesDir() {
    try {
      const dir = await commands.userThemesDir();
      await revealItemInDir(dir);
    } catch (e) {
      console.error("reveal user themes dir failed", e);
    }
  }

  function updateTerminalDefaults(patch: Partial<TerminalDefaults>): void {
    updateSettingsDraft((current) => ({
      ...current,
      terminalDefaults: {
        ...DEFAULT_TERMINAL_DEFAULTS,
        ...(current.terminalDefaults ?? {}),
        ...patch,
      },
    }));
  }

  function updateBeforeShellStarts(value: string): void {
    updateTerminalDefaults({ beforeShellStarts: value.trim() ? value : null });
  }

  function updateSplitProfileBehavior(value: SplitProfileBehavior): void {
    updateTerminalDefaults({ splitProfileBehavior: value });
  }

  function handleEnvChange(
    content: Content,
    _previousContent: Content,
    status: OnChangeStatus,
  ): void {
    envContent = content;
    if (status.contentErrors) {
      envError = describeContentErrors(status.contentErrors);
      return;
    }

    let json: unknown;
    try {
      json = "json" in content ? content.json : JSON.parse(content.text);
    } catch (error) {
      envError = error instanceof Error ? error.message : "Invalid JSON";
      return;
    }

    if (!isPlainObject(json)) {
      envError = "Terminal environment rules must be a JSON object.";
      return;
    }

    const env =
      Object.keys(json).length > 0 ? (json as TerminalEnvRules) : null;
    envError = null;
    lastEnvJson = serializeEnv(env);
    updateTerminalDefaults({ env });
  }

  function describeContentErrors(errors: ContentErrors): string {
    if ("parseError" in errors) {
      return errors.parseError.message;
    }

    const [first] = errors.validationErrors;
    return first?.message ?? "Invalid terminal environment rules.";
  }

  function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function serializeEnv(env: TerminalDefaults["env"] | undefined): string {
    return JSON.stringify(env ?? {}, null, 2);
  }
</script>

<div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3 mb-3">
  <div class="flex items-start justify-between gap-3">
    <div>
      <div class="text-[13px]">Terminal theme</div>
      <div class="text-[11px] text-text-muted mt-0.5">
        Color palette for the xterm pane. Independent of the GUI theme. Save
        iTerm2 <code>.itermcolors</code> files into
        <code>~/.config/roux/themes/</code> to add your own.
      </div>
    </div>
    <div class="flex items-center gap-1">
      <select
        class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6 max-w-[14rem]"
        value={currentTerminalThemeId}
        onchange={(e) => updateSetting("terminalTheme", e.currentTarget.value)}
      >
        <optgroup label="Auto">
          {#each allTerminalThemes.filter((t) => t.category === "auto") as t}
            <option value={t.id}>{t.label}</option>
          {/each}
        </optgroup>
        <optgroup label="App theme palettes">
          {#each allTerminalThemes.filter((t) => t.category === "matching") as t}
            <option value={t.id}>{t.label}</option>
          {/each}
        </optgroup>
        <optgroup label="Editor themes">
          {#each allTerminalThemes.filter((t) => t.category === "editor") as t}
            <option value={t.id}>{t.label}</option>
          {/each}
        </optgroup>
        {#if $userTerminalThemes.length > 0}
          <optgroup label="User">
            {#each allTerminalThemes.filter((t) => t.category === "user") as t}
              <option value={t.id}>{t.label}</option>
            {/each}
          </optgroup>
        {/if}
        {#if isMissingUserTheme}
          <!-- Persisted theme references a user file that's not
               present right now (deleted, renamed, or themes
               folder hasn't loaded yet). Surface it as a
               disabled option so the dropdown reflects the
               setting; selecting any other entry overwrites it. -->
          <option value={currentTerminalThemeId} disabled>
            Missing: {currentTerminalThemeId.slice("user:".length)}
          </option>
        {/if}
      </select>
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        title="Open ~/.config/roux/themes/ in the file manager"
        onclick={revealUserThemesDir}>Reveal</button
      >
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        title="Re-scan ~/.config/roux/themes/"
        onclick={() => void loadUserTerminalThemes()}>Reload</button
      >
    </div>
  </div>
  {#if isMissingUserTheme}
    <p class="mt-2 text-[11px] text-amber-500/90">
      This theme file isn't currently loaded. The setting is preserved — drop
      the file back into <code>~/.config/roux/themes/</code> and hit Reload, or pick
      a different theme.
    </p>
  {:else if currentDef?.description}
    <p class="mt-2 text-[11px] text-text-muted">{currentDef.description}</p>
  {/if}
</div>
<div class="flex items-center justify-between py-2">
  <span class="text-[13px]">Font size</span>
  <input
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-20 text-right focus:border-accent-dim"
    type="number"
    value={$settings.fontSize}
    oninput={(e) =>
      updateSetting("fontSize", parseInt(e.currentTarget.value) || 14)}
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
  <span class="text-[13px]">Scrollback lines</span>
  <input
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-24 text-right focus:border-accent-dim"
    type="number"
    value={$settings.scrollback}
    oninput={(e) =>
      updateSetting("scrollback", parseInt(e.currentTarget.value) || 5000)}
  />
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">GPU acceleration</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Applies to terminals opened after this change.
    </div>
  </div>
  <select
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
    value={$settings.gpuAcceleration ?? "auto"}
    onchange={(e) =>
      updateSetting(
        "gpuAcceleration",
        e.currentTarget.value as GpuAcceleration,
      )}
  >
    <option value="auto">Auto</option>
    <option value="on">On (WebGL)</option>
    <option value="off">Off (DOM)</option>
  </select>
</div>
<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-start justify-between gap-3">
    <div>
      <div class="text-[13px] font-semibold">Terminal defaults</div>
      <div class="mt-0.5 text-[11px] text-text-muted">
        Applies to newly spawned shells before profile-specific setup.
      </div>
    </div>
    <select
      class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
      value={terminalDefaults.splitProfileBehavior}
      onchange={(e) =>
        updateSplitProfileBehavior(
          e.currentTarget.value as SplitProfileBehavior,
        )}
      title="Plain split behavior"
    >
      <option value="plainShell">Plain shell</option>
      <option value="appDefaultProfile">App default profile</option>
      <option value="activePaneProfile">Active pane profile</option>
      <option value="askEveryTime">Ask every time</option>
    </select>
  </div>

  <div class="mt-3 flex flex-col gap-1.5">
    <label
      for="terminal-before-shell-starts"
      class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
    >
      Before shell starts
    </label>
    <textarea
      id="terminal-before-shell-starts"
      class="min-h-16 rounded-md border border-border bg-bg-deep px-3 py-2 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
      value={terminalDefaults.beforeShellStarts ?? ""}
      oninput={(e) => updateBeforeShellStarts(e.currentTarget.value)}
      spellcheck="false"
      placeholder="aws sts get-caller-identity --profile prod >/dev/null 2>&1 || aws sso login --profile prod"
    ></textarea>
  </div>

  <div class="mt-3 flex flex-col gap-1.5">
    <div class="flex items-center justify-between gap-3">
      <label
        for="terminal-env-rules"
        class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
      >
        Environment
      </label>
      {#if envError}
        <span class="text-[11px] text-red">{envError}</span>
      {/if}
    </div>
    <div
      id="terminal-env-rules"
      class="roux-json-editor h-56 overflow-hidden rounded-md border border-border bg-bg-deep"
    >
      <JSONEditor
        content={envContent}
        mode={Mode.text}
        mainMenuBar={false}
        navigationBar={false}
        statusBar={true}
        validator={envRuleValidator}
        onChange={handleEnvChange}
      />
    </div>
  </div>
</div>
<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between">
    <div class="text-[13px] font-semibold">Shell</div>
  </div>
  <div class="mt-0.5 text-[11px] text-text-muted">
    Shell used for terminal panes and login-shell PATH discovery (for finding <code
      class="font-mono">gh</code
    >, <code class="font-mono">git</code>,
    <code class="font-mono">wt</code>, etc. via Homebrew). Defaults to your OS
    login shell, then <code class="font-mono">$SHELL</code>. Set this only if
    auto-detection chooses the wrong shell. New terminal panes use the updated
    shell right away; restart Roux if integration PATH discovery needs to be
    refreshed.
  </div>
  <div class="mt-3 flex items-center justify-between gap-2">
    <span class="text-[13px]">Binary path</span>
    <div class="flex gap-1">
      <input
        class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
        value={$settings.shellBinaryPath ?? ""}
        oninput={(e) =>
          updateSetting("shellBinaryPath", e.currentTarget.value || null)}
        placeholder="/opt/homebrew/bin/fish"
      />
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        onclick={browseShellBinary}>...</button
      >
    </div>
  </div>
</div>

<style>
  :global(.roux-json-editor .jse-main) {
    height: 100%;
    min-height: 0;
    background: transparent;
  }

  :global(.roux-json-editor .jse-menu),
  :global(.roux-json-editor .jse-status-bar) {
    border-color: var(--border-subtle);
  }
</style>
