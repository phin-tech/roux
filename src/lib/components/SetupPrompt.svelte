<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { runSetup } from "$lib/tauri";

  interface Props {
    visible: boolean;
    ghAvailable?: boolean;
    ondone: () => void;
  }

  let { visible, ghAvailable = true, ondone }: Props = $props();

  let installing = $state(false);
  let error = $state("");

  async function handleInstall() {
    installing = true;
    error = "";
    try {
      await runSetup();
      ondone();
    } catch (e: any) {
      error = e?.toString() ?? "Installation failed";
      installing = false;
    }
  }

  function handleSkip() {
    ondone();
  }
</script>

{#if visible}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-md"
    transition:fade={{ duration: 150 }}
  >
    <div
      class="ui-dialog w-[440px] rounded-2xl"
      transition:scale={{ duration: 150, start: 0.96 }}
    >
      <!-- Header -->
      <div class="border-b border-hairline bg-bg-surface/30 px-6 pt-5 pb-4">
        <div class="mb-3 flex h-10 w-10 items-center justify-center rounded-xl border border-border-subtle bg-bg-surface/80 text-accent">
          <span class="text-xl">&#9095;</span>
        </div>
        <h2 class="mb-1 text-base font-semibold tracking-tight text-text-primary">Welcome to Roux</h2>
        <p class="text-xs text-text-muted">One quick thing before you get started</p>
      </div>

      <!-- Body -->
      <div class="px-6 py-5 flex flex-col gap-3">
        <p class="text-sm leading-relaxed text-text-secondary">
          Roux uses a small CLI tool to track your Claude Code session status
          (working, idle, waiting for input, etc).
        </p>
        <p class="text-sm leading-relaxed text-text-secondary">
          This will:
        </p>
        <ul class="ml-4 list-disc text-sm leading-relaxed text-text-secondary space-y-1">
          <li>Use the bundled <code class="rounded bg-bg-surface/60 px-1.5 py-0.5 text-xs text-text-primary">roux-cli</code> companion binary</li>
          <li>Add hooks to your Claude Code settings</li>
        </ul>

        {#if !ghAvailable}
          <div class="mt-2 border border-amber/15 bg-amber/8 px-3 py-2">
            <p class="text-xs text-amber">
              <span class="font-semibold">GitHub CLI not found.</span>
              GitHub Actions watches require <code class="rounded bg-bg-surface/60 px-1 py-0.5 text-[11px]">gh</code> to be installed.
              You can install it later and rerun setup when you need it.
            </p>
          </div>
        {/if}

        {#if error}
          <p class="text-xs text-red-400">{error}</p>
        {/if}
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-end gap-2 border-t border-hairline px-6 py-4">
        <button
          class="ui-btn-ghost rounded-lg px-4 py-2 text-sm"
          onclick={handleSkip}
          disabled={installing}
        >
          Skip for now
        </button>
        <button
          class="ui-btn-primary rounded-lg px-4 py-2 text-sm font-medium"
          onclick={handleInstall}
          disabled={installing}
        >
          {installing ? "Installing…" : "Install"}
        </button>
      </div>
    </div>
  </div>
{/if}
