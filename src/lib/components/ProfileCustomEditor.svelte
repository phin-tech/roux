<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import type { SpawnProfile, StartupBehavior } from "$lib/panes/profiles";

  interface Props {
    visible: boolean;
    onclose: () => void;
    onsubmit: (profile: SpawnProfile) => void;
  }

  let { visible, onclose, onsubmit }: Props = $props();

  let name = $state("");
  let setupCommand = $state("");
  let startupCommand = $state("");
  let startupBehavior = $state<StartupBehavior>("autoRun");
  let error = $state("");

  // Reset fields whenever the modal opens fresh.
  let wasVisible = $state(false);
  $effect(() => {
    if (visible && !wasVisible) {
      name = "";
      setupCommand = "";
      startupCommand = "";
      startupBehavior = "autoRun";
      error = "";
    }
    wasVisible = visible;
  });

  function submit() {
    const trimmedStartup = startupCommand.trim();
    const trimmedSetup = setupCommand.trim();
    if (!trimmedStartup && !trimmedSetup) {
      error = "Enter at least one command (setup or startup).";
      return;
    }
    const profile: SpawnProfile = {
      id: `inline-${crypto.randomUUID()}`,
      name: name.trim() || "Custom",
      setupCommand: trimmedSetup || undefined,
      startupCommand: trimmedStartup || undefined,
      startupBehavior,
      source: "inline",
    };
    onsubmit(profile);
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onclose();
      return;
    }
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    }
  }
</script>

{#if visible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-60 flex items-center justify-center bg-black/65 backdrop-blur-md"
    onclick={(e) => {
      if (e.target === e.currentTarget) onclose();
    }}
    onkeydown={handleKey}
    transition:fade={{ duration: 150 }}
  >
    <div
      class="ui-dialog w-[480px] rounded-2xl"
      transition:scale={{ duration: 150, start: 0.96 }}
    >
      <div class="border-b border-hairline bg-bg-surface/30 px-6 pt-5 pb-4">
        <h2
          class="mb-1 text-base font-semibold tracking-tight text-text-primary"
        >
          Custom spawn profile
        </h2>
        <p class="text-xs text-text-muted">
          One-shot recipe for this pane. Save it to settings to reuse later.
        </p>
      </div>

      <div class="flex flex-col gap-4 px-6 py-5">
        <div class="flex flex-col gap-1.5">
          <label
            for="profile-custom-name"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Name <span class="font-normal normal-case tracking-normal"
              >(optional)</span
            >
          </label>
          <input
            id="profile-custom-name"
            class="rounded-md border border-border-subtle bg-bg-deep px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
            bind:value={name}
            placeholder="Dev server"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label
            for="profile-custom-setup"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Setup command <span class="font-normal normal-case tracking-normal"
              >(optional, runs first)</span
            >
          </label>
          <input
            id="profile-custom-setup"
            class="rounded-md border border-border-subtle bg-bg-deep px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
            bind:value={setupCommand}
            placeholder="./scripts/start-mcp-servers.sh"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label
            for="profile-custom-startup"
            class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
          >
            Startup command
          </label>
          <input
            id="profile-custom-startup"
            class="rounded-md border border-border-subtle bg-bg-deep px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
            bind:value={startupCommand}
            placeholder="bun run dev"
          />
        </div>

        <label class="flex items-center gap-2.5 cursor-pointer group">
          <input
            type="checkbox"
            checked={startupBehavior === "typeOnly"}
            onchange={(e) =>
              (startupBehavior = (e.currentTarget as HTMLInputElement).checked
                ? "typeOnly"
                : "autoRun")}
            class="w-4 h-4 rounded border border-border bg-bg-deep accent-amber-500 cursor-pointer"
          />
          <span
            class="text-[13px] text-text-secondary group-hover:text-text-primary transition-colors"
          >
            Type only — don't press Enter automatically
          </span>
        </label>

        {#if error}
          <p class="text-xs text-red">{error}</p>
        {/if}
      </div>

      <div class="flex justify-end gap-2 border-t border-hairline px-6 py-4">
        <button
          class="cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-5 py-2 text-[13px] font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary"
          onclick={onclose}
        >
          Cancel
        </button>
        <button
          class="cursor-pointer rounded-xl border border-accent-dim/20 bg-accent-dim/15 px-5 py-2 text-[13px] font-medium text-accent hover:bg-accent-dim/24"
          onclick={submit}
        >
          Use profile
        </button>
      </div>
    </div>
  </div>
{/if}
