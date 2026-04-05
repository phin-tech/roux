<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();
</script>

<div
  class="absolute top-0 right-0 bottom-0 w-[380px] bg-bg-surface border-l border-border z-50 flex flex-col shadow-[-8px_0_32px_rgba(0,0,0,0.3)] transition-transform duration-250
    {visible ? 'translate-x-0' : 'translate-x-full'}"
>
  <div class="px-5 py-4 border-b border-border-subtle flex items-center justify-between">
    <span class="text-sm font-semibold">Settings</span>
    <button
      class="bg-transparent border-none text-text-muted cursor-pointer text-base p-1 rounded hover:text-text-primary hover:bg-bg-hover"
      onclick={onclose}
    >&times;</button>
  </div>

  <div class="flex-1 overflow-y-auto px-5 py-4">
    <!-- Layout -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Layout</h3>
      <div class="flex items-center justify-between py-2">
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
    </section>

    <!-- Worktrees -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Worktrees</h3>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Base path</div>
          <div class="text-[11px] text-text-muted mt-0.5">Where to create new worktrees</div>
        </div>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.worktreeBasePath ?? ""}
          oninput={(e) => updateSetting("worktreeBasePath", e.currentTarget.value || null)}
          placeholder="~/worktrees"
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Cleanup on close</div>
          <div class="text-[11px] text-text-muted mt-0.5">Auto-remove worktrees when closing sessions</div>
        </div>
        <button
          class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
            {$settings.cleanupWorktreesOnClose
              ? 'bg-accent-dim border-accent'
              : 'bg-bg-deep border-border'}"
          onclick={() => updateSetting("cleanupWorktreesOnClose", !$settings.cleanupWorktreesOnClose)}
        >
          <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {$settings.cleanupWorktreesOnClose
              ? 'left-[18px] bg-accent'
              : 'left-0.5 bg-text-secondary'}"></div>
        </button>
      </div>
    </section>

    <!-- Terminal -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Terminal</h3>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Font size</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-15 text-right focus:border-accent-dim"
          type="number"
          value={$settings.fontSize}
          oninput={(e) => updateSetting("fontSize", parseInt(e.currentTarget.value) || 14)}
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Font family</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.fontFamily}
          oninput={(e) => updateSetting("fontFamily", e.currentTarget.value)}
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Scrollback lines</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-20 text-right focus:border-accent-dim"
          type="number"
          value={$settings.scrollback}
          oninput={(e) => updateSetting("scrollback", parseInt(e.currentTarget.value) || 5000)}
        />
      </div>
    </section>

    <!-- Sessions -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Sessions</h3>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Confirm on close</div>
          <div class="text-[11px] text-text-muted mt-0.5">Prompt before closing active sessions</div>
        </div>
        <button
          class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
            {$settings.confirmOnClose
              ? 'bg-accent-dim border-accent'
              : 'bg-bg-deep border-border'}"
          onclick={() => updateSetting("confirmOnClose", !$settings.confirmOnClose)}
        >
          <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {$settings.confirmOnClose
              ? 'left-[18px] bg-accent'
              : 'left-0.5 bg-text-secondary'}"></div>
        </button>
      </div>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Restore on launch</div>
          <div class="text-[11px] text-text-muted mt-0.5">Show previous sessions on startup</div>
        </div>
        <button
          class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
            {$settings.restoreSessionsOnLaunch
              ? 'bg-accent-dim border-accent'
              : 'bg-bg-deep border-border'}"
          onclick={() => updateSetting("restoreSessionsOnLaunch", !$settings.restoreSessionsOnLaunch)}
        >
          <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {$settings.restoreSessionsOnLaunch
              ? 'left-[18px] bg-accent'
              : 'left-0.5 bg-text-secondary'}"></div>
        </button>
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Default project path</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.defaultProjectPath ?? ""}
          oninput={(e) => updateSetting("defaultProjectPath", e.currentTarget.value || null)}
          placeholder="~/src"
        />
      </div>
    </section>

    <!-- Claude -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Claude</h3>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Default model</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-25 text-right focus:border-accent-dim"
          value={$settings.defaultModel ?? ""}
          oninput={(e) => updateSetting("defaultModel", e.currentTarget.value || null)}
          placeholder="opus"
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Additional flags</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.additionalFlags.join(" ")}
          oninput={(e) => updateSetting("additionalFlags", e.currentTarget.value.split(" ").filter(Boolean))}
          placeholder="--verbose"
        />
      </div>
    </section>
  </div>
</div>
