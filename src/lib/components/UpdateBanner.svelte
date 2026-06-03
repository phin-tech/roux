<script lang="ts">
  import {
    updateStatus,
    performInstall,
    dismissUpdateBanner,
  } from "$lib/stores/updater";
  import { quitApp } from "$lib/tauri";

  let visible = $derived(
    $updateStatus.kind === "available" ||
      $updateStatus.kind === "downloading" ||
      $updateStatus.kind === "installed-restart-required" ||
      $updateStatus.kind === "error",
  );
</script>

{#if visible}
  <div
    class="pointer-events-auto fixed top-3 left-1/2 z-[60] -translate-x-1/2 rounded-xl border border-border-subtle bg-bg-surface/95 px-4 py-2.5 text-[12px] shadow-[0_8px_32px_rgba(2,6,23,0.55)] backdrop-blur-md"
    role="status"
  >
    {#if $updateStatus.kind === "available"}
      <div class="flex items-center gap-3">
        <div>
          <div class="font-semibold text-text-primary">
            Roux {$updateStatus.version} is available
          </div>
          <div class="text-[11px] text-text-muted">
            Install now to get the latest version.
          </div>
        </div>
        <button
          class="rounded border border-accent bg-accent-dim px-2.5 py-1 text-[11px] font-semibold text-text-primary hover:bg-accent/40"
          onclick={() => void performInstall()}>Install and restart</button
        >
        <button
          class="rounded border border-border px-2.5 py-1 text-[11px] text-text-secondary hover:bg-bg-hover"
          onclick={dismissUpdateBanner}>Later</button
        >
      </div>
    {:else if $updateStatus.kind === "downloading"}
      <div class="flex items-center gap-3">
        <div class="text-text-primary">
          Downloading update{$updateStatus.progress !== null
            ? ` (${Math.round($updateStatus.progress * 100)}%)`
            : "…"}
        </div>
        <div class="h-1 w-40 overflow-hidden rounded bg-bg-deep">
          <div
            class="h-full bg-accent transition-[width] duration-200"
            style="width: {$updateStatus.progress !== null
              ? Math.round($updateStatus.progress * 100)
              : 30}%"
          ></div>
        </div>
      </div>
    {:else if $updateStatus.kind === "installed-restart-required"}
      <div class="flex items-center gap-3">
        <div>
          <div class="font-semibold text-text-primary">Update installed</div>
          <div class="text-[11px] text-text-muted">
            Quit and reopen Roux to finish.
          </div>
        </div>
        <button
          class="rounded border border-accent bg-accent-dim px-2.5 py-1 text-[11px] font-semibold text-text-primary hover:bg-accent/40"
          onclick={() => void quitApp()}>Quit Roux</button
        >
        <button
          class="rounded border border-border px-2.5 py-1 text-[11px] text-text-secondary hover:bg-bg-hover"
          onclick={dismissUpdateBanner}>Later</button
        >
      </div>
    {:else if $updateStatus.kind === "error"}
      <div class="flex items-center gap-3">
        <div class="text-red">
          {#if $updateStatus.reason === "signature-invalid"}
            Update signature invalid — download blocked. Please report this.
          {:else if $updateStatus.reason === "network"}
            Couldn't reach the update server.
          {:else}
            Update check failed.
          {/if}
        </div>
        <button
          class="rounded border border-border px-2.5 py-1 text-[11px] text-text-secondary hover:bg-bg-hover"
          onclick={dismissUpdateBanner}>Dismiss</button
        >
      </div>
    {/if}
  </div>
{/if}
