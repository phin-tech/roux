<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { sessionState } from "$lib/stores/sessions";
  import { updateSetting } from "$lib/stores/settings";
  import { quitApp } from "$lib/tauri";
  import { flushPaneState } from "$lib/panes/persistence";

  interface Props {
    visible: boolean;
    oncancel: () => void;
  }

  let { visible, oncancel }: Props = $props();
  let quitting = $state(false);
  let skipNextTime = $state(false);

  let activeSessions = $derived(
    $sessionState.sessions.filter((s) => s.status !== "disconnected")
  );

  let busySessions = $derived(
    activeSessions.filter((s) => s.status === "thinking" || s.status === "generating")
  );

  async function handleQuit() {
    quitting = true;
    if (skipNextTime) {
      updateSetting("confirmOnQuit", false);
    }
    // Do NOT remove sessions — the Rust quit_app command kills PTYs and persists
    // session state so sessions reload as "disconnected" on next launch. Removing
    // them here empties sessions.json and breaks restore.
    try { await flushPaneState(); } catch {}
    quitApp();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      oncancel();
    }
    if (e.key === "Enter") {
      e.preventDefault();
      handleQuit();
    }
  }
</script>

{#if visible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-[60] flex items-center justify-center bg-black/65 backdrop-blur-md"
    onclick={(e) => { if (e.target === e.currentTarget) oncancel(); }}
    onkeydown={handleKeydown}
    transition:fade={{ duration: 150 }}
  >
    <div
      class="ui-dialog w-[400px] rounded-2xl"
      transition:scale={{ duration: 150, start: 0.96 }}
    >
      <div class="border-b border-hairline bg-bg-surface/30 px-6 pt-5 pb-4">
        <h2 class="mb-1 text-base font-semibold tracking-tight text-text-primary">Quit Roux?</h2>
        <p class="text-xs text-text-muted">
          {#if busySessions.length > 0}
            {busySessions.length} session{busySessions.length === 1 ? " is" : "s are"} still working.
          {:else}
            {activeSessions.length} active session{activeSessions.length === 1 ? "" : "s"} will be closed.
          {/if}
        </p>
      </div>

      {#if activeSessions.length > 0}
        <div class="px-6 py-4 flex flex-col gap-1.5 max-h-40 overflow-y-auto">
          {#each activeSessions as session}
            <div class="flex items-center gap-2 text-xs">
              <span class="inline-block h-1.5 w-1.5 rounded-full {session.status === 'thinking' || session.status === 'generating' ? 'bg-amber animate-pulse' : 'bg-green'}"></span>
              <span class="font-medium text-text-secondary truncate">{session.name}</span>
              <span class="text-text-muted ml-auto">{session.status}</span>
            </div>
          {/each}
        </div>
      {/if}

      <div class="px-6 pb-1">
        <label class="flex items-center gap-2 cursor-pointer group">
          <input
            type="checkbox"
            bind:checked={skipNextTime}
            class="w-3.5 h-3.5 rounded border border-border bg-bg-deep accent-accent cursor-pointer"
          />
          <span class="text-[11px] text-text-muted group-hover:text-text-secondary transition-colors">
            Don't ask next time
          </span>
        </label>
      </div>

      <div class="flex justify-end gap-2 border-t border-hairline px-6 py-4">
        <button
          class="cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-5 py-2 text-[13px] font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary"
          onclick={oncancel}
          disabled={quitting}
        >
          Cancel
        </button>
        <!-- svelte-ignore a11y_autofocus -->
        <button
          class="cursor-pointer rounded-xl border border-red/20 bg-red/15 px-5 py-2 text-[13px] font-medium text-red hover:bg-red/24 disabled:opacity-50"
          onclick={handleQuit}
          disabled={quitting}
          autofocus
        >
          {quitting ? "Closing..." : "Quit"}
        </button>
      </div>
    </div>
  </div>
{/if}
