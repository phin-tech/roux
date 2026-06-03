<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { commands } from "$lib/bindings";
  import type { GpuAcceleration } from "$lib/bindings";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { userTerminalThemes, loadUserTerminalThemes } from "$lib/stores/userTerminalThemes";
  import { getAllTerminalThemeDefinitions } from "$lib/themes";

  let allTerminalThemes = $derived(getAllTerminalThemeDefinitions($userTerminalThemes));
  let currentTerminalThemeId = $derived($settings.terminalTheme ?? "match-gui");
  let currentDef = $derived(
    allTerminalThemes.find((theme) => theme.id === currentTerminalThemeId),
  );
  let isMissingUserTheme = $derived(
    !currentDef && currentTerminalThemeId.startsWith("user:"),
  );

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
</script>

<div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3 mb-3">
  <div class="flex items-start justify-between gap-3">
    <div>
      <div class="text-[13px]">Terminal theme</div>
      <div class="text-[11px] text-text-muted mt-0.5">Color palette for the xterm pane. Independent of the GUI theme. Save iTerm2 <code>.itermcolors</code> files into <code>~/.config/roux/themes/</code> to add your own.</div>
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
        onclick={revealUserThemesDir}
      >Reveal</button>
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        title="Re-scan ~/.config/roux/themes/"
        onclick={() => void loadUserTerminalThemes()}
      >Reload</button>
    </div>
  </div>
  {#if isMissingUserTheme}
    <p class="mt-2 text-[11px] text-amber-500/90">
      This theme file isn't currently loaded. The setting is preserved — drop the file back into <code>~/.config/roux/themes/</code> and hit Reload, or pick a different theme.
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
  <span class="text-[13px]">Scrollback lines</span>
  <input
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-24 text-right focus:border-accent-dim"
    type="number"
    value={$settings.scrollback}
    oninput={(e) => updateSetting("scrollback", parseInt(e.currentTarget.value) || 5000)}
  />
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">GPU acceleration</div>
    <div class="text-[11px] text-text-muted mt-0.5">Applies to terminals opened after this change.</div>
  </div>
  <select
    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
    value={$settings.gpuAcceleration ?? "auto"}
    onchange={(e) => updateSetting("gpuAcceleration", e.currentTarget.value as GpuAcceleration)}
  >
    <option value="auto">Auto</option>
    <option value="on">On (WebGL)</option>
    <option value="off">Off (DOM)</option>
  </select>
</div>
<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between">
    <div class="text-[13px] font-semibold">Shell</div>
  </div>
  <div class="mt-0.5 text-[11px] text-text-muted">
    Shell used for terminal panes and login-shell PATH discovery
    (for finding <code class="font-mono">gh</code>, <code class="font-mono">git</code>,
    <code class="font-mono">wt</code>, etc. via Homebrew). Defaults to your OS login shell,
    then <code class="font-mono">$SHELL</code>. Set this only if auto-detection chooses the
    wrong shell. New terminal panes use the updated shell right away; restart Roux if
    integration PATH discovery needs to be refreshed.
  </div>
  <div class="mt-3 flex items-center justify-between gap-2">
    <span class="text-[13px]">Binary path</span>
    <div class="flex gap-1">
      <input
        class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
        value={$settings.shellBinaryPath ?? ""}
        oninput={(e) => updateSetting("shellBinaryPath", e.currentTarget.value || null)}
        placeholder="/opt/homebrew/bin/fish"
      />
      <button
        class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
        onclick={browseShellBinary}
      >...</button>
    </div>
  </div>
</div>
